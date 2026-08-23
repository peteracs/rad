pub use rad_syntax::native_types::{NativeScalarKind, NativeTypeFlavor};

/// A runtime native scalar. `bits` stores the exact representation (including
/// f32/f64 NaN payload canonicalization performed at construction).
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct NativeScalarValue {
    /// Shared with the declaring [`NativeTypeDescriptor`].
    ///
    /// This was an owned `String`, so every stored value carried its own copy
    /// of its type's name: a world holding a million `TransactionId`s held a
    /// million copies of `"TransactionId"`. Measured at ~50 bytes per stored
    /// value for a short name, and it grew byte-for-byte with the name.
    /// Sharing the declaration's allocation makes the cost one per type.
    pub type_name: std::sync::Arc<str>,
    pub repr: NativeScalarKind,
    pub flavor: NativeTypeFlavor,
    pub bits: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTypeDescriptor {
    /// The single owner of this type's name; values clone the `Arc`.
    pub name: std::sync::Arc<str>,
    pub repr: NativeScalarKind,
    pub flavor: NativeTypeFlavor,
    pub members: Vec<(String, u64)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFieldLayout {
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub align: usize,
}

/// An exact, platform-independent RAD `repr(C)` layout. RAD fixes the scalar
/// widths and uses the ordinary C field-alignment algorithm; unlike Rust's
/// host `Layout`, this result is identical on every VM target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeLayout {
    pub name: String,
    pub size: usize,
    pub align: usize,
    pub packed: bool,
    pub fields: Vec<NativeFieldLayout>,
}

fn align_up(value: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (value + align - 1) & !(align - 1)
}

/// Resolve every ABI-visible struct as one closed semantic product. Generic,
/// function, union, and ordinary RAD fields have no native ABI and are
/// rejected instead of being assigned an implementation-dependent layout.
pub fn compute_native_layouts(
    program: &crate::ast::Program,
) -> Result<std::collections::HashMap<String, NativeLayout>, String> {
    use crate::ast::{Decl, TypeExpr};
    use std::collections::{HashMap, HashSet};

    let native = program
        .declarations
        .iter()
        .filter_map(|decl| match decl {
            Decl::NativeType(item) => Some((item.name.clone(), item.repr)),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let structs = program
        .declarations
        .iter()
        .filter_map(|decl| match decl {
            Decl::Struct(item) if item.repr_c || item.packed => Some((item.name.clone(), item)),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let mut layouts = HashMap::new();
    let mut visiting = HashSet::new();

    fn layout_one(
        name: &str,
        native: &HashMap<String, NativeScalarKind>,
        structs: &HashMap<String, &crate::ast::DataDecl>,
        layouts: &mut HashMap<String, NativeLayout>,
        visiting: &mut HashSet<String>,
    ) -> Result<(usize, usize), String> {
        if let Some(layout) = layouts.get(name) {
            return Ok((layout.size, layout.align));
        }
        if let Some(kind) = NativeScalarKind::parse(name).or_else(|| native.get(name).copied()) {
            let width = kind.byte_width();
            return Ok((width, width));
        }
        let item = structs
            .get(name)
            .ok_or_else(|| format!("{name} is not a native scalar or repr(C) struct"))?;
        if !item.repr_c {
            return Err(format!("packed struct {name} must also declare repr(C)"));
        }
        if !visiting.insert(name.to_string()) {
            return Err(format!("recursive native layout involving {name}"));
        }
        let mut fields = Vec::with_capacity(item.fields.len());
        let mut offset = 0usize;
        let mut struct_align = 1usize;
        for field in &item.fields {
            let field_type = match &field.type_annotation {
                Some(TypeExpr::Named(field_type)) => field_type,
                Some(_) => {
                    return Err(format!(
                        "repr(C) field {}.{} must use a fixed-width native or repr(C) named type",
                        name, field.name
                    ))
                }
                None => {
                    return Err(format!(
                        "repr(C) field {}.{} requires an explicit native type",
                        name, field.name
                    ))
                }
            };
            let (size, natural_align) = layout_one(field_type, native, structs, layouts, visiting)?;
            let align = if item.packed { 1 } else { natural_align };
            offset = align_up(offset, align);
            fields.push(NativeFieldLayout {
                name: field.name.clone(),
                offset,
                size,
                align,
            });
            offset = offset
                .checked_add(size)
                .ok_or_else(|| format!("native layout for {name} exceeds addressable size"))?;
            struct_align = struct_align.max(align);
        }
        let size = align_up(offset, struct_align);
        visiting.remove(name);
        layouts.insert(
            name.to_string(),
            NativeLayout {
                name: name.to_string(),
                size,
                align: struct_align,
                packed: item.packed,
                fields,
            },
        );
        Ok((size, struct_align))
    }

    let names = structs.keys().cloned().collect::<Vec<_>>();
    for name in names {
        layout_one(&name, &native, &structs, &mut layouts, &mut visiting)?;
    }
    Ok(layouts)
}

impl NativeTypeDescriptor {
    pub fn scalar(repr: NativeScalarKind) -> Self {
        Self {
            name: repr.to_string().into(),
            repr,
            flavor: NativeTypeFlavor::Scalar,
            members: Vec::new(),
        }
    }

    pub fn member_bits(&self, member: &str) -> Option<u64> {
        self.members
            .iter()
            .find_map(|(name, bits)| (name == member).then_some(*bits))
    }

    pub fn accepts_bits(&self, bits: u64) -> bool {
        match self.flavor {
            NativeTypeFlavor::Scalar | NativeTypeFlavor::Opaque => true,
            NativeTypeFlavor::Enum => self.members.iter().any(|(_, value)| *value == bits),
            NativeTypeFlavor::Bitflags => {
                let admitted = self
                    .members
                    .iter()
                    .fold(0_u64, |mask, (_, value)| mask | *value);
                bits & !admitted == 0
            }
        }
    }
}

impl From<&crate::ast::NativeTypeDecl> for NativeTypeDescriptor {
    fn from(value: &crate::ast::NativeTypeDecl) -> Self {
        Self {
            name: value.name.as_str().into(),
            repr: value.repr,
            flavor: value.flavor,
            members: value.members.clone(),
        }
    }
}

impl NativeScalarValue {
    pub fn unsigned(&self) -> Option<u64> {
        (!self.repr.is_float()).then_some(match self.repr {
            NativeScalarKind::U8 => self.bits as u8 as u64,
            NativeScalarKind::U16 => self.bits as u16 as u64,
            NativeScalarKind::U32 => self.bits as u32 as u64,
            NativeScalarKind::U64 => self.bits,
            NativeScalarKind::I8 => (self.bits as i8) as i64 as u64,
            NativeScalarKind::I16 => (self.bits as i16) as i64 as u64,
            NativeScalarKind::I32 => (self.bits as i32) as i64 as u64,
            NativeScalarKind::I64 => self.bits,
            NativeScalarKind::F32 | NativeScalarKind::F64 => unreachable!(),
        })
    }

    pub fn signed(&self) -> Option<i64> {
        (!self.repr.is_float()).then_some(match self.repr {
            NativeScalarKind::U8 => self.bits as u8 as i64,
            NativeScalarKind::U16 => self.bits as u16 as i64,
            NativeScalarKind::U32 => self.bits as u32 as i64,
            NativeScalarKind::U64 => self.bits as i64,
            NativeScalarKind::I8 => self.bits as i8 as i64,
            NativeScalarKind::I16 => self.bits as i16 as i64,
            NativeScalarKind::I32 => self.bits as i32 as i64,
            NativeScalarKind::I64 => self.bits as i64,
            NativeScalarKind::F32 | NativeScalarKind::F64 => unreachable!(),
        })
    }

    pub fn float(&self) -> Option<f64> {
        match self.repr {
            NativeScalarKind::F32 => Some(f32::from_bits(self.bits as u32) as f64),
            NativeScalarKind::F64 => Some(f64::from_bits(self.bits)),
            _ => None,
        }
    }
}

pub fn checked_native_neg(value: &NativeScalarValue) -> Result<NativeScalarValue, String> {
    if value.flavor != NativeTypeFlavor::Scalar {
        return Err(format!(
            "negation is not defined for nominal type {}",
            value.type_name
        ));
    }
    let bits = match value.repr {
        NativeScalarKind::F32 => {
            let result = -f32::from_bits(value.bits as u32);
            if result.is_nan() {
                f32::NAN.to_bits() as u64
            } else {
                result.to_bits() as u64
            }
        }
        NativeScalarKind::F64 => {
            let result = -f64::from_bits(value.bits);
            if result.is_nan() {
                f64::NAN.to_bits()
            } else {
                result.to_bits()
            }
        }
        kind if kind.is_signed() => {
            let signed = value.signed().unwrap();
            let negated = signed
                .checked_neg()
                .ok_or_else(|| format!("{} arithmetic overflow", value.type_name))?;
            let admitted = match kind {
                NativeScalarKind::I8 => i8::try_from(negated).is_ok(),
                NativeScalarKind::I16 => i16::try_from(negated).is_ok(),
                NativeScalarKind::I32 => i32::try_from(negated).is_ok(),
                NativeScalarKind::I64 => true,
                _ => unreachable!(),
            };
            if !admitted {
                return Err(format!("{} arithmetic overflow", value.type_name));
            }
            negated as u64
        }
        _ => return Err(format!("negation is not defined for {}", value.type_name)),
    };
    Ok(NativeScalarValue {
        bits,
        ..value.clone()
    })
}

pub fn checked_native_bit_not(value: &NativeScalarValue) -> Result<NativeScalarValue, String> {
    if value.repr.is_float()
        || !matches!(
            value.flavor,
            NativeTypeFlavor::Scalar | NativeTypeFlavor::Bitflags
        )
    {
        return Err(format!(
            "bitwise not is not defined for {}",
            value.type_name
        ));
    }
    let width = value.repr.byte_width() * 8;
    let mask = if width == 64 {
        u64::MAX
    } else {
        (1_u64 << width) - 1
    };
    let bits = !value.bits & mask;
    if value.flavor == NativeTypeFlavor::Bitflags {
        return Err(format!(
            "bitwise not would construct undeclared bits for {}; use named flags",
            value.type_name
        ));
    }
    Ok(NativeScalarValue {
        bits,
        ..value.clone()
    })
}

pub fn decode_native_scalar(
    descriptor: &NativeTypeDescriptor,
    bytes: &[u8],
    little_endian: bool,
) -> Result<NativeScalarValue, String> {
    let width = descriptor.repr.byte_width();
    if bytes.len() != width {
        return Err(format!(
            "{} decoding requires exactly {} bytes, got {}",
            descriptor.name,
            width,
            bytes.len()
        ));
    }
    let mut lane = [0_u8; 8];
    if little_endian {
        lane[..width].copy_from_slice(bytes);
    } else {
        lane[8 - width..].copy_from_slice(bytes);
    }
    let bits = if little_endian {
        u64::from_le_bytes(lane)
    } else {
        u64::from_be_bytes(lane)
    };
    let bits = match descriptor.repr {
        NativeScalarKind::F32 if f32::from_bits(bits as u32).is_nan() => f32::NAN.to_bits() as u64,
        NativeScalarKind::F64 if f64::from_bits(bits).is_nan() => f64::NAN.to_bits(),
        _ => bits,
    };
    if !descriptor.accepts_bits(bits) {
        return Err(format!(
            "decoded bits 0x{bits:x} are not admitted by {}",
            descriptor.name
        ));
    }
    Ok(NativeScalarValue {
        type_name: descriptor.name.clone(),
        repr: descriptor.repr,
        flavor: descriptor.flavor,
        bits,
    })
}

pub fn encode_native_scalar(value: &NativeScalarValue, little_endian: bool) -> Vec<u8> {
    let width = value.repr.byte_width();
    let lane = if little_endian {
        value.bits.to_le_bytes()
    } else {
        value.bits.to_be_bytes()
    };
    if little_endian {
        lane[..width].to_vec()
    } else {
        lane[8 - width..].to_vec()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum NativeBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

pub fn checked_native_binary(
    left: &NativeScalarValue,
    right: &NativeScalarValue,
    op: NativeBinaryOp,
) -> Result<NativeScalarValue, String> {
    if left.type_name != right.type_name || left.repr != right.repr || left.flavor != right.flavor {
        return Err(format!(
            "native operation requires identical types, got {} and {}",
            left.type_name, right.type_name
        ));
    }
    let arithmetic = matches!(
        op,
        NativeBinaryOp::Add
            | NativeBinaryOp::Sub
            | NativeBinaryOp::Mul
            | NativeBinaryOp::Div
            | NativeBinaryOp::Mod
    );
    let bitwise = !arithmetic;
    if arithmetic && left.flavor != NativeTypeFlavor::Scalar {
        return Err(format!(
            "arithmetic is not defined for nominal type {}",
            left.type_name
        ));
    }
    if bitwise
        && !matches!(
            left.flavor,
            NativeTypeFlavor::Scalar | NativeTypeFlavor::Bitflags
        )
    {
        return Err(format!(
            "bitwise operations are not defined for {}",
            left.type_name
        ));
    }
    if left.repr.is_float() {
        if bitwise {
            return Err(format!(
                "bitwise operations are not defined for {}",
                left.repr
            ));
        }
        let a = left.float().unwrap();
        let b = right.float().unwrap();
        if matches!(op, NativeBinaryOp::Div | NativeBinaryOp::Mod) && b == 0.0 {
            return Err("Division by zero".to_string());
        }
        let value = match op {
            NativeBinaryOp::Add => a + b,
            NativeBinaryOp::Sub => a - b,
            NativeBinaryOp::Mul => a * b,
            NativeBinaryOp::Div => a / b,
            NativeBinaryOp::Mod => a % b,
            _ => unreachable!(),
        };
        let bits = if left.repr == NativeScalarKind::F32 {
            let value = value as f32;
            if value.is_nan() {
                f32::NAN.to_bits() as u64
            } else {
                value.to_bits() as u64
            }
        } else if value.is_nan() {
            f64::NAN.to_bits()
        } else {
            value.to_bits()
        };
        return Ok(NativeScalarValue {
            bits,
            ..left.clone()
        });
    }

    let width = left.repr.byte_width() * 8;
    let mask = if width == 64 {
        u64::MAX
    } else {
        (1_u64 << width) - 1
    };
    let bits = if matches!(
        op,
        NativeBinaryOp::BitAnd
            | NativeBinaryOp::BitOr
            | NativeBinaryOp::BitXor
            | NativeBinaryOp::Shl
            | NativeBinaryOp::Shr
    ) {
        let shift = (right.bits & 0xff) as usize;
        match op {
            NativeBinaryOp::BitAnd => left.bits & right.bits,
            NativeBinaryOp::BitOr => left.bits | right.bits,
            NativeBinaryOp::BitXor => left.bits ^ right.bits,
            NativeBinaryOp::Shl => {
                if shift >= width {
                    0
                } else {
                    (left.bits << shift) & mask
                }
            }
            NativeBinaryOp::Shr if left.repr.is_signed() => {
                if shift >= width {
                    if left.signed().unwrap() < 0 {
                        mask
                    } else {
                        0
                    }
                } else {
                    ((left.signed().unwrap() >> shift) as u64) & mask
                }
            }
            NativeBinaryOp::Shr => {
                if shift >= width {
                    0
                } else {
                    left.bits >> shift
                }
            }
            _ => unreachable!(),
        }
    } else if left.repr.is_signed() {
        let a = left.signed().unwrap() as i128;
        let b = right.signed().unwrap() as i128;
        if matches!(op, NativeBinaryOp::Div | NativeBinaryOp::Mod) && b == 0 {
            return Err("Division by zero".to_string());
        }
        let value = match op {
            NativeBinaryOp::Add => a.checked_add(b),
            NativeBinaryOp::Sub => a.checked_sub(b),
            NativeBinaryOp::Mul => a.checked_mul(b),
            NativeBinaryOp::Div => a.checked_div(b),
            NativeBinaryOp::Mod => a.checked_rem(b),
            _ => unreachable!(),
        }
        .ok_or_else(|| format!("{} arithmetic overflow", left.type_name))?;
        let (min, max) = match left.repr {
            NativeScalarKind::I8 => (i8::MIN as i128, i8::MAX as i128),
            NativeScalarKind::I16 => (i16::MIN as i128, i16::MAX as i128),
            NativeScalarKind::I32 => (i32::MIN as i128, i32::MAX as i128),
            NativeScalarKind::I64 => (i64::MIN as i128, i64::MAX as i128),
            _ => unreachable!(),
        };
        if !(min..=max).contains(&value) {
            return Err(format!("{} arithmetic overflow", left.type_name));
        }
        (value as i64 as u64) & mask
    } else {
        let a = left.unsigned().unwrap() as u128;
        let b = right.unsigned().unwrap() as u128;
        if matches!(op, NativeBinaryOp::Div | NativeBinaryOp::Mod) && b == 0 {
            return Err("Division by zero".to_string());
        }
        let value = match op {
            NativeBinaryOp::Add => a.checked_add(b),
            NativeBinaryOp::Sub => a.checked_sub(b),
            NativeBinaryOp::Mul => a.checked_mul(b),
            NativeBinaryOp::Div => a.checked_div(b),
            NativeBinaryOp::Mod => a.checked_rem(b),
            _ => unreachable!(),
        }
        .ok_or_else(|| format!("{} arithmetic overflow", left.type_name))?;
        if value > mask as u128 {
            return Err(format!("{} arithmetic overflow", left.type_name));
        }
        value as u64
    };
    Ok(NativeScalarValue {
        bits: bits & mask,
        ..left.clone()
    })
}

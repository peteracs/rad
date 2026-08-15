#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentData {
    pub type_name: String,
    pub layout: Arc<Vec<String>>,
    pub(crate) values: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateInst {
    pub machine: String,
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SumTypeInst {
    pub type_name: String,
    pub variant: String,
    pub(crate) fields: HashMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FnValue {
    pub name: String,
    pub arity: u8,
    pub chunk_id: usize,
}

/// A closure captures zero or more `CaptureCell` pointers.
///
/// Each pointer is a GC-managed raw pointer to a `CaptureCell`.
/// Multiple closures may share the same cell (aliased raw pointers;
/// the GC keeps cells alive as long as any closure referencing them
/// is reachable).
#[derive(Clone, Debug)]
pub struct ClosureValue {
    pub name: String,
    pub arity: u8,
    pub chunk_id: usize,
    pub captures: Vec<*mut gc::CaptureCell>,
}

unsafe impl Send for ClosureValue {}
unsafe impl Sync for ClosureValue {}

impl PartialEq for ClosureValue {
    fn eq(&self, other: &Self) -> bool {
        self.chunk_id == other.chunk_id
            && self.arity == other.arity
            && self.captures.len() == other.captures.len()
            && self
                .captures
                .iter()
                .zip(other.captures.iter())
                .all(|(&a, &b)| unsafe { (*a).get() == (*b).get() })
    }
}

impl Eq for ClosureValue {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipelineOp {
    Map,
    Filter,
}

impl fmt::Display for ComponentData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {{", display_type_name(&self.type_name))?;
        if !self.layout.is_empty() {
            write!(f, " ")?;
            let mut first = true;
            for (k, v) in self.layout.iter().zip(self.values.iter()) {
                if !first {
                    write!(f, ", ")?;
                }
                first = false;
                write!(f, "{}: {}", k, v)?;
            }
            write!(f, " ")?;
        }
        write!(f, "}}")
    }
}

#[cfg(test)]
mod tests {
    use super::{Builtin, Value, ValueTag, CANONICAL_FLOAT_NAN};
    use std::collections::HashSet;

    #[test]
    fn builtin_all_has_unique_names() {
        let mut seen = HashSet::new();
        for builtin in Builtin::ALL {
            assert!(
                seen.insert(builtin.name()),
                "duplicate builtin registration: {}",
                builtin.name()
            );
            assert_eq!(
                Builtin::from_name(builtin.name()),
                Some(builtin),
                "builtin name lookup must be generated from the same catalog"
            );
        }
    }

    #[test]
    fn builtin_signature_purity_matches_canonical_effect_metadata() {
        for builtin in Builtin::ALL {
            let name = builtin.name();
            let signature = crate::builtins::builtin_type_scheme(name)
                .unwrap_or_else(|| panic!("missing builtin signature: {name}"));
            assert_eq!(
                signature.is_pure,
                crate::builtins::builtin_effect(name).is_pure(),
                "builtin purity drift: {name}"
            );
        }
    }

    #[test]
    fn every_float_nan_is_canonical_and_never_object_tagged() {
        let reserved_and_ieee_patterns = [
            0x7FF0_0000_0000_0001,
            0x7FF8_0000_0000_0000,
            0x7FFC_0000_0000_0000,
            0x7FFF_FFFF_FFFF_FFFF,
            0xFFF0_0000_0000_0001,
            0xFFF8_0000_0000_0000,
            0xFFFC_0000_0000_0000,
            0xFFFC_0000_0000_0001,
            0xFFFC_7FFF_FFFF_FFFF,
            0xFFFC_8000_0000_0000,
            0xFFFF_FFFF_FFFF_FFFF,
        ];

        for bits in reserved_and_ieee_patterns {
            let input = f64::from_bits(bits);
            assert!(input.is_nan(), "test pattern {bits:#018x} must be NaN");
            let value = Value::from_float(input);
            assert_eq!(value.to_raw(), CANONICAL_FLOAT_NAN, "input {bits:#018x}");
            assert_eq!(value.tag(), ValueTag::Float, "input {bits:#018x}");
            assert!(value.is_float(), "input {bits:#018x}");
            assert!(!value.is_heap_object_tag(), "input {bits:#018x}");
            assert!(value.as_float().is_some_and(f64::is_nan));
        }

        let zero = std::hint::black_box(0.0_f64);
        for arithmetic_nan in [
            zero / zero,
            f64::INFINITY - f64::INFINITY,
            (-1.0_f64).sqrt(),
        ] {
            let value = Value::from_float(arithmetic_nan);
            assert_eq!(value.to_raw(), CANONICAL_FLOAT_NAN);
            assert_eq!(value.tag(), ValueTag::Float);
        }
    }
}

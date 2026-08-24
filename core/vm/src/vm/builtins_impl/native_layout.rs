fn abi_type_name<'a>(value: &'a Value, caller: &str) -> Result<&'a str, String> {
    if let Some(name) = value.as_str() {
        return Ok(name);
    }
    if let Some(component) = value.as_component() {
        return Ok(&component.type_name);
    }
    Err(format!(
        "{caller} expects a repr(C) type name or value, got {}",
        value.type_name()
    ))
}

/// Encode one closed native value. Scalars carry their exact descriptor;
/// `repr(C)` structs carry ordinary component data plus the compiler's sealed
/// layout. Padding is deterministically zeroed because RAD values have no
/// addressable, uninitialized padding bytes.
fn native_encoded_width(
    value: &Value,
    layouts: &std::collections::HashMap<String, crate::native_types::NativeLayout>,
) -> Result<usize, String> {
    if let Some(scalar) = value.as_native_scalar() {
        return Ok(scalar.repr.byte_width());
    }
    let component = value.as_component().ok_or_else(|| {
        format!(
            "native encoding requires a fixed-width native scalar or repr(C) struct, got {}",
            value.type_name()
        )
    })?;
    layouts
        .get(&component.type_name)
        .map(|layout| layout.size)
        .ok_or_else(|| {
            format!(
                "native encoding requires a repr(C) struct, but {} has no native layout",
                component.type_name
            )
        })
}

fn encode_native_value_into(
    value: &Value,
    little_endian: bool,
    layouts: &std::collections::HashMap<String, crate::native_types::NativeLayout>,
    destination: &mut [u8],
) -> Result<(), String> {
    if let Some(scalar) = value.as_native_scalar() {
        return crate::native_types::encode_native_scalar_into(
            scalar,
            little_endian,
            destination,
        );
    }

    let component = value.as_component().ok_or_else(|| {
        format!(
            "native encoding requires a fixed-width native scalar or repr(C) struct, got {}",
            value.type_name()
        )
    })?;
    let layout = layouts.get(&component.type_name).ok_or_else(|| {
        format!(
            "native encoding requires a repr(C) struct, but {} has no native layout",
            component.type_name
        )
    })?;
    if destination.len() != layout.size {
        return Err(format!(
            "native encoding width mismatch for {}: destination is {} bytes, repr(C) layout is {}",
            component.type_name,
            destination.len(),
            layout.size
        ));
    }
    if component.layout.len() != layout.fields.len()
        || component.values.len() != layout.fields.len()
    {
        return Err(format!(
            "native encoding layout mismatch for {}: value has {} fields, repr(C) layout has {}",
            component.type_name,
            component.values.len(),
            layout.fields.len()
        ));
    }

    destination.fill(0);
    for ((field_name, value), field) in component
        .layout
        .iter()
        .zip(&component.values)
        .zip(&layout.fields)
    {
        if field_name != &field.name {
            return Err(format!(
                "native encoding layout mismatch for {}: value field {} occupies repr(C) slot {}",
                component.type_name, field_name, field.name
            ));
        }
        let end = field.offset.checked_add(field.size).ok_or_else(|| {
            format!(
                "native encoding offset overflow for {}.{}",
                component.type_name, field.name
            )
        })?;
        encode_native_value_into(
            value,
            little_endian,
            layouts,
            &mut destination[field.offset..end],
        )
        .map_err(|error| format!("{}.{}: {error}", component.type_name, field.name))?;
    }
    Ok(())
}

impl VM {
    fn bi_size_of(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!("size_of() expects 1 argument, got {}", args.len()));
        }
        let size = if let Some(native) = args[0].as_native_type() {
            native.repr.byte_width()
        } else {
            let name = abi_type_name(&args[0], "size_of()")?;
            self.native_layouts
                .get(name)
                .ok_or_else(|| format!("size_of(): {name} has no repr(C) layout"))?
                .size
        };
        Ok(Value::from_int(&mut self.gc, size as i64))
    }

    fn bi_offset_of(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!("offset_of() expects 2 arguments, got {}", args.len()));
        }
        let name = abi_type_name(&args[0], "offset_of()")?;
        let field = args[1]
            .as_str()
            .ok_or_else(|| "offset_of() field must be a string".to_string())?;
        let layout = self
            .native_layouts
            .get(name)
            .ok_or_else(|| format!("offset_of(): {name} has no repr(C) layout"))?;
        let offset = layout
            .fields
            .iter()
            .find_map(|candidate| (candidate.name == field).then_some(candidate.offset))
            .ok_or_else(|| format!("offset_of(): {name} has no field {field}"))?;
        Ok(Value::from_int(&mut self.gc, offset as i64))
    }

    fn bi_decode_native(
        &mut self,
        args: Vec<Value>,
        little_endian: bool,
    ) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "decode_{}() expects 3 arguments, got {}",
                if little_endian { "le" } else { "be" },
                args.len()
            ));
        }
        let bytes = args[0]
            .as_bytebuf()
            .ok_or_else(|| "native decoding expects a bytebuf".to_string())?;
        let offset = args[1]
            .as_int()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| "native decoding offset must be a non-negative integer".to_string())?;
        let descriptor = args[2]
            .as_native_type()
            .cloned()
            .ok_or_else(|| "native decoding requires a native type as its third argument".to_string())?;
        let end = offset
            .checked_add(descriptor.repr.byte_width())
            .ok_or_else(|| "native decoding offset overflow".to_string())?;
        let lane = bytes.get(offset..end).ok_or_else(|| {
            format!(
                "native decoding range {offset}..{end} exceeds bytebuf length {}",
                bytes.len()
            )
        })?;
        let value = crate::native_types::decode_native_scalar(&descriptor, lane, little_endian)?;
        Ok(Value::from_native_scalar(&mut self.gc, value))
    }

    fn bi_encode_native(
        &mut self,
        args: Vec<Value>,
        little_endian: bool,
    ) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "encode_{}() expects 1 argument, got {}",
                if little_endian { "le" } else { "be" },
                args.len()
            ));
        }
        let width = native_encoded_width(&args[0], &self.native_layouts)?;
        let mut bytes = vec![0; width];
        encode_native_value_into(
            &args[0],
            little_endian,
            &self.native_layouts,
            &mut bytes,
        )?;
        Ok(Value::bytebuf(&mut self.gc, bytes))
    }
}

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
        let value = args[0]
            .as_native_scalar()
            .ok_or_else(|| "native encoding requires a fixed-width native value".to_string())?;
        let bytes = crate::native_types::encode_native_scalar(value, little_endian);
        Ok(Value::bytebuf(&mut self.gc, bytes))
    }
}

// Growable text and byte buffers, including the fixed-width little-endian
// accessors that back the wire and FFI encoders.

impl VM {

    fn bi_buffer_new(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err(format!(
                "buffer_new() takes no arguments, got {}",
                args.len()
            ));
        }
        Ok(Value::buffer(&mut self.gc, String::new()))
    }

    fn bi_buffer_append(&mut self, mut args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "buffer_append() expects 2 arguments, got {}",
                args.len()
            ));
        }
        let s_val = args.pop().unwrap();
        let buf_val = args.pop().unwrap();

        let s = s_val
            .as_str()
            .ok_or_else(|| "buffer_append() second argument must be a string".to_string())?;

        let mut buf = buf_val
            .into_buffer()
            .ok_or_else(|| "buffer_append() first argument must be a buffer".to_string())?;

        buf.push_str(s);
        Ok(Value::buffer(&mut self.gc, buf))
    }

    fn bi_buffer_to_str(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "buffer_to_str() expects 1 argument, got {}",
                args.len()
            ));
        }
        let buf = args[0]
            .as_buffer()
            .ok_or_else(|| "buffer_to_str() argument must be a buffer".to_string())?;
        Ok(Value::from_string(&mut self.gc, buf.clone()))
    }

    fn bi_bytebuf_new(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "bytebuf_new() expects 1 argument, got {}",
                args.len()
            ));
        }
        let size = bytebuf_index_arg(&args[0], "bytebuf_new() size")?;
        Ok(Value::bytebuf(&mut self.gc, vec![0; size]))
    }

    fn bi_bytebuf_len(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "bytebuf_len() expects 1 argument, got {}",
                args.len()
            ));
        }
        let bytes = args[0]
            .as_bytebuf()
            .ok_or_else(|| "bytebuf_len() expects a bytebuf".to_string())?;
        Ok(Value::from_int(&mut self.gc, bytes.len() as i64))
    }

    fn bi_bytebuf_get(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "bytebuf_get() expects 2 arguments, got {}",
                args.len()
            ));
        }
        let bytes = args[0]
            .as_bytebuf()
            .ok_or_else(|| "bytebuf_get() expects a bytebuf".to_string())?;
        let idx = bytebuf_index_arg(&args[1], "bytebuf_get() index")?;
        if idx >= bytes.len() {
            return Err(format!(
                "bytebuf_get() index {} out of bounds (len {})",
                idx,
                bytes.len()
            ));
        }
        Ok(Value::from_int(&mut self.gc, i64::from(bytes[idx])))
    }

    fn bi_bytebuf_set_u8(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "bytebuf_set_u8() expects 3 arguments, got {}",
                args.len()
            ));
        }
        let mut bytes = args[0]
            .into_bytebuf()
            .ok_or_else(|| "bytebuf_set_u8() expects a bytebuf".to_string())?;
        let idx = bytebuf_index_arg(&args[1], "bytebuf_set_u8() index")?;
        let byte = bytebuf_u8_arg(&args[2], "bytebuf_set_u8() value")?;
        if idx >= bytes.len() {
            return Err(format!(
                "bytebuf_set_u8() index {} out of bounds (len {})",
                idx,
                bytes.len()
            ));
        }
        bytes[idx] = byte;
        Ok(Value::bytebuf(&mut self.gc, bytes))
    }

    fn bi_bytebuf_set_u32_le(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_bytebuf_set_u32_or_i32_le(args, "bytebuf_set_u32_le()")
    }

    fn bi_bytebuf_set_i32_le(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_bytebuf_set_u32_or_i32_le(args, "bytebuf_set_i32_le()")
    }

    fn bi_bytebuf_set_u32_or_i32_le(
        &mut self,
        args: Vec<Value>,
        fn_name: &str,
    ) -> Result<Value, String> {
        if args.len() != 3 {
            return Err(format!(
                "{} expects 3 arguments, got {}",
                fn_name,
                args.len()
            ));
        }
        let mut bytes = args[0]
            .into_bytebuf()
            .ok_or_else(|| format!("{} expects a bytebuf", fn_name))?;
        let offset = bytebuf_index_arg(&args[1], &format!("{} offset", fn_name))?;
        let value = args[2]
            .as_int()
            .ok_or_else(|| format!("{} expects int value", fn_name))?;
        bytebuf_write_u32_le(&mut bytes, offset, value as u32, fn_name)?;
        Ok(Value::bytebuf(&mut self.gc, bytes))
    }

    fn bi_bytebuf_get_u32_le(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_bytebuf_get_u32_or_i32_le(args, false, "bytebuf_get_u32_le()")
    }

    fn bi_bytebuf_get_i32_le(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.bi_bytebuf_get_u32_or_i32_le(args, true, "bytebuf_get_i32_le()")
    }

    fn bi_bytebuf_get_u32_or_i32_le(
        &mut self,
        args: Vec<Value>,
        signed: bool,
        fn_name: &str,
    ) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "{} expects 2 arguments, got {}",
                fn_name,
                args.len()
            ));
        }
        let bytes = args[0]
            .as_bytebuf()
            .ok_or_else(|| format!("{} expects a bytebuf", fn_name))?;
        let offset = bytebuf_index_arg(&args[1], &format!("{} offset", fn_name))?;
        let value = bytebuf_read_u32_le(bytes, offset, fn_name)?;
        let result = if signed {
            i64::from(value as i32)
        } else {
            i64::from(value)
        };
        Ok(Value::from_int(&mut self.gc, result))
    }

    fn bi_bytebuf_to_list(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "bytebuf_to_list() expects 1 argument, got {}",
                args.len()
            ));
        }
        let bytes = args[0]
            .as_bytebuf()
            .ok_or_else(|| "bytebuf_to_list() expects a bytebuf".to_string())?;
        let mut values = Vec::with_capacity(bytes.len());
        for byte in bytes {
            values.push(Value::from_int(&mut self.gc, i64::from(*byte)));
        }
        Ok(Value::list(&mut self.gc, values))
    }

    fn bi_bytebuf_from_list(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "bytebuf_from_list() expects 1 argument, got {}",
                args.len()
            ));
        }
        let bytes = bytes_from_list_arg(&args[0], "bytebuf_from_list()")?;
        Ok(Value::bytebuf(&mut self.gc, bytes))
    }
}

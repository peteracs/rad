

impl VM {
    fn bi_load_extension(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("load_extension() requires 2 arguments (path, timeout_ms)".into());
        }
        let path_val = &args[0];
        let path = path_val.as_str().ok_or_else(|| {
            format!(
                "load_extension() expects string, got {}",
                path_val.type_name()
            )
        })?;
        let timeout_ms = args[1].as_int().ok_or_else(|| {
            format!(
                "load_extension() timeout_ms expects int, got {}",
                args[1].type_name()
            )
        })?;
        if !(1..=crate::ffi::MAX_NATIVE_WORKER_CALL_TIMEOUT_MS as i64).contains(&timeout_ms) {
            return Err(format!(
                "load_extension() timeout_ms must be between 1 and {}",
                crate::ffi::MAX_NATIVE_WORKER_CALL_TIMEOUT_MS
            ));
        }
        let call_timeout = std::time::Duration::from_millis(timeout_ms as u64);

        #[cfg(target_arch = "wasm32")]
        {
            let _ = (path, call_timeout);
            return Err("Plugins are not supported on wasm32".to_string());
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let boundary = "native-load:v3";
            let argument_digest = crate::replay::args_digest(&args)?;
            if let Some(replayer) = self.replayer.as_mut() {
                let record = replayer.next_io(boundary, &argument_digest)?;
                let encoded = record.result?;
                let (functions, manifest) = crate::ffi::decode_recorded_plugin(encoded)?;
                let manifests = std::sync::Arc::make_mut(&mut self.native_extension_manifests);
                if !manifests
                    .iter()
                    .any(|loaded| loaded.digest() == manifest.digest())
                {
                    manifests.push(manifest);
                }
                let mut map = MapStorage::new();
                for (name, info) in functions {
                    map.insert(MapKey::Str(name), Value::from_native_fn(&mut self.gc, info));
                }
                return Ok(Value::map(&mut self.gc, map));
            }

            let (functions, lib, manifest) =
                match crate::ffi::load_plugin_isolated(path, call_timeout) {
                    Ok(loaded) => loaded,
                    Err(error) => {
                        if let Some(recorder) = self.recorder.as_mut() {
                            recorder.record_io(
                                boundary,
                                argument_digest,
                                &Err(error.clone()),
                            );
                        }
                        return Err(error);
                    }
                };

            if let Some(recorder) = self.recorder.as_mut() {
                let encoded = crate::ffi::encode_recorded_plugin(&manifest)?;
                recorder.record_io(boundary, argument_digest, &Ok(encoded));
            }

            let manifests = std::sync::Arc::make_mut(&mut self.native_extension_manifests);
            if !manifests
                .iter()
                .any(|loaded| loaded.digest() == manifest.digest())
            {
                manifests.push(manifest);
            }

            let mut map = MapStorage::new();
            for (name, info) in functions {
                map.insert(MapKey::Str(name), Value::from_native_fn(&mut self.gc, info));
            }
            // NativeFnInfo owns the isolated generation handle. Dropping this
            // temporary owner permits automatic unload when the last
            // callable value disappears instead of retaining every reload
            // for the lifetime of the VM.
            drop(lib);

            Ok(Value::map(&mut self.gc, map))
        }
    }

    /// Convert a one-argument host/native failure into a typed language
    /// `Result`; the worker remains process-isolated and other plugin
    /// generations remain usable after an `Err`.
    fn bi_host_try(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err(format!(
                "host_try() expects a callable and one argument, got {} arguments",
                args.len()
            ));
        }
        let callable = args[0];
        let argument = args[1];
        let result = self.call_value_caught(&callable, vec![argument]);
        Ok(match result {
            Ok(value) => self.make_result(true, value),
            Err(error) => {
                let message = Value::from_string(&mut self.gc, error);
                self.make_result(false, message)
            }
        })
    }

    /// Zero-argument form used for health/determinism probes.
    fn bi_host_try0(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err(format!(
                "host_try0() expects one callable, got {} arguments",
                args.len()
            ));
        }
        let callable = args[0];
        let result = self.call_value_caught(&callable, Vec::new());
        Ok(match result {
            Ok(value) => self.make_result(true, value),
            Err(error) => {
                let message = Value::from_string(&mut self.gc, error);
                self.make_result(false, message)
            }
        })
    }

    // ── Tier 1: Standard I/O ──

    fn bi_eprint(&mut self, args: Vec<Value>) -> Result<Value, String> {
        let s = args
            .iter()
            .map(|v| v.print_display())
            .collect::<Vec<_>>()
            .join(" ");
        self.eprint_buffer.push(s.clone());
        if !self.suppress_output {
            eprintln!("{}", s);
        }
        Ok(Value::NIL)
    }

    fn bi_write_stdout(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("write_stdout() requires exactly 1 argument".into());
        }
        let s = args[0]
            .as_str()
            .ok_or_else(|| format!("write_stdout() expects string, got {}", args[0].type_name()))?;
        self.print_buffer.push(s.to_string());
        if !self.suppress_output {
            use std::io::Write;
            print!("{}", s);
            let _ = std::io::stdout().flush();
        }
        Ok(Value::NIL)
    }

    fn bi_write_stderr(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("write_stderr() requires exactly 1 argument".into());
        }
        let s = args[0]
            .as_str()
            .ok_or_else(|| format!("write_stderr() expects string, got {}", args[0].type_name()))?;
        self.eprint_buffer.push(s.to_string());
        if !self.suppress_output {
            use std::io::Write;
            eprint!("{}", s);
            let _ = std::io::stderr().flush();
        }
        Ok(Value::NIL)
    }

    fn bi_read_stdin_all(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err("read_stdin_all() takes no arguments".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            return Err("read_stdin_all() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                return Ok(self.spawn_io_task(move || {
                    use std::io::Read;
                    let mut buf = String::new();
                    std::io::stdin()
                        .read_to_string(&mut buf)
                        .map_err(|e| format!("read_stdin_all() failed: {}", e))?;
                    Ok(IoTaskPayload::String(buf))
                }));
            }
            use std::io::Read;
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| format!("read_stdin_all() failed: {}", e))?;
            let gc = &mut self.gc;
            Ok(Value::from_string(gc, buf))
        }
    }

    fn bi_sleep_ms(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("sleep_ms() requires 1 argument".into());
        }
        let ms = args[0]
            .as_int()
            .or_else(|| args[0].as_float().map(|f| f as i64))
            .ok_or_else(|| format!("sleep_ms() expects int, got {}", args[0].type_name()))?;
        if ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(ms as u64));
        }
        Ok(Value::NIL)
    }

    fn bi_flush_stdout(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if !args.is_empty() {
            return Err("flush_stdout() takes no arguments".into());
        }
        use std::io::Write;
        std::io::stdout()
            .flush()
            .map_err(|e| format!("flush_stdout() failed: {}", e))?;
        Ok(Value::NIL)
    }

    // ── Tier 2: File I/O ──

    fn bi_append_file(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("append_file() requires exactly 2 arguments".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "append_file() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        let content = args[1].as_str().ok_or_else(|| {
            format!(
                "append_file() expects content string, got {}",
                args[1].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (path, content);
            return Err("append_file() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                let content_owned = content.to_string();
                return Ok(self.spawn_io_task(move || {
                    use std::io::Write;
                    let mut file = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path_owned)
                        .map_err(|e| format!("append_file() failed for '{}': {}", path_owned, e))?;
                    file.write_all(content_owned.as_bytes())
                        .map_err(|e| format!("append_file() failed for '{}': {}", path_owned, e))?;
                    Ok(IoTaskPayload::Nil)
                }));
            }
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| format!("append_file() failed for '{}': {}", path, e))?;
            file.write_all(content.as_bytes())
                .map_err(|e| format!("append_file() failed for '{}': {}", path, e))?;
            Ok(Value::NIL)
        }
    }

    fn bi_file_exists(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("file_exists() requires exactly 1 argument".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "file_exists() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("file_exists() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    let exists = std::path::Path::new(&path_owned).exists();
                    Ok(IoTaskPayload::Int(if exists { 1 } else { 0 }))
                }));
            }
            Ok(Value::from_bool(std::path::Path::new(path).exists()))
        }
    }

    fn bi_remove_file(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("remove_file() requires exactly 1 argument".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "remove_file() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("remove_file() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    fs::remove_file(&path_owned)
                        .map_err(|e| format!("remove_file() failed for '{}': {}", path_owned, e))?;
                    Ok(IoTaskPayload::Nil)
                }));
            }
            fs::remove_file(path)
                .map_err(|e| format!("remove_file() failed for '{}': {}", path, e))?;
            Ok(Value::NIL)
        }
    }

    fn bi_list_dir(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("list_dir() requires exactly 1 argument".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "list_dir() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("list_dir() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    let mut names = Vec::new();
                    let entries = fs::read_dir(&path_owned)
                        .map_err(|e| format!("list_dir() failed for '{}': {}", path_owned, e))?;
                    for entry in entries {
                        let entry = entry.map_err(|e| format!("list_dir() entry error: {}", e))?;
                        names.push(entry.file_name().to_string_lossy().into_owned());
                    }
                    Ok(IoTaskPayload::StringList(names))
                }));
            }
            let mut result = Vec::new();
            let entries = fs::read_dir(path)
                .map_err(|e| format!("list_dir() failed for '{}': {}", path, e))?;
            let gc = &mut self.gc;
            for entry in entries {
                let entry = entry.map_err(|e| format!("list_dir() entry error: {}", e))?;
                result.push(Value::from_string(
                    gc,
                    entry.file_name().to_string_lossy().into_owned(),
                ));
            }
            Ok(Value::list(gc, result))
        }
    }

    fn bi_create_dir(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("create_dir() requires exactly 1 argument".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "create_dir() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("create_dir() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    fs::create_dir_all(&path_owned)
                        .map_err(|e| format!("create_dir() failed for '{}': {}", path_owned, e))?;
                    Ok(IoTaskPayload::Nil)
                }));
            }
            fs::create_dir_all(path)
                .map_err(|e| format!("create_dir() failed for '{}': {}", path, e))?;
            Ok(Value::NIL)
        }
    }

    fn bi_remove_dir(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("remove_dir() requires exactly 1 argument".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "remove_dir() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("remove_dir() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    fs::remove_dir_all(&path_owned)
                        .map_err(|e| format!("remove_dir() failed for '{}': {}", path_owned, e))?;
                    Ok(IoTaskPayload::Nil)
                }));
            }
            fs::remove_dir_all(path)
                .map_err(|e| format!("remove_dir() failed for '{}': {}", path, e))?;
            Ok(Value::NIL)
        }
    }

    fn bi_read_file_bytes(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 1 {
            return Err("read_file_bytes() requires exactly 1 argument".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "read_file_bytes() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err("read_file_bytes() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    let bytes = fs::read(&path_owned).map_err(|e| {
                        format!("read_file_bytes() failed for '{}': {}", path_owned, e)
                    })?;
                    Ok(IoTaskPayload::Bytes(bytes))
                }));
            }
            let bytes = fs::read(path)
                .map_err(|e| format!("read_file_bytes() failed for '{}': {}", path, e))?;
            let gc = &mut self.gc;
            let values: Vec<Value> = bytes
                .into_iter()
                .map(|b| Value::from_int(gc, b as i64))
                .collect();
            Ok(Value::list(gc, values))
        }
    }

    fn bi_write_file_bytes(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("write_file_bytes() requires exactly 2 arguments".into());
        }
        let path = args[0].as_str().ok_or_else(|| {
            format!(
                "write_file_bytes() expects path string, got {}",
                args[0].type_name()
            )
        })?;
        let list = args[1].as_list().ok_or_else(|| {
            format!(
                "write_file_bytes() expects list of ints, got {}",
                args[1].type_name()
            )
        })?;
        let mut bytes = Vec::with_capacity(list.len());
        for (i, v) in list.iter().enumerate() {
            let n = v
                .as_int()
                .ok_or_else(|| format!("write_file_bytes() list element {} is not an int", i))?;
            if !(0..=255).contains(&n) {
                return Err(format!(
                    "write_file_bytes() byte value {} out of range 0..255 at index {}",
                    n, i
                ));
            }
            bytes.push(n as u8);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (path, bytes);
            return Err("write_file_bytes() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let path_owned = path.to_string();
                return Ok(self.spawn_io_task(move || {
                    fs::write(&path_owned, &bytes).map_err(|e| {
                        format!("write_file_bytes() failed for '{}': {}", path_owned, e)
                    })?;
                    Ok(IoTaskPayload::Nil)
                }));
            }
            fs::write(path, &bytes)
                .map_err(|e| format!("write_file_bytes() failed for '{}': {}", path, e))?;
            Ok(Value::NIL)
        }
    }

    fn bi_http_post(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("http_post() requires exactly 2 arguments".into());
        }
        let url = args[0].as_str().ok_or_else(|| {
            format!(
                "http_post() expects url string, got {}",
                args[0].type_name()
            )
        })?;
        let body = args[1].as_str().ok_or_else(|| {
            format!(
                "http_post() expects body string, got {}",
                args[1].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (url, body);
            return Err("http_post() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let url_owned = url.to_string();
                let body_owned = body.to_string();
                return Ok(self.spawn_io_task(move || {
                    let response =
                        ureq::post(&url_owned)
                            .send(body_owned.as_bytes())
                            .map_err(|e| {
                                format!("http_post() request failed for '{}': {}", url_owned, e)
                            })?;
                    let text = response
                        .into_body()
                        .read_to_string()
                        .map_err(|e| format!("http_post() failed reading response body: {}", e))?;
                    Ok(IoTaskPayload::String(text))
                }));
            }
            let response = ureq::post(url)
                .send(body.as_bytes())
                .map_err(|e| format!("http_post() request failed for '{}': {}", url, e))?;
            let text = response
                .into_body()
                .read_to_string()
                .map_err(|e| format!("http_post() failed reading response body: {}", e))?;
            let gc = &mut self.gc;
            Ok(Value::from_string(gc, text))
        }
    }

    fn bi_http_post_json(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("http_post_json() requires exactly 2 arguments".into());
        }
        let url = args[0].as_str().ok_or_else(|| {
            format!(
                "http_post_json() expects url string, got {}",
                args[0].type_name()
            )
        })?;
        let body = args[1].as_str().ok_or_else(|| {
            format!(
                "http_post_json() expects body string, got {}",
                args[1].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (url, body);
            return Err("http_post_json() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.in_async_context {
                let url_owned = url.to_string();
                let body_owned = body.to_string();
                return Ok(self.spawn_io_task(move || {
                    let response = ureq::post(&url_owned)
                        .content_type("application/json")
                        .send(body_owned.as_bytes())
                        .map_err(|e| {
                            format!("http_post_json() request failed for '{}': {}", url_owned, e)
                        })?;
                    let mut resp_body = response.into_body();
                    let text = resp_body.read_to_string().map_err(|e| {
                        format!("http_post_json() failed reading response body: {}", e)
                    })?;
                    Ok(IoTaskPayload::String(text))
                }));
            }
            let response = ureq::post(url)
                .content_type("application/json")
                .send(body.as_bytes())
                .map_err(|e| format!("http_post_json() request failed for '{}': {}", url, e))?;
            let mut resp_body = response.into_body();
            let text = resp_body
                .read_to_string()
                .map_err(|e| format!("http_post_json() failed reading response body: {}", e))?;
            Ok(Value::from_string(&mut self.gc, text))
        }
    }

    fn bi_http_request(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 4 {
            return Err(
                "http_request() requires exactly 4 arguments: method, url, headers, body".into(),
            );
        }
        let method = args[0]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "http_request() expects method string, got {}",
                    args[0].type_name()
                )
            })?
            .to_string();
        let url = args[1]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "http_request() expects url string, got {}",
                    args[1].type_name()
                )
            })?
            .to_string();
        let headers_map = args[2].as_map().ok_or_else(|| {
            format!(
                "http_request() expects headers map, got {}",
                args[2].type_name()
            )
        })?;
        let mut headers: Vec<(String, String)> = Vec::new();
        for (k, v) in headers_map.iter() {
            let key = match k {
                MapKey::Str(s) => s.clone(),
                other => {
                    return Err(format!(
                        "http_request() header key must be string, got {}",
                        other
                    ))
                }
            };
            let val = v
                .as_str()
                .ok_or_else(|| {
                    format!(
                        "http_request() header value must be string, got {}",
                        v.type_name()
                    )
                })?
                .to_string();
            headers.push((key, val));
        }
        let body = args[3]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "http_request() expects body string, got {}",
                    args[3].type_name()
                )
            })?
            .to_string();
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (method, url, headers, body);
            return Err("http_request() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            type HttpResponse = Result<(u16, String, Vec<(String, String)>), String>;
            let do_request = move || -> HttpResponse {
                let method_upper = method.to_uppercase();
                let send_with_headers =
                    |mut req: ureq::RequestBuilder<ureq::typestate::WithBody>,
                     hdrs: &[(String, String)],
                     b: &str|
                     -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
                        for (k, v) in hdrs {
                            req = req.header(k.as_str(), v.as_str());
                        }
                        if b.is_empty() {
                            req.send_empty()
                        } else {
                            req.send(b.as_bytes())
                        }
                    };
                let call_no_body =
                    |mut req: ureq::RequestBuilder<ureq::typestate::WithoutBody>,
                     hdrs: &[(String, String)]|
                     -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
                        for (k, v) in hdrs {
                            req = req.header(k.as_str(), v.as_str());
                        }
                        req.call()
                    };
                let response = match method_upper.as_str() {
                    "GET" => call_no_body(ureq::get(&url), &headers),
                    "HEAD" => call_no_body(ureq::head(&url), &headers),
                    "DELETE" => call_no_body(ureq::delete(&url), &headers).or_else(|_| {
                        send_with_headers(ureq::delete(&url).force_send_body(), &headers, &body)
                    }),
                    "POST" => send_with_headers(ureq::post(&url), &headers, &body),
                    "PUT" => send_with_headers(ureq::put(&url), &headers, &body),
                    "PATCH" => send_with_headers(ureq::patch(&url), &headers, &body),
                    other => return Err(format!("http_request() unsupported method: {}", other)),
                };
                let response = response.map_err(|e| format!("http_request() failed: {}", e))?;
                let status = response.status().as_u16();
                let mut resp_headers: Vec<(String, String)> = Vec::new();
                for (name, val) in response.headers().iter() {
                    let n: &ureq::http::HeaderName = name;
                    let v: &ureq::http::HeaderValue = val;
                    resp_headers.push((n.to_string(), v.to_str().unwrap_or("").to_string()));
                }
                let text = response
                    .into_body()
                    .read_to_string()
                    .map_err(|e| format!("http_request() failed reading body: {}", e))?;
                Ok((status, text, resp_headers))
            };
            if self.in_async_context {
                return Ok(self.spawn_io_task(move || {
                    let (status, text, resp_headers) = do_request()?;
                    let mut header_pairs: Vec<(String, IoTaskPayload)> = Vec::new();
                    for (k, v) in resp_headers {
                        header_pairs.push((k, IoTaskPayload::String(v)));
                    }
                    Ok(IoTaskPayload::ValueMap(vec![
                        ("status".to_string(), IoTaskPayload::Int(status as i64)),
                        ("body".to_string(), IoTaskPayload::String(text)),
                        ("headers".to_string(), IoTaskPayload::ValueMap(header_pairs)),
                    ]))
                }));
            }
            let (status, text, resp_headers) = do_request()?;
            let mut result_map = MapStorage::new();
            result_map.insert(
                MapKey::Str("status".to_string()),
                Value::from_int(&mut self.gc, status as i64),
            );
            result_map.insert(
                MapKey::Str("body".to_string()),
                Value::from_string(&mut self.gc, text),
            );
            let mut hdr_map = MapStorage::new();
            for (k, v) in resp_headers {
                hdr_map.insert(MapKey::Str(k), Value::from_string(&mut self.gc, v));
            }
            result_map.insert(
                MapKey::Str("headers".to_string()),
                Value::map(&mut self.gc, hdr_map),
            );
            Ok(Value::map(&mut self.gc, result_map))
        }
    }

    // ── Tier 4: TCP Networking ──

    fn bi_tcp_connect(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("tcp_connect() requires exactly 2 arguments: host, port".into());
        }
        let host = args[0]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "tcp_connect() expects host string, got {}",
                    args[0].type_name()
                )
            })?
            .to_string();
        let port = args[1].as_int().ok_or_else(|| {
            format!(
                "tcp_connect() expects port int, got {}",
                args[1].type_name()
            )
        })?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (host, port);
            return Err("tcp_connect() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let addr = format!("{}:{}", host, port);
            let stream = std::net::TcpStream::connect(&addr)
                .map_err(|e| format!("tcp_connect() failed for '{}': {}", addr, e))?;
            let handle_id = self.next_net_handle_id;
            self.next_net_handle_id += 1;
            self.net_handles
                .insert(handle_id, super::NetHandle::TcpStream(stream));
            let gc = &mut self.gc;
            Ok(Value::from_int(gc, handle_id as i64))
        }
    }

    fn bi_tcp_listen(&mut self, args: Vec<Value>) -> Result<Value, String> {
        if args.len() != 2 {
            return Err("tcp_listen() requires exactly 2 arguments: host, port".into());
        }
        let host = args[0]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "tcp_listen() expects host string, got {}",
                    args[0].type_name()
                )
            })?
            .to_string();
        let port = args[1]
            .as_int()
            .ok_or_else(|| format!("tcp_listen() expects port int, got {}", args[1].type_name()))?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = (host, port);
            return Err("tcp_listen() is not supported in wasm runtime".to_string());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let addr = format!("{}:{}", host, port);
            let listener = std::net::TcpListener::bind(&addr)
                .map_err(|e| format!("tcp_listen() failed for '{}': {}", addr, e))?;
            let handle_id = self.next_net_handle_id;
            self.next_net_handle_id += 1;
            self.net_handles
                .insert(handle_id, super::NetHandle::TcpListener(listener));
            let gc = &mut self.gc;
            Ok(Value::from_int(gc, handle_id as i64))
        }
    }}

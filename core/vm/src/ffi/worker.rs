// Crash- and timeout-contained native execution. The RAD process owns the
// world; one worker process owns one sealed plugin generation. Calls are
// serialized over a loopback stream so a native fault cannot corrupt VM
// memory, and retained function values keep their exact worker generation.

#[cfg(not(target_arch = "wasm32"))]
const NATIVE_WORKER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
#[cfg(not(target_arch = "wasm32"))]
const MAX_WORKER_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

#[cfg(not(target_arch = "wasm32"))]
#[derive(serde::Serialize, serde::Deserialize)]
enum NativeWorkerRequest {
    Hello { token: String },
    Invoke {
        export: String,
        args: Vec<serde_json::Value>,
    },
    Shutdown,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(serde::Serialize, serde::Deserialize)]
enum NativeWorkerResponse {
    Ready { plugin: serde_json::Value },
    Call {
        result: Result<serde_json::Value, String>,
    },
    Failure { message: String },
}

#[cfg(not(target_arch = "wasm32"))]
struct NativeWorkerState {
    child: std::process::Child,
    stream: std::net::TcpStream,
    terminal_failure: Option<String>,
}

/// Process-owned execution resource for one content-addressed plugin image.
#[cfg(not(target_arch = "wasm32"))]
pub struct NativeWorkerHandle {
    plugin: String,
    state: std::sync::Mutex<NativeWorkerState>,
}

#[cfg(not(target_arch = "wasm32"))]
impl NativeWorkerHandle {
    fn invoke(
        &self,
        export: &str,
        args: &[Value],
        target: &mut GcHeap,
    ) -> Result<Value, String> {
        let encoded_args = args
            .iter()
            .map(crate::replay::encode_value)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("cannot encode native arguments: {error}"))?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| "native worker lock was poisoned".to_string())?;
        if let Some(failure) = &state.terminal_failure {
            return Err(failure.clone());
        }
        if let Err(error) = write_worker_message(
            &mut state.stream,
            &NativeWorkerRequest::Invoke {
                export: export.to_string(),
                args: encoded_args,
            },
        ) {
            return Err(mark_worker_failed(&mut state, &self.plugin, error));
        }
        let response = match read_worker_message::<NativeWorkerResponse>(&mut state.stream) {
            Ok(response) => response,
            Err(error) => return Err(mark_worker_failed(&mut state, &self.plugin, error)),
        };
        match response {
            NativeWorkerResponse::Call { result: Ok(value) } => {
                crate::replay::decode_value(target, &value)
            }
            NativeWorkerResponse::Call { result: Err(error) } => Err(format!(
                "HostCallFailure: plugin '{}' export {}(): {error}",
                self.plugin, export
            )),
            NativeWorkerResponse::Failure { message } => Err(format!(
                "HostCallFailure: plugin '{}' export {}(): {message}",
                self.plugin, export
            )),
            NativeWorkerResponse::Ready { .. } => Err(mark_worker_failed(
                &mut state,
                &self.plugin,
                "worker returned an unexpected ready response".to_string(),
            )),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for NativeWorkerHandle {
    fn drop(&mut self) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.terminal_failure.is_none() {
            let _ = write_worker_message(&mut state.stream, &NativeWorkerRequest::Shutdown);
        }
        let _ = state.child.kill();
        let _ = state.child.wait();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn mark_worker_failed(
    state: &mut NativeWorkerState,
    plugin: &str,
    detail: String,
) -> String {
    let status = state.child.try_wait().ok().flatten();
    let _ = state.child.kill();
    let _ = state.child.wait();
    let failure = if detail.contains("timed out") || detail.contains("would block") {
        format!(
            "HostCallFailure: plugin '{plugin}' exceeded the {} ms native-call timeout",
            NATIVE_WORKER_TIMEOUT.as_millis()
        )
    } else if let Some(status) = status {
        format!("HostCallFailure: plugin '{plugin}' worker exited with {status}: {detail}")
    } else {
        format!("HostCallFailure: plugin '{plugin}' worker failed: {detail}")
    };
    state.terminal_failure = Some(failure.clone());
    failure
}

#[cfg(not(target_arch = "wasm32"))]
fn write_worker_message<T: serde::Serialize>(
    stream: &mut std::net::TcpStream,
    message: &T,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(message)
        .map_err(|error| format!("native worker message is not serializable: {error}"))?;
    if bytes.len() > MAX_WORKER_MESSAGE_BYTES {
        return Err(format!(
            "native worker message exceeds {} bytes",
            MAX_WORKER_MESSAGE_BYTES
        ));
    }
    let length = u32::try_from(bytes.len())
        .map_err(|_| "native worker message length overflow".to_string())?;
    std::io::Write::write_all(stream, &length.to_le_bytes())
        .and_then(|_| std::io::Write::write_all(stream, &bytes))
        .and_then(|_| std::io::Write::flush(stream))
        .map_err(|error| worker_io_error("write", error))
}

#[cfg(not(target_arch = "wasm32"))]
fn read_worker_message<T: serde::de::DeserializeOwned>(
    stream: &mut std::net::TcpStream,
) -> Result<T, String> {
    let mut length = [0u8; 4];
    std::io::Read::read_exact(stream, &mut length)
        .map_err(|error| worker_io_error("read", error))?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_WORKER_MESSAGE_BYTES {
        return Err(format!(
            "native worker response declares {length} bytes; limit is {}",
            MAX_WORKER_MESSAGE_BYTES
        ));
    }
    let mut bytes = vec![0; length];
    std::io::Read::read_exact(stream, &mut bytes)
        .map_err(|error| worker_io_error("read", error))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("native worker response is malformed: {error}"))
}

#[cfg(not(target_arch = "wasm32"))]
fn worker_io_error(operation: &str, error: std::io::Error) -> String {
    if matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        format!("native worker {operation} timed out")
    } else {
        format!("native worker {operation} failed: {error}")
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn worker_executable() -> Result<PathBuf, String> {
    if let Some(configured) = std::env::var_os("RAD_FFI_WORKER_EXECUTABLE") {
        let configured = PathBuf::from(configured);
        if configured.is_file() {
            return Ok(configured);
        }
        return Err(format!(
            "RAD_FFI_WORKER_EXECUTABLE does not name a file: {}",
            configured.display()
        ));
    }
    let current = std::env::current_exe()
        .map_err(|error| format!("cannot locate the RAD executable: {error}"))?;
    let worker = current.with_file_name(format!(
        "rad-ffi-worker{}",
        std::env::consts::EXE_SUFFIX
    ));
    if !worker.is_file() {
        return Err(format!(
            "isolated native worker is missing: expected '{}'; install rad-ffi-worker beside rad or set RAD_FFI_WORKER_EXECUTABLE",
            worker.display()
        ));
    }
    Ok(worker)
}

#[cfg(not(target_arch = "wasm32"))]
fn worker_token(path: &str) -> String {
    static NONCE: AtomicU64 = AtomicU64::new(1);
    let mut digest = Sha256::new();
    digest.update(b"rad-native-worker-token/v1");
    digest.update(std::process::id().to_le_bytes());
    digest.update(NONCE.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    digest.update(path.as_bytes());
    digest.update(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes(),
    );
    hex::encode(digest.finalize())
}

/// Start one isolated worker and bind every export to that exact process and
/// content-addressed generation.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_plugin_isolated(path: &str) -> Result<LoadedPlugin<LoadedNativeLibrary>, String> {
    let executable = worker_executable()?;
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .map_err(|error| format!("cannot bind native-worker channel: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("cannot configure native-worker channel: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("cannot inspect native-worker channel: {error}"))?;
    let token = worker_token(path);
    let mut command = std::process::Command::new(&executable);
    command
        .args(["--connect", &address.to_string(), "--plugin", path])
        .env("RAD_FFI_WORKER_TOKEN", &token)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|error| {
        format!(
            "cannot start isolated native worker '{}': {error}",
            executable.display()
        )
    })?;
    let started = std::time::Instant::now();
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|error| format!("cannot inspect native worker: {error}"))?
                {
                    return Err(format!(
                        "native worker exited before handshake with {status}"
                    ));
                }
                if started.elapsed() >= NATIVE_WORKER_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "native worker exceeded the {} ms startup timeout",
                        NATIVE_WORKER_TIMEOUT.as_millis()
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("native-worker accept failed: {error}"));
            }
        }
    };
    stream
        .set_nonblocking(false)
        .map_err(|error| format!("cannot make native-worker channel blocking: {error}"))?;
    stream
        .set_read_timeout(Some(NATIVE_WORKER_TIMEOUT))
        .and_then(|_| stream.set_write_timeout(Some(NATIVE_WORKER_TIMEOUT)))
        .map_err(|error| format!("cannot configure native-worker timeout: {error}"))?;
    write_worker_message(
        &mut stream,
        &NativeWorkerRequest::Hello {
            token: token.clone(),
        },
    )?;
    let response = read_worker_message::<NativeWorkerResponse>(&mut stream).map_err(|error| {
        let _ = child.kill();
        let _ = child.wait();
        if error.contains("timed out") || error.contains("would block") {
            format!(
                "HostCallFailure: plugin '{path}' exceeded the {} ms load timeout",
                NATIVE_WORKER_TIMEOUT.as_millis()
            )
        } else {
            format!("HostCallFailure: plugin '{path}' failed during isolated load: {error}")
        }
    })?;
    let encoded = match response {
        NativeWorkerResponse::Ready { plugin } => plugin,
        NativeWorkerResponse::Failure { message } => return Err(message),
        NativeWorkerResponse::Call { .. } => {
            return Err("native worker returned a call result during handshake".to_string())
        }
    };
    let handle = std::sync::Arc::new(NativeWorkerHandle {
        plugin: path.to_string(),
        state: std::sync::Mutex::new(NativeWorkerState {
            child,
            stream,
            terminal_failure: None,
        }),
    });
    let (mut functions, manifest) = decode_recorded_plugin(encoded)?;
    for (_, function) in &mut functions {
        function.execution = NativeExecution::Isolated(std::sync::Arc::clone(&handle));
    }
    Ok((
        functions,
        LoadedNativeLibrary::Isolated(handle),
        manifest,
    ))
}

/// Process entry used by the dedicated `rad-ffi-worker` adapter.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_plugin_worker(connect: &str, plugin: &str, token: &str) -> Result<(), String> {
    let mut stream = std::net::TcpStream::connect(connect)
        .map_err(|error| format!("native worker cannot connect to owner: {error}"))?;
    stream
        // An idle generation is healthy. Only the owning VM times an active
        // request; the worker waits indefinitely for its next command.
        .set_read_timeout(None)
        .and_then(|_| stream.set_write_timeout(Some(NATIVE_WORKER_TIMEOUT)))
        .map_err(|error| format!("native worker cannot configure channel: {error}"))?;
    match read_worker_message::<NativeWorkerRequest>(&mut stream)? {
        NativeWorkerRequest::Hello { token: supplied } if supplied == token => {}
        NativeWorkerRequest::Hello { .. } => {
            return Err("native worker handshake token mismatch".to_string())
        }
        _ => return Err("native worker expected a handshake".to_string()),
    }
    let mut gc = GcHeap::new();
    let (functions, _library, manifest) = match load_plugin(plugin, &mut gc) {
        Ok(loaded) => loaded,
        Err(error) => {
            let _ = write_worker_message(
                &mut stream,
                &NativeWorkerResponse::Failure {
                    message: format!("HostCallFailure: plugin '{plugin}' failed to load: {error}"),
                },
            );
            return Err(error);
        }
    };
    let encoded = encode_recorded_plugin(&manifest)?;
    write_worker_message(
        &mut stream,
        &NativeWorkerResponse::Ready { plugin: encoded },
    )?;
    let functions = functions.into_iter().collect::<std::collections::HashMap<_, _>>();
    loop {
        let request = match read_worker_message::<NativeWorkerRequest>(&mut stream) {
            Ok(request) => request,
            Err(error) if error.contains("failed to fill whole buffer") => return Ok(()),
            Err(error) => return Err(error),
        };
        match request {
            NativeWorkerRequest::Invoke { export, args } => {
                let result = if let Some(function) = functions.get(&export) {
                    let decoded = args
                        .iter()
                        .map(|value| crate::replay::decode_value(&mut gc, value))
                        .collect::<Result<Vec<_>, _>>();
                    match decoded {
                        Ok(args) => invoke_native(function, &args, &mut gc)
                            .and_then(|value| crate::replay::encode_value(&value)),
                        Err(error) => Err(format!("cannot decode native arguments: {error}")),
                    }
                } else {
                    Err(format!("plugin does not export '{export}'"))
                };
                write_worker_message(&mut stream, &NativeWorkerResponse::Call { result })?;
                // All call values have been reduced to pointer-free JSON. The
                // worker retains no guest roots between requests.
                unsafe {
                    gc.sweep(&std::collections::HashSet::new());
                }
            }
            NativeWorkerRequest::Shutdown => return Ok(()),
            NativeWorkerRequest::Hello { .. } => {
                write_worker_message(
                    &mut stream,
                    &NativeWorkerResponse::Failure {
                        message: "duplicate native worker handshake".to_string(),
                    },
                )?;
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub fn load_plugin_isolated(_path: &str) -> Result<LoadedPlugin<()>, String> {
    Err("native extensions are not supported on wasm32".to_string())
}

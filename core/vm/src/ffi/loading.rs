// Loading a plugin from disk and sealing its image: the only place a native
// library becomes callable, and the digest that pins which bytes did so.
#[cfg(not(target_arch = "wasm32"))]
pub fn load_plugin(
    path: &str,
    merge_into: &mut GcHeap,
) -> Result<LoadedPlugin<LoadedNativeLibrary>, String> {
    unsafe {
        let requested = Path::new(path);
        let platform_path = if requested.extension().is_none() {
            let candidate = requested.with_extension(std::env::consts::DLL_EXTENSION);
            candidate.exists().then_some(candidate)
        } else {
            None
        };
        let resolved: PathBuf = platform_path.unwrap_or_else(|| requested.to_path_buf());
        let sealed = seal_native_library(&resolved)?;
        let loader_path = sealed.loader_path();
        let lib = libloading::Library::new(&loader_path).map_err(|error| {
            format!(
                "Failed to load sealed plugin '{}': {}",
                loader_path.display(),
                error
            )
        })?;

        type DescribeFn = unsafe extern "C" fn() -> *const RadExtensionDescriptor;
        let describe_fn: libloading::Symbol<DescribeFn> = lib
            .get(b"rad_extension_descriptor\0")
            .map_err(|e| format!("Failed to find rad_extension_descriptor: {e}"))?;
        let descriptor = describe_fn();
        if descriptor.is_null() {
            return Err("rad_extension_descriptor returned null".to_string());
        }
        let descriptor = &*descriptor;
        if descriptor.abi_version != RAD_EXTENSION_ABI_VERSION {
            return Err(format!(
                "native extension ABI mismatch: library declares {}, runtime requires {}",
                descriptor.abi_version, RAD_EXTENSION_ABI_VERSION
            ));
        }
        if descriptor.extension_id.is_null()
            || descriptor.extension_version.is_null()
            || descriptor.abi_contract_json.is_null()
        {
            return Err(
                "native extension descriptor requires id, version, and ABI contract".to_string(),
            );
        }
        let extension_id = CStr::from_ptr(descriptor.extension_id)
            .to_str()
            .map_err(|_| "native extension id is not UTF-8".to_string())?
            .to_string();
        let extension_version = CStr::from_ptr(descriptor.extension_version)
            .to_str()
            .map_err(|_| "native extension version is not UTF-8".to_string())?
            .to_string();
        if extension_id.is_empty() || extension_version.is_empty() {
            return Err("native extension descriptor id/version cannot be empty".to_string());
        }
        let abi_contract = NativeAbiContract::parse(
            CStr::from_ptr(descriptor.abi_contract_json)
                .to_str()
                .map_err(|_| "native ABI contract is not UTF-8".to_string())?,
        )?;

        type InitFn = unsafe extern "C" fn(*const RadPluginApi);
        let init_fn: libloading::Symbol<InitFn> = lib
            .get(b"rad_extension_init\0")
            .map_err(|e| format!("Failed to find rad_extension_init: {}", e))?;

        let mut context = RegistrationContext {
            functions: Vec::new(),
            errors: Vec::new(),
        };

        let api = RadPluginApi {
            ctx: &mut context as *mut RegistrationContext as *mut c_void,
            register_fn: api_register_fn,
            make_nil: api_make_nil,
            make_int: api_make_int,
            make_float: api_make_float,
            make_bool: api_make_bool,
            make_string: api_make_string,
            make_host_handle: api_make_host_handle,
            as_int: api_as_int,
            as_float: api_as_float,
            as_bool: api_as_bool,
            as_string_ptr: api_as_string_ptr,
            as_string_len: api_as_string_len,
            as_host_handle: api_as_host_handle,
            set_error: api_set_error,
        };

        init_fn(&api);
        if !context.errors.is_empty() {
            return Err(format!(
                "native extension registration failed: {}",
                context.errors.join("; ")
            ));
        }
        if context.functions.is_empty() {
            return Err("native extension registered no functions".to_string());
        }

        drain_native_values_into(merge_into);
        let exports = context
            .functions
            .iter()
            .map(
                |(name, _, arity, effects, signature, deterministic, replayable)| {
                    NativeExportManifest {
                name: name.clone(),
                arity: *arity,
                effects: effects.clone(),
                        signature: signature.clone(),
                        deterministic: *deterministic,
                        replayable: *replayable,
                    }
                },
            )
            .collect::<Vec<_>>();
        let manifest = std::sync::Arc::new(NativeExtensionManifest::from_binary(
            &resolved,
            &sealed.bytes,
            extension_id,
            extension_version,
            &exports,
            abi_contract,
        ));
        let functions = context
            .functions
            .into_iter()
            .map(
                |(name, func, arity, effects, signature, deterministic, replayable)| {
                let info = NativeFnInfo {
                    name: name.clone(),
                    execution: NativeExecution::InProcess(func),
                    arity,
                    effects,
                        signature,
                        deterministic,
                        replayable,
                    extension: std::sync::Arc::clone(&manifest),
                };
                (name, info)
                },
            )
            .collect();

        Ok((
            functions,
            LoadedNativeLibrary::InProcess {
                _library: lib,
                _sealed_file: sealed.file,
                _sealed_path: sealed.path,
            },
            manifest,
        ))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn seal_native_library(source: &Path) -> Result<SealedNativeImage, String> {
    let bytes = std::fs::read(source)
        .map_err(|error| format!("Failed to read plugin '{}': {}", source.display(), error))?;
    let digest = hex::encode(Sha256::digest(&bytes));
    let cache = std::env::temp_dir()
        .join("rad-native-extension-cache")
        .join("v1");
    std::fs::create_dir_all(&cache).map_err(|error| {
        format!(
            "Failed to create native extension cache '{}': {}",
            cache.display(),
            error
        )
    })?;
    let file_name = match source.extension().and_then(|extension| extension.to_str()) {
        Some(extension) => format!("{digest}.{extension}"),
        None => digest.clone(),
    };
    let sealed_path = cache.join(file_name);

    if !sealed_path.exists() {
        let nonce = SEALED_IMAGE_NONCE.fetch_add(1, Ordering::Relaxed);
        let temporary = cache.join(format!(".{digest}.{}.{}.tmp", std::process::id(), nonce));
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| {
                format!(
                    "Failed to create sealed plugin image '{}': {}",
                    temporary.display(),
                    error
                )
            })?;
        output.write_all(&bytes).map_err(|error| {
            format!(
                "Failed to write sealed plugin image '{}': {}",
                temporary.display(),
                error
            )
        })?;
        output.sync_all().map_err(|error| {
            format!(
                "Failed to sync sealed plugin image '{}': {}",
                temporary.display(),
                error
            )
        })?;
        drop(output);
        if let Err(error) = std::fs::rename(&temporary, &sealed_path) {
            if !sealed_path.exists() {
                return Err(format!(
                    "Failed to publish sealed plugin image '{}': {}",
                    sealed_path.display(),
                    error
                ));
            }
            let _ = std::fs::remove_file(&temporary);
        }
    }

    let sealed_bytes = std::fs::read(&sealed_path).map_err(|error| {
        format!(
            "Failed to verify sealed plugin image '{}': {}",
            sealed_path.display(),
            error
        )
    })?;
    if sealed_bytes != bytes {
        return Err(format!(
            "Sealed plugin image '{}' does not match content digest {}",
            sealed_path.display(),
            digest
        ));
    }
    let mut permissions = std::fs::metadata(&sealed_path)
        .map_err(|error| error.to_string())?
        .permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&sealed_path, permissions).map_err(|error| {
        format!(
            "Failed to make sealed plugin image '{}' read-only: {}",
            sealed_path.display(),
            error
        )
    })?;

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Permit the dynamic loader to read the image, but deny replacement
        // and deletion while this VM retains the library handle.
        options.share_mode(0x0000_0001);
    }
    let file = options.open(&sealed_path).map_err(|error| {
        format!(
            "Failed to retain sealed plugin image '{}': {}",
            sealed_path.display(),
            error
        )
    })?;
    Ok(SealedNativeImage {
        path: sealed_path,
        file,
        bytes,
    })
}

#[cfg(target_arch = "wasm32")]
pub fn load_plugin(_path: &str, _merge_into: &mut GcHeap) -> Result<LoadedPlugin<()>, String> {
    Err("Plugins are not supported on wasm32".to_string())
}

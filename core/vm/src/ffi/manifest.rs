// The stable description of a native extension: handle tokens, sealed image
// identity, declared effect sets, and the export/extension manifests those
// declarations are checked against.
#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static FFI_GC: RefCell<GcHeap> = const { RefCell::new(GcHeap::new()) };
    static NATIVE_CALL_OWNER: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Unforgeable outside `rad_vm`: embedders may clone and pass a token back,
/// but only the extension that minted it can recover its numeric payload.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HostHandleToken {
    owner_digest: String,
    type_name: String,
    token: u64,
}

impl HostHandleToken {
    pub(crate) fn from_recorded(owner_digest: String, type_name: String, token: u64) -> Self {
        Self {
            owner_digest,
            type_name,
            token,
        }
    }

    pub fn owner_digest(&self) -> &str {
        &self.owner_digest
    }

    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    pub(crate) fn token_for_runtime(&self) -> u64 {
        self.token
    }
}

pub type NativeFnPtr = unsafe extern "C" fn(args: *const u64, argc: usize) -> u64;
pub type LoadedPlugin<L> = (
    Vec<(String, NativeFnInfo)>,
    L,
    std::sync::Arc<NativeExtensionManifest>,
);

/// How one native export is executed. Ordinary RAD programs receive only
/// isolated workers; the in-process variant exists solely inside the worker
/// and read-only verifier process. Replay stubs never touch a plugin image.
#[derive(Clone)]
pub enum NativeExecution {
    #[cfg(not(target_arch = "wasm32"))]
    InProcess(NativeFnPtr),
    #[cfg(not(target_arch = "wasm32"))]
    Isolated(std::sync::Arc<NativeWorkerHandle>),
    Replay,
}

#[cfg(not(target_arch = "wasm32"))]
pub enum LoadedNativeLibrary {
    InProcess {
        // Drop the loader handle before releasing the open sealed-image handle.
        _library: libloading::Library,
        _sealed_file: File,
        _sealed_path: PathBuf,
    },
    Isolated(std::sync::Arc<NativeWorkerHandle>),
}

#[cfg(not(target_arch = "wasm32"))]
struct SealedNativeImage {
    path: PathBuf,
    file: File,
    bytes: Vec<u8>,
}

#[cfg(not(target_arch = "wasm32"))]
impl SealedNativeImage {
    #[cfg(target_os = "linux")]
    fn loader_path(&self) -> PathBuf {
        use std::os::fd::AsRawFd;
        // Linux's loader follows this descriptor to the already-open inode. A
        // later rename or replacement of the cache path cannot change which
        // bytes are mapped.
        PathBuf::from(format!("/proc/self/fd/{}", self.file.as_raw_fd()))
    }

    #[cfg(not(target_os = "linux"))]
    fn loader_path(&self) -> PathBuf {
        self.path.clone()
    }
}

#[cfg(not(target_arch = "wasm32"))]
static SEALED_IMAGE_NONCE: AtomicU64 = AtomicU64::new(0);

pub const RAD_EXTENSION_ABI_VERSION: u32 = 3;
pub const NATIVE_RESOURCE_CONTRACT_VERSION: u32 = 2;

/// Machine-checkable C layout published by one extension. The verifier
/// rejects malformed, overlapping, or out-of-bounds layouts before any
/// function becomes callable.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct NativeAbiContract {
    pub contract_version: u32,
    pub calling_convention: String,
    pub types: Vec<NativeTypeLayout>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct NativeTypeLayout {
    pub name: String,
    pub size: u32,
    pub alignment: u32,
    pub fields: Vec<NativeFieldLayout>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
pub struct NativeFieldLayout {
    pub name: String,
    pub type_name: String,
    pub offset: u32,
    pub size: u32,
}

impl NativeAbiContract {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn parse(encoded: &str) -> Result<Self, String> {
        let contract: Self = serde_json::from_str(encoded)
            .map_err(|error| format!("native ABI contract is invalid JSON: {error}"))?;
        contract.validate()?;
        Ok(contract)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn validate(&self) -> Result<(), String> {
        if self.contract_version != 1 {
            return Err(format!(
                "native ABI contract version {} is unsupported; runtime requires 1",
                self.contract_version
            ));
        }
        if self.calling_convention != "C" {
            return Err(format!(
                "native ABI calling convention '{}' is unsupported; expected C",
                self.calling_convention
            ));
        }
        let mut type_names = std::collections::BTreeSet::new();
        for layout in &self.types {
            if !type_names.insert(layout.name.clone()) || layout.name.is_empty() {
                return Err(format!("duplicate or empty native ABI type '{}'", layout.name));
            }
            if layout.size == 0
                || layout.alignment == 0
                || !layout.alignment.is_power_of_two()
            {
                return Err(format!(
                    "native ABI type '{}' has invalid size/alignment {}/{}",
                    layout.name, layout.size, layout.alignment
                ));
            }
            let mut fields = layout.fields.iter().collect::<Vec<_>>();
            fields.sort_by_key(|field| field.offset);
            let mut names = std::collections::BTreeSet::new();
            let mut previous_end = 0_u32;
            for field in fields {
                if field.name.is_empty()
                    || field.type_name.is_empty()
                    || !names.insert(field.name.clone())
                    || field.size == 0
                {
                    return Err(format!(
                        "native ABI type '{}' has an invalid field declaration",
                        layout.name
                    ));
                }
                let end = field.offset.checked_add(field.size).ok_or_else(|| {
                    format!("native ABI field '{}.{}' overflows", layout.name, field.name)
                })?;
                if field.offset < previous_end || end > layout.size {
                    return Err(format!(
                        "native ABI field '{}.{}' overlaps or exceeds size {}",
                        layout.name, field.name, layout.size
                    ));
                }
                previous_end = end;
            }
        }
        Ok(())
    }
}

/// Exact semantic contract attached to one native export. Effect strings at
/// the C boundary use `reads:Type`, `writes:Type`, `emits:Event`, `io`, and
/// `async`; registration rejects every unknown or duplicate declaration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeEffectSet {
    reads: std::sync::Arc<[String]>,
    writes: std::sync::Arc<[String]>,
    emits: std::sync::Arc<[String]>,
    io: bool,
    asynchronous: bool,
}

impl NativeEffectSet {
    pub fn declare(effects: &[&str]) -> Result<Self, String> {
        Self::parse(
            &effects
                .iter()
                .map(|effect| (*effect).to_string())
                .collect::<Vec<_>>(),
        )
    }

    fn parse(raw: &[String]) -> Result<Self, String> {
        let mut reads = Vec::new();
        let mut writes = Vec::new();
        let mut emits = Vec::new();
        let mut io = false;
        let mut asynchronous = false;
        let mut seen = std::collections::BTreeSet::new();
        for effect in raw {
            if !seen.insert(effect.clone()) {
                return Err(format!("duplicate native effect declaration `{effect}`"));
            }
            if effect == "io" {
                io = true;
            } else if effect == "async" {
                asynchronous = true;
            } else if let Some(name) = effect.strip_prefix("reads:") {
                validate_effect_name(effect, name)?;
                reads.push(name.to_string());
            } else if let Some(name) = effect.strip_prefix("writes:") {
                validate_effect_name(effect, name)?;
                writes.push(name.to_string());
            } else if let Some(name) = effect.strip_prefix("emits:") {
                validate_effect_name(effect, name)?;
                emits.push(name.to_string());
            } else {
                return Err(format!(
                    "unknown native effect `{effect}`; expected reads:Type, writes:Type, emits:Event, io, or async"
                ));
            }
        }
        reads.sort();
        writes.sort();
        emits.sort();
        Ok(Self {
            reads: reads.into(),
            writes: writes.into(),
            emits: emits.into(),
            io,
            asynchronous,
        })
    }

    pub fn reads(&self) -> &[String] {
        &self.reads
    }

    pub fn writes(&self) -> &[String] {
        &self.writes
    }

    pub fn emits(&self) -> &[String] {
        &self.emits
    }

    pub fn performs_io(&self) -> bool {
        self.io
    }

    pub fn is_async(&self) -> bool {
        self.asynchronous
    }

    fn canonical_strings(&self) -> Vec<String> {
        let mut effects = Vec::new();
        effects.extend(self.reads.iter().map(|name| format!("reads:{name}")));
        effects.extend(self.writes.iter().map(|name| format!("writes:{name}")));
        effects.extend(self.emits.iter().map(|name| format!("emits:{name}")));
        if self.io {
            effects.push("io".to_string());
        }
        if self.asynchronous {
            effects.push("async".to_string());
        }
        effects
    }

    pub(crate) fn canonical_strings_for_runtime(&self) -> Vec<String> {
        self.canonical_strings()
    }
}

fn validate_effect_name(effect: &str, name: &str) -> Result<(), String> {
    let valid = !name.is_empty()
        && name.chars().enumerate().all(|(index, ch)| {
            ch == '_' || ch == '$' || ch.is_ascii_alphabetic() || ch.is_ascii_digit() && index > 0
        });
    if valid {
        Ok(())
    } else {
        Err(format!(
            "invalid authority name in native effect `{effect}`"
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeExportManifest {
    name: String,
    arity: u32,
    effects: NativeEffectSet,
    signature: String,
    deterministic: bool,
    replayable: bool,
}

impl NativeExportManifest {
    pub fn new(
        name: impl Into<String>,
        arity: u32,
        effects: NativeEffectSet,
        signature: impl Into<String>,
        deterministic: bool,
        replayable: bool,
    ) -> Result<Self, String> {
        let name = name.into();
        validate_effect_name(&format!("export:{name}"), &name)?;
        let signature = signature.into();
        if signature.trim().is_empty() || !signature.contains("->") {
            return Err(format!("native export `{name}` has an invalid signature"));
        }
        Ok(Self {
            name,
            arity,
            effects,
            signature,
            deterministic,
            replayable,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn arity(&self) -> u32 {
        self.arity
    }

    pub fn effects(&self) -> &NativeEffectSet {
        &self.effects
    }

    pub fn signature(&self) -> &str {
        &self.signature
    }

    pub fn deterministic(&self) -> bool {
        self.deterministic
    }

    pub fn replayable(&self) -> bool {
        self.replayable
    }
}

/// Stable, pointer-free identity of one loaded native implementation.
///
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeExtensionManifest {
    extension_id: String,
    extension_version: Option<String>,
    abi_version: u32,
    content_digest: String,
    target: String,
    exported_functions: std::sync::Arc<[NativeExportManifest]>,
    abi_contract: NativeAbiContract,
    resource_contract_version: u32,
    digest: String,
}

impl NativeExtensionManifest {
    #[cfg(not(target_arch = "wasm32"))]
    fn semantic_digest(
        extension_id: &str,
        extension_version: &str,
        content_digest: &str,
        target: &str,
        exported_functions: &[NativeExportManifest],
        abi_contract: &NativeAbiContract,
    ) -> String {
        let mut out =
            crate::canonical::CanonicalWriter::with_domain("rad-native-extension-manifest/v3");
        out.text(extension_id);
        out.text(extension_version);
        out.u32(RAD_EXTENSION_ABI_VERSION);
        out.text(content_digest);
        out.text(target);
        out.usize(exported_functions.len());
        for export in exported_functions {
            out.text(&export.name);
            out.u32(export.arity);
            out.text(&export.signature);
            out.bool(export.deterministic);
            out.bool(export.replayable);
            let effects = export.effects.canonical_strings();
            out.usize(effects.len());
            for effect in effects {
                out.text(&effect);
            }
        }
        out.text(
            &serde_json::to_string(abi_contract)
                .expect("validated native ABI contract serializes"),
        );
        out.u32(NATIVE_RESOURCE_CONTRACT_VERSION);
        hex::encode(Sha256::digest(out.finish()))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn from_binary(
        path: &Path,
        bytes: &[u8],
        extension_id: String,
        extension_version: String,
        exports: &[NativeExportManifest],
        abi_contract: NativeAbiContract,
    ) -> Self {
        let _ = path;
        let content_digest = hex::encode(Sha256::digest(bytes));
        let target = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);
        let mut exported_functions = exports.to_vec();
        exported_functions.sort_by(|a, b| a.name.cmp(&b.name));

        let digest = Self::semantic_digest(
            &extension_id,
            &extension_version,
            &content_digest,
            &target,
            &exported_functions,
            &abi_contract,
        );

        Self {
            extension_id,
            extension_version: Some(extension_version),
            abi_version: RAD_EXTENSION_ABI_VERSION,
            content_digest,
            target,
            exported_functions: exported_functions.into(),
            abi_contract,
            resource_contract_version: NATIVE_RESOURCE_CONTRACT_VERSION,
            digest,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn from_replay(
        extension_id: String,
        extension_version: String,
        content_digest: String,
        target: String,
        mut exported_functions: Vec<NativeExportManifest>,
        abi_contract: NativeAbiContract,
        expected_digest: &str,
    ) -> Result<Self, String> {
        abi_contract.validate()?;
        exported_functions.sort_by(|left, right| left.name.cmp(&right.name));
        let digest = Self::semantic_digest(
            &extension_id,
            &extension_version,
            &content_digest,
            &target,
            &exported_functions,
            &abi_contract,
        );
        if digest != expected_digest {
            return Err(format!(
                "recorded native manifest digest mismatch: expected {expected_digest}, computed {digest}"
            ));
        }
        Ok(Self {
            extension_id,
            extension_version: Some(extension_version),
            abi_version: RAD_EXTENSION_ABI_VERSION,
            content_digest,
            target,
            exported_functions: exported_functions.into(),
            abi_contract,
            resource_contract_version: NATIVE_RESOURCE_CONTRACT_VERSION,
            digest,
        })
    }

    pub fn extension_id(&self) -> &str {
        &self.extension_id
    }

    pub fn extension_version(&self) -> Option<&str> {
        self.extension_version.as_deref()
    }

    pub fn abi_version(&self) -> u32 {
        self.abi_version
    }

    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn exported_functions(&self) -> &[NativeExportManifest] {
        &self.exported_functions
    }

    pub fn abi_contract(&self) -> &NativeAbiContract {
        &self.abi_contract
    }

    pub fn resource_contract_version(&self) -> u32 {
        self.resource_contract_version
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub(crate) fn encode_manifest(&self, out: &mut crate::canonical::CanonicalWriter) {
        out.text(&self.extension_id);
        out.optional_text(self.extension_version.as_deref());
        out.u32(self.abi_version);
        out.text(&self.content_digest);
        out.text(&self.target);
        out.usize(self.exported_functions.len());
        for export in self.exported_functions.iter() {
            out.text(&export.name);
            out.u32(export.arity);
            out.text(&export.signature);
            out.bool(export.deterministic);
            out.bool(export.replayable);
            let effects = export.effects.canonical_strings();
            out.usize(effects.len());
            for effect in effects {
                out.text(&effect);
            }
        }
        out.text(
            &serde_json::to_string(&self.abi_contract)
                .expect("validated native ABI contract serializes"),
        );
        out.u32(self.resource_contract_version);
    }
}



/// Deny-by-default builtin mask for sandboxed execution.
///
/// Everything with an IO/Async effect is denied (files, network, stdin,
/// clocks, dynamic extensions) except `print`/`eprint`, which are buffered
/// because sandbox VMs run with output suppressed, and `rand_*`, which is
/// deterministic under the per-run seed. The WHOLE speculation/persistence
/// family (`fork`/`simulate*`/`fork_*`/`merge_forks*`/`load_world`/
/// `try_load_world`/`sandbox_run`/`commit`) is denied by name to prevent
/// nesting and wholesale world replacement; `commit` in particular is never
/// grantable — only the host commits.
pub fn builtin_allowed_in_sandbox(builtin: Builtin) -> bool {
    match builtin {
        // Buffered, host-inspectable output.
        Builtin::Print | Builtin::Eprint => true,
        // Guest randomness is a supported, deterministic feature: every
        // sandboxed run is seeded (DEFAULT_SEED or the grant's seed), so
        // rand_* stays allowed even though its honest effect row is IO.
        Builtin::RandInt | Builtin::RandFloat | Builtin::RandBool | Builtin::RandSeed => true,
        // Speculation family: no nesting, no committing from inside. The
        // whole family must be listed by name, because its members carry the
        // ECS effect (not IO/Async) and would otherwise fall through to the
        // allow arm — which is exactly what happened to the newer members:
        // simulate_many/simulate_seeded/fork_with nested speculation, and
        // load_world/try_load_world/fork_from_bytes/fork_apply/merge_forks
        // replaced or rewrote the guest world WHOLESALE, bypassing the
        // per-component write ACL that gates set/spawn/set_resource
        // (dogfood seq 254 residual audit, item 4).
        Builtin::Fork
        | Builtin::Simulate
        | Builtin::SimulatePar
        | Builtin::SimulateMany
        | Builtin::SimulateSeeded
        | Builtin::ForkWith
        | Builtin::ForkFromBytes
        | Builtin::ForkApply
        | Builtin::MergeForks
        | Builtin::MergeForksWith
        | Builtin::LoadWorld
        | Builtin::TryLoadWorld
        | Builtin::SandboxRun
        | Builtin::Commit
        | Builtin::Peek => false,
        // Host environment probes.
        Builtin::SysArgs | Builtin::LoadExtension | Builtin::GcCollect => false,
        // Everything else: deny if it carries IO or Async effects
        // (files, network, stdin, clocks, metrics), allow otherwise.
        b => {
            let effects = crate::builtins::builtin_effect(b.name());
            !effects.allows(Effect::IO) && !effects.allows(Effect::Async)
        }
    }
}

/// Capability grant for a single sandboxed simulation.
#[derive(Clone, Debug)]
pub struct SandboxCaps {
    /// World types the sandbox may write: component names for `set`/`spawn`
    /// and module-qualified authoritative relation identities for resolver
    /// fact operations.
    /// Empty set = no writes permitted at all.
    pub writable_components: HashSet<String>,
    /// Component/resource types the sandbox may read via `get` / `res` /
    /// `query` / … A grant with no `"read"` key gets `{"*"}` (read
    /// everything), so the read dimension is opt-in and existing grants keep
    /// their prior behavior. `{"*"}` in the set is the wildcard; an explicit
    /// list is an allowlist and also gates the bulk readers (which require
    /// the wildcard, mirroring how `despawn` requires the `"*"` write grant).
    pub readable_components: HashSet<String>,
    /// Instruction budget charged on loop back-edges and calls.
    pub fuel: u64,
    /// GC allocation ceiling in bytes.
    pub mem_limit: usize,
}

impl SandboxCaps {
    /// Trusted-constructor default: reads everything (`{"*"}`), matching the
    /// pre-read-dimension behavior. `from_json` overrides `readable_components`
    /// when the grant carries an explicit `"read"` key.
    pub fn new(writable_components: HashSet<String>, fuel: u64, mem_limit: usize) -> Self {
        SandboxCaps {
            writable_components,
            readable_components: HashSet::from(["*".to_string()]),
            fuel,
            mem_limit,
        }
    }

    /// Whether a write to `component` is permitted by this grant.
    pub fn may_write(&self, component: &str) -> bool {
        self.writable_components.contains("*") || self.writable_components.contains(component)
    }

    /// Structural changes (`despawn`) require the wildcard grant, since they
    /// touch every component on the entity.
    pub fn may_despawn(&self) -> bool {
        self.writable_components.contains("*")
    }

    /// Whether a read of `component` is permitted by this grant.
    pub fn may_read(&self, component: &str) -> bool {
        self.readable_components.contains("*") || self.readable_components.contains(component)
    }

    /// Bulk readers that dump or enumerate the whole world (`save_world`,
    /// `world_digest`, `entities()` with no filter) cannot be keyed to a
    /// single component, so they require the wildcard read grant — the same
    /// precedent as `despawn` requiring the wildcard write grant.
    pub fn may_read_all(&self) -> bool {
        self.readable_components.contains("*")
    }

    /// Parse a capability grant from its JSON wire format:
    ///
    /// ```json
    /// { "write": ["Health", "PlanBuffer"], "fuel": 1000000,
    ///   "mem_bytes": 16777216, "seed": 42 }
    /// ```
    ///
    /// Missing keys fall back to defaults; `write` defaults to empty (deny all
    /// writes). Returns `(caps, seed)`.
    pub fn from_json(text: &str) -> Result<(SandboxCaps, u64), String> {
        let parsed: serde_json::Value =
            serde_json::from_str(text).map_err(|e| format!("sandbox caps: invalid JSON: {}", e))?;
        let obj = parsed
            .as_object()
            .ok_or_else(|| "sandbox caps: expected a JSON object".to_string())?;

        let mut writable = HashSet::new();
        if let Some(w) = obj.get("write") {
            let arr = w.as_array().ok_or_else(|| {
                "sandbox caps: 'write' must be an array of component names".to_string()
            })?;
            for item in arr {
                let name = item
                    .as_str()
                    .ok_or_else(|| "sandbox caps: 'write' entries must be strings".to_string())?;
                writable.insert(name.to_string());
            }
        }
        // `read` is symmetric with `write`; a MISSING key means "read
        // everything" (`{"*"}`) so a pre-read-dimension grant is unchanged,
        // while a PRESENT key — even `[]` — is an explicit allowlist that
        // also denies the bulk readers. `[]` therefore means "read nothing",
        // exactly as `write: []` means "write nothing".
        let readable = match obj.get("read") {
            Some(r) => {
                let arr = r.as_array().ok_or_else(|| {
                    "sandbox caps: 'read' must be an array of component names".to_string()
                })?;
                let mut set = HashSet::new();
                for item in arr {
                    let name = item.as_str().ok_or_else(|| {
                        "sandbox caps: 'read' entries must be strings".to_string()
                    })?;
                    set.insert(name.to_string());
                }
                set
            }
            None => HashSet::from(["*".to_string()]),
        };
        let fuel = match obj.get("fuel") {
            Some(v) => v
                .as_u64()
                .ok_or_else(|| "sandbox caps: 'fuel' must be a non-negative integer".to_string())?,
            None => DEFAULT_FUEL,
        };
        let mem_limit = match obj.get("mem_bytes") {
            Some(v) => v.as_u64().ok_or_else(|| {
                "sandbox caps: 'mem_bytes' must be a non-negative integer".to_string()
            })? as usize,
            None => DEFAULT_MEM_BYTES,
        };
        let seed = match obj.get("seed") {
            Some(v) => v
                .as_u64()
                .ok_or_else(|| "sandbox caps: 'seed' must be a non-negative integer".to_string())?,
            None => DEFAULT_SEED,
        };
        for key in obj.keys() {
            if !matches!(
                key.as_str(),
                "write" | "read" | "fuel" | "mem_bytes" | "seed"
            ) {
                return Err(format!("sandbox caps: unknown key '{}'", key));
            }
        }
        let mut caps = SandboxCaps::new(writable, fuel, mem_limit);
        caps.readable_components = readable;
        Ok((caps, seed))
    }
}

/// Result of running untrusted source in a guest VM (see
/// `VM::run_sandbox_guest`). Everything here is plain data — no values from
/// the guest heap survive into this struct.
pub struct SandboxOutcome {
    /// The guest's final world on success, or its failure message (compile
    /// error, capability denial, budget exhaustion, runtime error).
    pub result: Result<crate::world::WorldSnapshot, String>,
    /// Buffered guest `print` output.
    pub prints: Vec<String>,
    /// Fuel consumed (charge points crossed: loop back-edges and calls).
    pub fuel_spent: u64,
    /// JSON set by the guest's last `sandbox_output(v)` call, if any.
    pub output_json: Option<String>,
}

/// Deterministic per-fork seed derivation (SplitMix64 finalizer).
///
/// Used so that `simulate_par(world, schedule, ticks, n, seed)` produces
/// bit-identical results regardless of how many threads execute the forks.
#[inline]
pub fn fork_seed(parent_seed: u64, fork_index: u64) -> u64 {
    let mut z = parent_seed.wrapping_add(fork_index.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let out = z ^ (z >> 31);
    if out == 0 {
        0xD1B5_4A32_D192_ED03
    } else {
        out
    }
}

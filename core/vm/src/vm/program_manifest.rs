//! Canonical identity of one sealed executable program.
//!
//! Portable replay must bind names to the same slots and bytecode, not merely
//! observe the same mutable runtime values. This manifest is rebuilt from the
//! VM's private, sealed program tables and is the sole source of
//! `program_digest()`.

use super::VM;
use crate::canonical::CanonicalWriter;
use sha2::{Digest, Sha256};

pub const COMPILER_SEMANTIC_VERSION: u32 = 1;
pub const BYTECODE_SEMANTIC_VERSION: u32 = 1;
pub const PROGRAM_MANIFEST_VERSION: u32 = 2;

/// Immutable canonical description of the executable program installed in a
/// VM. Runtime values, handler `fired` bits, and other attempt state belong to
/// the replay checkpoint rather than this artifact.
#[derive(Clone)]
pub struct CompiledProgramManifest {
    canonical_bytes: std::sync::Arc<[u8]>,
    digest: String,
}

impl CompiledProgramManifest {
    pub(crate) fn capture(vm: &VM) -> Self {
        let mut out = CanonicalWriter::with_domain("rad-compiled-program-manifest/v2");
        out.u32(PROGRAM_MANIFEST_VERSION);
        out.u32(COMPILER_SEMANTIC_VERSION);
        out.u32(BYTECODE_SEMANTIC_VERSION);
        out.optional_text(vm.program_source_identity.as_deref());

        // Exact global name -> slot mapping. Slot order is semantic because
        // call_global resolves a name to an index before reading globals.
        out.usize(vm.global_names.len());
        for name in vm.global_names.iter() {
            out.text(name);
        }

        // Sealed instruction streams and source maps. Source lines are part
        // of the identity because they appear in canonical diagnostics.
        out.usize(vm.chunks.len());
        let mut constant_roots = Vec::new();
        for chunk in vm.chunks.iter() {
            out.text(chunk.name());
            out.bytes(chunk.code());
            out.usize(chunk.lines().len());
            for line in chunk.lines() {
                out.u32(*line);
            }
            out.usize(chunk.constants().len());
            constant_roots.extend_from_slice(chunk.constants());
        }
        out.text(&crate::vm::replay_clone::fingerprint_roots(&constant_roots));

        let mut machines = vm.state_machines.iter().collect::<Vec<_>>();
        machines.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(machines.len());
        for (machine, states) in machines {
            out.text(machine);
            let mut states = states.iter().collect::<Vec<_>>();
            states.sort_by(|left, right| left.0.cmp(right.0));
            out.usize(states.len());
            for (state, transitions) in states {
                out.text(state);
                out.usize(transitions.len());
                for transition in transitions {
                    out.text(&transition.event);
                    out.text(&transition.target);
                    out.optional_u64(transition.guard_chunk_id.map(|id| id as u64));
                }
            }
        }

        let mut handlers = vm.event_handlers.iter().collect::<Vec<_>>();
        handlers.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(handlers.len());
        for (event, entries) in handlers {
            out.text(event);
            out.usize(entries.len());
            for entry in entries {
                out.usize(entry.chunk_id);
                out.u16(entry.param_slot);
                out.bool(entry.once);
                out.bool(entry.has_guard);
            }
        }

        let mut systems = vm.systems.iter().collect::<Vec<_>>();
        systems.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(systems.len());
        for (name, system) in systems {
            out.text(name);
            out.usize(system.chunk_id);
            encode_signature(&mut out, &system.params);
            encode_signature(&mut out, &system.resource_params);
            encode_sorted_strings(&mut out, system.after.iter());
            encode_sorted_strings(&mut out, system.before.iter());
            out.bool(system.serial_group.is_some());
            if let Some(group) = system.serial_group {
                out.u32(group);
            }
            encode_sorted_strings(&mut out, system.accum_resources.iter());
        }

        let mut intents = vm.intent_registry.iter().collect::<Vec<_>>();
        intents.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(intents.len());
        for (name, intent) in intents {
            out.text(name);
            out.text(&intent.name);
            out.text(&intent.key_field);
            out.usize(intent.fields.len());
            for field in intent.fields.iter() {
                out.text(field);
            }
        }

        let mut resolvers = vm.resolver_registry.iter().collect::<Vec<_>>();
        resolvers.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(resolvers.len());
        for (owned_intent, resolver) in resolvers {
            out.text(owned_intent);
            out.text(&resolver.name);
            out.text(&resolver.intent);
            out.u16(resolver.global_slot);
        }

        let mut constraints = vm.constraint_registry.iter().collect::<Vec<_>>();
        constraints.sort_by(|left, right| {
            (&left.name, &left.attached_component, left.global_slot).cmp(&(
                &right.name,
                &right.attached_component,
                right.global_slot,
            ))
        });
        out.usize(constraints.len());
        for constraint in constraints {
            out.text(&constraint.name);
            out.text(&constraint.attached_component);
            encode_sorted_strings(&mut out, constraint.watches.iter());
            out.u16(constraint.global_slot);
        }

        let mut layouts = vm.component_layouts.iter().collect::<Vec<_>>();
        layouts.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(layouts.len());
        for (name, fields) in layouts {
            out.text(name);
            out.usize(fields.len());
            for field in fields.iter() {
                out.text(field);
            }
        }

        let mut field_types = vm.component_field_types.iter().collect::<Vec<_>>();
        field_types.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(field_types.len());
        for (name, fields) in field_types {
            out.text(name);
            out.usize(fields.len());
            for (field, ty) in fields.iter() {
                out.text(field);
                out.text(&ty.to_string());
            }
        }

        let mut versions = vm.component_versions.iter().collect::<Vec<_>>();
        versions.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(versions.len());
        for (name, version) in versions {
            out.text(name);
            out.u32(*version);
        }

        let mut variants = vm.variant_layouts.iter().collect::<Vec<_>>();
        variants.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(variants.len());
        for ((type_name, variant), fields) in variants {
            out.text(type_name);
            out.text(variant);
            out.usize(fields.len());
            for field in fields {
                out.text(field);
            }
        }

        encode_sorted_strings(&mut out, vm.transient_resources.iter());

        let mut indexed = vm.indexed_decl.iter().collect::<Vec<_>>();
        indexed.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(indexed.len());
        for (component, fields) in indexed {
            out.text(component);
            encode_sorted_strings(&mut out, fields.iter());
        }

        let mut migrations = vm.migrations.iter().collect::<Vec<_>>();
        migrations.sort_by(|left, right| left.0.cmp(right.0));
        out.usize(migrations.len());
        for (component, migration) in migrations {
            out.text(component);
            out.usize(migration.chunk_id);
            out.u16(migration.param_slot);
            out.bool(migration.version_slot.is_some());
            if let Some(slot) = migration.version_slot {
                out.u16(slot);
            }
        }

        // Native implementations are executable program input. Bind their
        // content-addressed manifests rather than process-local function
        // pointers or name/arity alone.
        let mut extensions = vm.native_extension_manifests.iter().collect::<Vec<_>>();
        extensions.sort_by(|left, right| left.digest().cmp(right.digest()));
        out.usize(extensions.len());
        for extension in extensions {
            extension.encode_manifest(&mut out);
        }

        let canonical_bytes: std::sync::Arc<[u8]> = out.finish().into();
        let digest = hex::encode(Sha256::digest(&canonical_bytes));
        Self {
            canonical_bytes,
            digest,
        }
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

fn encode_signature(out: &mut CanonicalWriter, signature: &[(String, bool, String)]) {
    out.usize(signature.len());
    for (parameter, mutable, component) in signature {
        out.text(parameter);
        out.bool(*mutable);
        out.text(component);
    }
}

fn encode_sorted_strings<'a>(
    out: &mut CanonicalWriter,
    values: impl IntoIterator<Item = &'a String>,
) {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort();
    out.usize(values.len());
    for value in values {
        out.text(value);
    }
}

#[cfg(test)]
mod tests {
    use crate::compiler::StateTransitionInfo;
    use crate::vm::{
        ConstraintRuntimeInfo, HandlerEntry, IntentRuntimeInfo, MigrationEntry,
        ResolverRuntimeInfo, SystemRuntimeInfo, VM,
    };
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    #[test]
    fn global_symbol_slot_order_is_part_of_program_identity() {
        let mut left = VM::new_with_seed(7);
        let source = r#"
            fn attempt() { return 1 }
            fn alternate() { return 2 }
        "#;
        let mut lexer = crate::lexer::Lexer::new(source);
        let tokens = lexer.tokenize().0;
        let mut parser = crate::parser::Parser::new(tokens);
        let program = parser.parse();
        let result = crate::compiler::Compiler::new()
            .compile(&program)
            .expect("compile");
        left.load_compile_result(result);

        let original = left.compiled_program_manifest().digest().to_string();
        let attempt = left
            .global_names
            .iter()
            .position(|name| name == "attempt")
            .unwrap();
        let alternate = left
            .global_names
            .iter()
            .position(|name| name == "alternate")
            .unwrap();
        std::sync::Arc::make_mut(&mut left.global_names).swap(attempt, alternate);
        let swapped = left.compiled_program_manifest().digest().to_string();

        assert_ne!(original, swapped);
    }

    #[test]
    fn every_program_table_family_changes_manifest_identity() {
        fn baseline() -> (VM, String) {
            let vm = VM::new_with_seed(7);
            let digest = vm.compiled_program_manifest().digest().to_string();
            (vm, digest)
        }
        fn changed(vm: &VM, baseline: &str) {
            assert_ne!(vm.compiled_program_manifest().digest(), baseline);
        }

        let (mut vm, digest) = baseline();
        Arc::make_mut(&mut vm.state_machines).insert(
            "Door".into(),
            HashMap::from([(
                "Closed".into(),
                vec![StateTransitionInfo {
                    event: "Open".into(),
                    target: "Open".into(),
                    guard_chunk_id: Some(2),
                }],
            )]),
        );
        changed(&vm, &digest);

        #[cfg(not(target_arch = "wasm32"))]
        {
            let (mut vm, digest) = baseline();
            let extension = crate::ffi::NativeExtensionManifest::from_binary(
                std::path::Path::new("generic-extension.bin"),
                b"implementation-a",
                &[("transform".into(), 1)],
            );
            Arc::make_mut(&mut vm.native_extension_manifests).push(Arc::new(extension));
            changed(&vm, &digest);
        }

        let (mut vm, digest) = baseline();
        vm.program_source_identity = Some(Arc::from("source-graph-a"));
        changed(&vm, &digest);

        let (mut vm, digest) = baseline();
        Arc::make_mut(&mut vm.event_handlers).insert(
            "Ping".into(),
            vec![HandlerEntry {
                chunk_id: 3,
                param_slot: 4,
                once: true,
                fired: false,
                has_guard: true,
            }],
        );
        changed(&vm, &digest);

        let (mut vm, digest) = baseline();
        Arc::make_mut(&mut vm.systems).insert(
            "Move".into(),
            SystemRuntimeInfo {
                params: vec![("position".into(), true, "Position".into())],
                resource_params: Vec::new(),
                chunk_id: 5,
                after: vec!["Input".into()],
                before: vec!["Render".into()],
                serial_group: Some(1),
                accum_resources: HashSet::new(),
            },
        );
        changed(&vm, &digest);

        let (mut vm, digest) = baseline();
        Arc::make_mut(&mut vm.intent_registry).insert(
            "Move".into(),
            IntentRuntimeInfo {
                name: "Move".into(),
                key_field: "target".into(),
                fields: Arc::new(vec!["target".into()]),
            },
        );
        Arc::make_mut(&mut vm.resolver_registry).insert(
            "Move".into(),
            ResolverRuntimeInfo {
                name: "ResolveMove".into(),
                intent: "Move".into(),
                global_slot: 7,
            },
        );
        Arc::make_mut(&mut vm.constraint_registry).push(ConstraintRuntimeInfo {
            name: "Bounds".into(),
            attached_component: "Position".into(),
            watches: Arc::new(vec!["Velocity".into()]),
            global_slot: 8,
        });
        changed(&vm, &digest);

        let (mut vm, digest) = baseline();
        Arc::make_mut(&mut vm.component_layouts)
            .insert("Position".into(), Arc::new(vec!["x".into()]));
        Arc::make_mut(&mut vm.component_versions).insert("Position".into(), 2);
        Arc::make_mut(&mut vm.variant_layouts)
            .insert(("Direction".into(), "North".into()), vec!["speed".into()]);
        Arc::make_mut(&mut vm.transient_resources).insert("Scratch".into());
        Arc::make_mut(&mut vm.indexed_decl).insert("Position".into(), HashSet::from(["x".into()]));
        vm.migrations.insert(
            "Position".into(),
            MigrationEntry {
                chunk_id: 9,
                param_slot: 1,
                version_slot: Some(2),
            },
        );
        changed(&vm, &digest);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_extension_binary_content_is_part_of_program_identity() {
        fn with_extension(bytes: &[u8]) -> String {
            let mut vm = VM::new_with_seed(7);
            let extension = crate::ffi::NativeExtensionManifest::from_binary(
                std::path::Path::new("generic-extension.bin"),
                bytes,
                &[("transform".into(), 1)],
            );
            Arc::make_mut(&mut vm.native_extension_manifests).push(Arc::new(extension));
            vm.compiled_program_manifest().digest().to_string()
        }

        assert_ne!(
            with_extension(b"implementation-a"),
            with_extension(b"implementation-b")
        );
    }
}



#[cfg(test)]
mod tests {
    use crate::compiler::Compiler;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::VM;

    fn run(src: &str) -> VM {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let mut parser = Parser::new(tokens);
        let program = parser.parse();
        assert!(
            parser.errors().is_empty(),
            "parse errors: {:?}",
            parser.errors()
        );
        let result = Compiler::new().compile(&program).expect("compile");
        let mut vm = VM::new();
        vm.suppress_output();
        vm.load_compile_result(result);
        vm.run(0).expect("run");
        vm
    }

    /// The canonical timeline dance: diverge, rewind, diverge, merge, commit.
    #[test]
    fn disjoint_field_edits_merge_cleanly() {
        let vm = run(r#"
            component Gold { amount: 0 }
            component Health { hp: 100 }
            let hero = spawn("hero", Gold { amount: 10 }, Health { hp: 100 })
            let base = fork()

            set(hero, Gold { amount: 99 })          // ours
            let ours = fork()

            commit(base)                             // rewind
            set(hero, Health { hp: 50 })             // theirs
            let theirs = fork()

            let merged = merge_forks(base, ours, theirs) |> unwrap
            commit(merged)

            let g = get(hero, Gold) |> unwrap
            let h = get(hero, Health) |> unwrap
            print(f"{g.amount} {h.hp}")
        "#);
        assert_eq!(vm.print_buffer, vec!["99 50"]);
    }

    /// Same component, *different fields*: merges — granularity is the field.
    #[test]
    fn same_component_different_fields_merge() {
        let vm = run(r#"
            component Stats { atk: 1, def: 1 }
            let hero = spawn("hero", Stats { atk: 1, def: 1 })
            let base = fork()
            set(hero, Stats { atk: 7, def: 1 })
            let ours = fork()
            commit(base)
            set(hero, Stats { atk: 1, def: 9 })
            let theirs = fork()
            let merged = merge_forks(base, ours, theirs) |> unwrap
            commit(merged)
            let s = get(hero, Stats) |> unwrap
            print(f"{s.atk} {s.def}")
        "#);
        assert_eq!(vm.print_buffer, vec!["7 9"]);
    }

    #[test]
    fn same_field_divergence_conflicts() {
        let vm = run(r#"
            component Gold { amount: 0 }
            let hero = spawn("hero", Gold { amount: 10 })
            let base = fork()
            set(hero, Gold { amount: 1 })
            let ours = fork()
            commit(base)
            set(hero, Gold { amount: 2 })
            let theirs = fork()
            match merge_forks(base, ours, theirs) {
                Ok(_) => { print("merged") }
                Err(conflicts) => {
                    print(len(conflicts))
                    for c in conflicts {
                        match c {
                            FieldConflict { ent, name, comp, field, base, ours, theirs } => {
                                print(f"{name}: {comp}.{field} base={base} ours={ours} theirs={theirs}")
                            }
                            _ => { print("unexpected kind") }
                        }
                    }
                }
            }
        "#);
        // Conflicts are data: the test *destructures* one instead of
        // grepping prose.
        assert_eq!(vm.print_buffer[0], "1");
        assert_eq!(
            vm.print_buffer[1],
            "hero: Gold.amount base=10 ours=1 theirs=2"
        );
    }

    /// Both forks setting the same field to the same value is not a conflict.
    #[test]
    fn convergent_edits_are_not_conflicts() {
        let vm = run(r#"
            component Gold { amount: 0 }
            let hero = spawn("hero", Gold { amount: 10 })
            let base = fork()
            set(hero, Gold { amount: 42 })
            let ours = fork()
            commit(base)
            set(hero, Gold { amount: 42 })
            let theirs = fork()
            let merged = merge_forks(base, ours, theirs) |> unwrap
            commit(merged)
            let g = get(hero, Gold) |> unwrap
            print(g.amount)
        "#);
        assert_eq!(vm.print_buffer, vec!["42"]);
    }

    /// Id collision between independent spawns: remap + deep reference
    /// rewrite, not a conflict. `watcher.target` (set in theirs) must follow
    /// beta to its fresh id.
    #[test]
    fn spawn_id_collision_remaps_and_rewrites_references() {
        let vm = run(r#"
            component Tag { label: "" }
            component Watch { target: 0 }
            let watcher = spawn("watcher", Watch { target: 0 })
            let base = fork()

            let alpha = spawn("alpha", Tag { label: "ours" })
            let ours = fork()

            commit(base)
            let beta = spawn("beta", Tag { label: "theirs" })
            set(watcher, Watch { target: beta })
            let theirs = fork()

            let merged = merge_forks(base, ours, theirs) |> unwrap
            commit(merged)

            let a = get_entity("alpha")
            let b = get_entity("beta")
            print(a == b)                            // distinct entities survived
            let ta = get(a, Tag) |> unwrap
            let tb = get(b, Tag) |> unwrap
            print(f"{ta.label} {tb.label}")
            let w = get(watcher, Watch) |> unwrap
            print(w.target == b)                     // reference followed the remap
        "#);
        assert_eq!(vm.print_buffer, vec!["false", "ours theirs", "true"]);
    }

    /// Names are identity: two forks spawning the same name is a conflict.
    #[test]
    fn name_collision_conflicts() {
        let vm = run(r#"
            component Tag { label: "" }
            let base = fork()
            let _a = spawn("boss", Tag { label: "ours" })
            let ours = fork()
            commit(base)
            let _b = spawn("boss", Tag { label: "theirs" })
            let theirs = fork()
            match merge_forks(base, ours, theirs) {
                Ok(_) => { print("merged") }
                Err(conflicts) => {
                    for c in conflicts {
                        match c {
                            NameConflict { name, entities } => {
                                print(f"name {name} claimed by {len(entities)} entities")
                            }
                            _ => { print("unexpected kind") }
                        }
                    }
                }
            }
        "#);
        assert_eq!(vm.print_buffer, vec!["name boss claimed by 2 entities"]);
    }

    #[test]
    fn clean_despawn_wins_but_despawn_vs_modify_conflicts() {
        // Clean: ours despawns, theirs never touched it.
        let vm = run(r#"
            component Tag { label: "" }
            let mook = spawn("mook", Tag { label: "x" })
            let keeper = spawn("keeper", Tag { label: "k" })
            let base = fork()
            despawn(mook)
            let ours = fork()
            commit(base)
            set(keeper, Tag { label: "k2" })
            let theirs = fork()
            let merged = merge_forks(base, ours, theirs) |> unwrap
            commit(merged)
            print(has(mook, Tag))
            let t = get(keeper, Tag) |> unwrap
            print(t.label)
        "#);
        assert_eq!(vm.print_buffer, vec!["false", "k2"]);

        // Dirty: ours despawns what theirs modified.
        let vm = run(r#"
            component Tag { label: "" }
            let mook = spawn("mook", Tag { label: "x" })
            let base = fork()
            despawn(mook)
            let ours = fork()
            commit(base)
            set(mook, Tag { label: "promoted" })
            let theirs = fork()
            match merge_forks(base, ours, theirs) {
                Ok(_) => { print("merged") }
                Err(msg) => { print(msg) }
            }
        "#);
        let out = &vm.print_buffer[0];
        assert!(
            out.contains("despawned in ours but modified in theirs"),
            "got: {}",
            out
        );
    }

    #[test]
    fn resource_fields_merge_independently() {
        let vm = run(r#"
            resource Bank { gold: 100, vault: "copper" }
            let base = fork()
            set_resource(Bank, Bank { gold: 250, vault: "copper" })
            let ours = fork()
            commit(base)
            set_resource(Bank, Bank { gold: 100, vault: "iron" })
            let theirs = fork()
            let merged = merge_forks(base, ours, theirs) |> unwrap
            commit(merged)
            let b = get_resource(Bank) |> unwrap
            print(f"{b.gold} {b.vault}")
        "#);
        assert_eq!(vm.print_buffer, vec!["250 iron"]);
    }

    #[test]
    fn component_added_and_removed_across_forks() {
        let vm = run(r#"
            component Tag { label: "" }
            component Buff { power: 0 }
            let hero = spawn("hero", Tag { label: "h" }, Buff { power: 3 })
            let base = fork()
            remove(hero, Buff)                       // ours removes
            let ours = fork()
            commit(base)
            set(hero, Tag { label: "renamed" })      // theirs edits another comp
            let theirs = fork()
            let merged = merge_forks(base, ours, theirs) |> unwrap
            commit(merged)
            print(has(hero, Buff))
            let t = get(hero, Tag) |> unwrap
            print(t.label)
        "#);
        assert_eq!(vm.print_buffer, vec!["false", "renamed"]);
    }

    /// Map *keys* are entity references too: remap must rewrite them.
    #[test]
    fn rewrite_covers_map_keys_and_nested_values() {
        use crate::gc::GcHeap;
        use crate::value::{MapKey, MapStorage, Value};
        use std::collections::HashMap;

        let mut gc = GcHeap::new();
        let inner_ref = Value::from_entity_id(&mut gc, 7);
        let list = Value::list(&mut gc, vec![inner_ref]);
        let mut m = MapStorage::new();
        m.insert(MapKey::Entity(7), list);
        m.insert(MapKey::Str("untouched".into()), Value::from_int(&mut gc, 1));
        let map_val = Value::map(&mut gc, m);

        let mut remap = HashMap::new();
        remap.insert(7u32, 99u32);
        let rewritten = map_val.rewrite_entity_ids(&remap, &mut gc);

        let storage = rewritten.as_map().expect("map");
        assert!(storage.contains_key(&MapKey::Entity(99)));
        assert!(!storage.contains_key(&MapKey::Entity(7)));
        let inner = storage.get(&MapKey::Entity(99)).unwrap();
        let item = inner.as_list().unwrap().iter().next().copied().unwrap();
        assert_eq!(item.as_entity_id(), Some(99));
        // Untouched subtree shares the original allocation (no copy).
        assert!(storage.contains_key(&MapKey::Str("untouched".into())));
    }

    /// merge(base, a, b) and merge(base, b, a) agree wherever no remap is
    /// involved: same final component values.
    #[test]
    fn merge_is_symmetric_for_field_edits() {
        let src = |first: &str, second: &str| {
            format!(
                r#"
                component Stats {{ atk: 1, def: 1 }}
                let hero = spawn("hero", Stats {{ atk: 1, def: 1 }})
                let base = fork()
                set(hero, Stats {{ atk: 7, def: 1 }})
                let a = fork()
                commit(base)
                set(hero, Stats {{ atk: 1, def: 9 }})
                let b = fork()
                let merged = merge_forks(base, {first}, {second}) |> unwrap
                commit(merged)
                let s = get(hero, Stats) |> unwrap
                print(f"{{s.atk}} {{s.def}}")
                "#
            )
        };
        let ab = run(&src("a", "b"));
        let ba = run(&src("b", "a"));
        assert_eq!(ab.print_buffer, ba.print_buffer);
        assert_eq!(ab.print_buffer, vec!["7 9"]);
    }

    /// D3: name claims become resolvable. Two forks spawn "T-5"; the picker
    /// answers "keep both, as T-5/a and T-5/b"; merge_forks_with applies the
    /// renames and the merged world holds both entities under their new
    /// names with their data intact.
    #[test]
    fn name_claim_resolves_with_renames() {
        let vm = run(r#"
            component Tag { label: "" }
            let base = fork()
            let _a = spawn("T-5", Tag { label: "ours" })
            let ours = fork()
            commit(base)
            let _b = spawn("T-5", Tag { label: "theirs" })
            let theirs = fork()

            match merge_forks(base, ours, theirs) {
                Ok(_) => { print("unexpected clean merge") }
                Err(conflicts) => {
                    let mut fixes = []
                    for c in conflicts {
                        match c {
                            NameConflict { name, entities } => {
                                fixes = push(fixes, (c, [f"{name}/a", f"{name}/b"]))
                            }
                            _ => { print("unexpected kind") }
                        }
                    }
                    let merged = merge_forks_with(base, ours, theirs, fixes) |> unwrap
                    commit(merged)
                }
            }

            let a = get_entity("T-5/a")
            let b = get_entity("T-5/b")
            print(a == b)
            let ta = get(a, Tag) |> unwrap
            let tb = get(b, Tag) |> unwrap
            print(f"{ta.label} {tb.label}")
            print(get_entity("T-5") == nil)
        "#);
        assert_eq!(vm.print_buffer, vec!["false", "ours theirs", "true"]);
    }

    /// A rename that still collides is re-validated, not trusted: both
    /// claimants sent to the same new name come back as a NameClaim on it.
    #[test]
    fn rename_resolution_still_colliding_reconflicts() {
        let vm = run(r#"
            component Tag { label: "" }
            let base = fork()
            let _a = spawn("T-5", Tag { label: "ours" })
            let ours = fork()
            commit(base)
            let _b = spawn("T-5", Tag { label: "theirs" })
            let theirs = fork()

            match merge_forks(base, ours, theirs) {
                Ok(_) => { print("unexpected clean merge") }
                Err(conflicts) => {
                    let mut fixes = []
                    for c in conflicts {
                        fixes = push(fixes, (c, ["T-9", "T-9"]))
                    }
                    match merge_forks_with(base, ours, theirs, fixes) {
                        Ok(_) => { print("merged?!") }
                        Err(cs) => {
                            for c in cs {
                                match c {
                                    NameConflict { name, entities } => {
                                        print(f"{name} still claimed by {len(entities)}")
                                    }
                                    _ => { print("unexpected kind") }
                                }
                            }
                        }
                    }
                }
            }
        "#);
        assert_eq!(vm.print_buffer, vec!["T-9 still claimed by 2"]);
    }

    /// A rename cannot steal a name an untouched base entity already owns:
    /// the claim comes back naming the thief and the owner.
    #[test]
    fn rename_resolution_cannot_steal_untouched_name() {
        let vm = run(r#"
            component Tag { label: "" }
            let _keeper = spawn("anchor", Tag { label: "old" })
            let base = fork()
            let _a = spawn("T-5", Tag { label: "ours" })
            let ours = fork()
            commit(base)
            let _b = spawn("T-5", Tag { label: "theirs" })
            let theirs = fork()

            match merge_forks(base, ours, theirs) {
                Ok(_) => { print("unexpected clean merge") }
                Err(conflicts) => {
                    let mut fixes = []
                    for c in conflicts {
                        fixes = push(fixes, (c, ["anchor", "T-5/b"]))
                    }
                    match merge_forks_with(base, ours, theirs, fixes) {
                        Ok(_) => { print("merged?!") }
                        Err(cs) => {
                            for c in cs {
                                match c {
                                    NameConflict { name, entities } => {
                                        print(f"{name} still claimed by {len(entities)}")
                                    }
                                    _ => { print("unexpected kind") }
                                }
                            }
                        }
                    }
                }
            }
        "#);
        assert_eq!(vm.print_buffer, vec!["anchor still claimed by 2"]);
    }

    /// "" in a rename resolution unnames: one claimant keeps the name, the
    /// other becomes anonymous but keeps its data.
    #[test]
    fn rename_resolution_empty_string_unnames() {
        let vm = run(r#"
            component Tag { label: "" }
            let base = fork()
            let _a = spawn("T-5", Tag { label: "ours" })
            let ours = fork()
            commit(base)
            let _b = spawn("T-5", Tag { label: "theirs" })
            let theirs = fork()

            match merge_forks(base, ours, theirs) {
                Ok(_) => { print("unexpected clean merge") }
                Err(conflicts) => {
                    let mut fixes = []
                    for c in conflicts {
                        fixes = push(fixes, (c, ["T-5", ""]))
                    }
                    let merged = merge_forks_with(base, ours, theirs, fixes) |> unwrap
                    commit(merged)
                }
            }

            let a = get_entity("T-5")
            let ta = get(a, Tag) |> unwrap
            print(ta.label)
            let mut labels = []
            for e in entities(Tag) {
                let t = get(e, Tag) |> unwrap
                labels = push(labels, t.label)
            }
            print(len(labels))
        "#);
        assert_eq!(vm.print_buffer, vec!["ours", "2"]);
    }

    /// RenameConflict (same id carrying different names in both forks —
    /// reachable through id reuse: despawn frees the id, respawn reclaims
    /// it under a new name) takes a single chosen name as its resolution.
    #[test]
    fn rename_conflict_resolves_with_chosen_name() {
        let vm = run(r#"
            component Tag { label: "" }
            let e = spawn("draft", Tag { label: "x" })
            let base = fork()
            despawn(e)
            let _o = spawn("ours-name", Tag { label: "x" })
            let ours = fork()
            commit(base)
            despawn(e)
            let _t = spawn("theirs-name", Tag { label: "x" })
            let theirs = fork()

            match merge_forks(base, ours, theirs) {
                Ok(_) => { print("unexpected clean merge") }
                Err(conflicts) => {
                    let mut fixes = []
                    for c in conflicts {
                        match c {
                            RenameConflict { ent, base, ours, theirs } => {
                                fixes = push(fixes, (c, "final-name"))
                            }
                            _ => { print("unexpected kind") }
                        }
                    }
                    let merged = merge_forks_with(base, ours, theirs, fixes) |> unwrap
                    commit(merged)
                }
            }

            let f = get_entity("final-name")
            let t = get(f, Tag) |> unwrap
            print(t.label)
            print(get_entity("ours-name") == nil)
            print(get_entity("theirs-name") == nil)
        "#);
        assert_eq!(vm.print_buffer, vec!["x", "true", "true"]);
    }
}

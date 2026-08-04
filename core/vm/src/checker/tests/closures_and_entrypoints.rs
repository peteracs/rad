

    #[test]
    fn closure_destructure_uses_expected_pipeline_param_types() {
        let errors = check_src(
            r#"
            fn main() -> nil {
                let rows = [(1, 2), (3, 4)]
                let out = rows |> map(fn([a, b]) { return a + b })
                print(out)
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "expected closure destructure to typecheck with inferred tuple element type, got: {:?}",
            errors
        );
    }

    #[test]
    fn closure_destructure_reports_tuple_arity_mismatch() {
        let errors = check_src(
            r#"
            fn takes_pair(f: fn((int, int)) -> int) -> int {
                return f((1, 2))
            }
            fn main() -> nil {
                let _ = takes_pair(fn([a, b, c]: (int, int)) { return a })
            }
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("tuple value has 2 elements but 3 bindings")),
            "expected tuple arity mismatch error, got: {:?}",
            errors
        );
    }

    #[test]
    fn closure_cross_destructure_duplicate_name_detected() {
        let errors = check_src(
            r#"
            fn main() -> nil {
                let rows = [[(1, 2), (3, 4)]]
                let _ = rows |> map(fn([a, b], [a, c]) { return a })
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Duplicate parameter")),
            "expected duplicate parameter error for cross-destructure, got: {:?}",
            errors
        );
    }

    #[test]
    fn closure_destructure_name_conflicts_with_plain_param() {
        let errors = check_src(
            r#"
            fn main() -> nil {
                let rows = [[(1, 2)]]
                let _ = rows |> reduce(0, fn(a, [a, b]) { return a })
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Duplicate parameter")),
            "expected duplicate parameter error for destructure vs plain param, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_spawn_rejected() {
        let errors = check_src(
            r#"
            resource ClusterPool {
              free_workers: 2
            }
            fn main() -> nil {
              spawn(ClusterPool { free_workers: 1 })
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("spawn() cannot add resource")),
            "expected spawn-resource rejection, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_component_name_collision() {
        let errors = check_src(
            r#"
            component Pool { size: 0 }
            resource Pool { size: 0 }
            fn main() -> nil {}
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("conflicts with an existing component")),
            "expected name collision error, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_update_with_entity_rejected() {
        let errors = check_src(
            r#"
            resource Settings { volume: 50 }
            fn main() -> nil {
              let e = spawn()
              update(e, Settings) { volume = 80 }
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("is a resource; use update(Settings)")),
            "expected update(entity, resource) rejection, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_entities_query_rejected() {
        let errors = check_src(
            r#"
            resource Config { debug: false }
            fn main() -> nil {
              let _ = entities(Config)
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("entities() cannot query resource")),
            "expected entities-resource rejection, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_duplicate_declaration_rejected() {
        let errors = check_src(
            r#"
            resource Dup { x: 1 }
            resource Dup { x: 999 }
            fn main() -> nil {}
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Duplicate resource declaration")),
            "expected duplicate resource error, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_update_inside_system_with_mut_param_rejected() {
        let errors = check_src(
            r#"
            component Tag { label: "" }
            resource Counter { n: 0 }
            system Bad(t: Tag, c: mut Counter) {
              update(Counter) { n = 999 }
            }
            fn main() -> nil {}
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("conflicts with mutable system parameter")),
            "expected writeback conflict error, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_set_resource_inside_system_with_mut_param_rejected() {
        let errors = check_src(
            r#"
            resource Counter { n: 0 }
            system Bad(c: mut Counter) {
              set_resource(Counter, Counter { n: 500 })
            }
            fn main() -> nil {}
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("set_resource(Counter, ...) conflicts with mutable system parameter")),
            "expected set_resource conflict error, got: {:?}",
            errors
        );
    }

    #[test]
    fn resource_update_with_readonly_param_allowed() {
        let errors = check_src(
            r#"
            resource Config { value: 0 }
            system Reader(c: Config) {
              let _ = c.value
            }
            fn main() -> nil {}
        "#,
        );
        assert!(
            !errors
                .iter()
                .any(|e| e.message.contains("conflicts with mutable")),
            "readonly resource param should not trigger conflict, got: {:?}",
            errors
        );
    }

    #[test]
    fn get_resource_on_component_rejected() {
        let errors = check_src(
            r#"
            component Foo { x: 1 }
            fn main() -> nil {
              let _ = get_resource(Foo)
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("is a component, not a resource")),
            "expected component-not-resource error, got: {:?}",
            errors
        );
    }

    #[test]
    fn set_resource_on_component_rejected() {
        let errors = check_src(
            r#"
            component Foo { x: 1 }
            fn main() -> nil {
              set_resource(Foo, Foo { x: 99 })
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("is a component, not a resource")),
            "expected component-not-resource error, got: {:?}",
            errors
        );
    }

    // ---- shift operators (`<<` / `>>` as int expressions) ----

    #[test]
    fn shift_ops_type_as_int() {
        let errors = check_src(
            r#"
            fn f(a: int, b: int) -> int {
              return a << 2 | b >> 3
            }
            fn main() -> nil { print(f(8, 64)) }
        "#,
        );
        assert!(
            errors.is_empty(),
            "int shifts should check clean, got: {:?}",
            errors
        );
    }

    #[test]
    fn shift_on_float_rejected() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let x = 1.5 << 2
              print(x)
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("requires int operands")),
            "expected int-operand error for float shift, got: {:?}",
            errors
        );
    }

    #[test]
    fn shift_on_list_expression_points_at_push() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let xs = [1, 2]
              let ys = xs << 3
              print(ys)
            }
        "#,
        );
        let e = errors
            .iter()
            .find(|e| e.message.contains("int left shift"))
            .expect("expected list-shift expression error");
        assert!(
            e.hint.as_deref().is_some_and(|h| h.contains("push(xs, v)")),
            "hint should point at push(), got: {:?}",
            e.hint
        );
    }

    #[test]
    fn statement_level_list_append_still_checks_clean() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let mut xs = [1]
              xs << 2
              xs << 3 << 4
              print(xs)
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "list append statements must stay legal, got: {:?}",
            errors
        );
    }

    // ---- builtin shadowing (bindings shadow builtins, like the runtime) ----

    #[test]
    fn calling_shadowed_builtin_is_compile_error_with_rename_hint() {
        let errors = check_src(
            r#"
            fn walk(range: int) -> int {
              let mut acc = 0
              for i in range(0, range) {
                acc = acc + i
              }
              return acc
            }
            fn main() -> nil { print(walk(5)) }
        "#,
        );
        let e = errors
            .iter()
            .find(|e| e.message.contains("Cannot call non-function 'range'"))
            .expect("expected non-callable error for shadowed builtin");
        assert!(
            e.hint
                .as_deref()
                .is_some_and(|h| h.contains("shadows the builtin")),
            "hint should explain the builtin shadow, got: {:?}",
            e.hint
        );
    }

    #[test]
    fn defining_builtin_named_binding_warns() {
        let warnings = check_src_warnings(
            r#"
            fn f(range: int) -> int { return range }
            fn main() -> nil { print(f(1)) }
        "#,
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("shadows the builtin function 'range()'")),
            "expected builtin-shadow warning, got: {:?}",
            warnings
        );
    }

    #[test]
    fn fn_typed_binding_named_like_builtin_does_not_warn() {
        let warnings = check_src_warnings(
            r#"
            fn main() -> nil {
              let lookup = fn(x: int) -> int { return x + 1 }
              print(lookup(1))
            }
        "#,
        );
        assert!(
            !warnings
                .iter()
                .any(|w| w.message.contains("shadows the builtin")),
            "fn-typed bindings are callable, no shadow warning expected, got: {:?}",
            warnings
        );
    }

    // ---- pub let ----

    #[test]
    fn pub_let_unused_is_not_warned() {
        let warnings = check_src_warnings(
            r#"
            pub let EXPORTED = 42
            fn main() -> nil {}
        "#,
        );
        assert!(
            !warnings
                .iter()
                .any(|w| w.message.contains("Unused variable 'EXPORTED'")),
            "pub let is a module export, never 'unused', got: {:?}",
            warnings
        );
    }

    #[test]
    fn private_top_level_let_unused_still_warns() {
        let warnings = check_src_warnings(
            r#"
            let PRIVATE = 42
            fn main() -> nil {}
        "#,
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("Unused variable 'PRIVATE'")),
            "private unused top-level lets must keep warning, got: {:?}",
            warnings
        );
    }

    // ---- indexed update blocks ----

    #[test]
    fn update_indexed_on_non_list_field_rejected() {
        let errors = check_src(
            r#"
            component C { tag: int = 0 }
            fn main() -> nil {
              let e = entity "e" { C {} }
              update(e, C) { tag[0] = 1 }
            }
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("indexed assignment needs an indexable field")),
            "expected non-indexable indexed-update error, got: {:?}",
            errors
        );
    }

    #[test]
    fn update_indexed_index_must_be_int() {
        let errors = check_src(
            r#"
            component C { vals: list = [0] }
            fn main() -> nil {
              let e = entity "e" { C {} }
              update(e, C) { vals["x"] = 1 }
            }
        "#,
        );
        assert!(
            errors.iter().any(|e| e
                .message
                .contains("index for list field 'vals' must be int")),
            "expected int-index error, got: {:?}",
            errors
        );
    }

    #[test]
    fn update_indexed_on_list_field_checks_clean() {
        let errors = check_src(
            r#"
            component C { vals: list = [0, 0] }
            fn main() -> nil {
              let e = entity "e" { C {} }
              update(e, C) { vals[1] = 9 }
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "indexed list update should check clean, got: {:?}",
            errors
        );
    }

    // ---- mixed map values widen like mixed lists ----

    #[test]
    fn mixed_map_values_warn_and_widen() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let mixed = {"scores": [1, 2, 3], "owner_count": 3}
              print(mixed["owner_count"])
            }
        "#,
        );
        assert!(
            !errors
                .iter()
                .any(|e| e.message.contains("Heterogeneous map")),
            "mixed map values should warn, not error, got: {:?}",
            errors
        );
        let warnings = check_src_warnings(
            r#"
            fn main() -> nil {
              let mixed = {"scores": [1, 2, 3], "owner_count": 3}
              print(mixed["owner_count"])
            }
        "#,
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.message.contains("mixed value types")),
            "expected mixed-map warning, got: {:?}",
            warnings
        );
    }

    #[test]
    fn map_any_annotation_silences_mixed_value_warning() {
        // The warning's own hint says "annotate the binding as `map<K, any>`
        // to silence this warning" — applied literally, it used to warn
        // identically (dogfood bug seq 58-6d: only the list<any> half of the
        // suppression predicate existed). The annotated binding must be
        // silent; the unannotated case (covered above) must still warn.
        let warnings = check_src_warnings(
            r#"
            fn main() -> nil {
              let annotated: map<str, any> = { "n": 1, "s": "two" }
              print(annotated["n"])
            }
        "#,
        );
        assert!(
            !warnings
                .iter()
                .any(|w| w.message.contains("mixed value types")),
            "map<K, any> annotation must silence the mixed-value warning, got: {:?}",
            warnings
        );

        // Re-assignment to a binding already typed map<K, any> is the same
        // accepted-mixed contract.
        let warnings = check_src_warnings(
            r#"
            fn main() -> nil {
              let mut m: map<str, any> = {}
              m = { "n": 1, "s": "two" }
              print(m["n"])
            }
        "#,
        );
        assert!(
            !warnings
                .iter()
                .any(|w| w.message.contains("mixed value types")),
            "assignment to a map<K, any> binding must not warn, got: {:?}",
            warnings
        );
    }

    #[test]
    fn update_indexed_on_map_field_checks_clean() {
        let errors = check_src(
            r#"
            component Inv { items: map<str, int> = {} }
            fn main() -> nil {
              let e = entity "e" { Inv {} }
              update(e, Inv) { items["sword"] = 1 }
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "map-field keyed update should check clean, got: {:?}",
            errors
        );
    }

    #[test]
    fn update_indexed_map_key_type_mismatch_rejected() {
        let errors = check_src(
            r#"
            component Inv { items: map<str, int> = {} }
            fn main() -> nil {
              let e = entity "e" { Inv {} }
              update(e, Inv) { items[3] = 1 }
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("key for map field 'items' expects str")),
            "expected map key type error, got: {:?}",
            errors
        );
    }

    #[test]
    fn update_indexed_map_value_type_mismatch_rejected() {
        let errors = check_src(
            r#"
            component Inv { items: map<str, int> = {} }
            fn main() -> nil {
              let e = entity "e" { Inv {} }
              update(e, Inv) { items["sword"] = "two" }
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("map field 'items' holds int values")),
            "expected map value type error, got: {:?}",
            errors
        );
    }

    #[test]
    fn set_at_returns_collection_type() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let xs: list<int> = [1, 2]
              let ys: list<int> = set_at(xs, 0, 9)
              let m: map<str, int> = {"a": 1}
              let m2: map<str, int> = set_at(m, "b", 2)
              print(ys)
              print(m2)
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "set_at should preserve collection types, got: {:?}",
            errors
        );
    }

    #[test]
    fn set_at_on_map_with_wrong_key_type_rejected() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let m: map<str, int> = {"a": 1}
              print(set_at(m, 3, 2))
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("set_at() map key expects str")),
            "expected map key error, got: {:?}",
            errors
        );
    }

    #[test]
    fn set_at_on_int_rejected() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              print(set_at(5, 0, 1))
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("set_at() expects a list or map")),
            "expected non-collection error, got: {:?}",
            errors
        );
    }

    // ---- res(): direct resource access ----

    #[test]
    fn res_reads_resource_with_precise_field_types() {
        let errors = check_src(
            r#"
            resource Rng { s: int = 12345 }
            fn next() -> int {
              return res(Rng).s * 3
            }
            fn main() -> nil { print(next()) }
        "#,
        );
        assert!(
            errors.is_empty(),
            "res(Rng).s should check as int, got: {:?}",
            errors
        );
    }

    #[test]
    fn res_field_typo_is_caught() {
        let errors = check_src(
            r#"
            resource Rng { s: int = 12345 }
            fn main() -> nil {
              print(res(Rng).seed)
            }
        "#,
        );
        let e = errors
            .iter()
            .find(|e| e.message.contains("No field 'seed' on resource 'Rng'"))
            .expect("expected unknown-field error on resource");
        assert!(
            e.hint
                .as_deref()
                .is_some_and(|h| h.contains("Available fields: s")),
            "hint should list resource fields, got: {:?}",
            e.hint
        );
    }

    #[test]
    fn res_on_component_rejected() {
        let errors = check_src(
            r#"
            component Foo { x: int = 1 }
            fn main() -> nil {
              print(res(Foo))
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("'Foo' is a component, not a resource")),
            "expected component-not-resource error, got: {:?}",
            errors
        );
    }

    #[test]
    fn res_on_unknown_name_rejected() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              print(res(Nope))
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("unknown resource 'Nope'")),
            "expected unknown-resource error, got: {:?}",
            errors
        );
    }

    #[test]
    fn get_in_unannotated_fn_stays_allowed() {
        let errors = check_src(
            r#"
            component C { x: int = 5 }
            fn read_x(e: entity) -> int {
              return unwrap(get(e, C)).x
            }
            fn main() -> nil {
              let e = entity "e" { C {} }
              print(read_x(e))
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "ECS reads in unannotated fns are allowed (inference must not over-restrict), got: {:?}",
            errors
        );
    }

    #[test]
    fn res_rejected_in_pure_fn() {
        let errors = check_src(
            r#"
            resource Cfg { n: int = 1 }
            pure fn bad() -> int {
              return res(Cfg).n
            }
            fn main() -> nil { print(bad()) }
        "#,
        );
        assert!(
            !errors.is_empty(),
            "res() reads world state and must not pass inside `pure fn`"
        );
    }

    // ---- buffcore round: ~, for-where, .field, sum/product, system self ----

    #[test]
    fn bitnot_requires_int() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let x = ~1.5
              print(x)
            }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Bitwise '~' requires int")),
            "expected int-operand error for ~float, got: {:?}",
            errors
        );
    }

    #[test]
    fn bitnot_on_int_checks_clean() {
        let errors = check_src(
            r#"
            fn main() -> nil {
              let all = 7
              let revoked = 2
              print(all & ~revoked)
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "~int should check clean, got: {:?}",
            errors
        );
    }

    #[test]
    fn system_self_is_entity_typed() {
        let errors = check_src(
            r#"
            component Tag {}
            component Score { points: int = 0 }
            system bump(Tag, s: mut Score) {
              s.points = s.points + require(self, Score).points
              remove(self, Tag)
            }
            fn main() -> nil {
              let _e = entity "e" { Tag {}, Score {} }
              schedule [bump]
            }
        "#,
        );
        assert!(
            errors.is_empty(),
            "self in systems should check clean, got: {:?}",
            errors
        );
    }

    #[test]
    fn self_undefined_outside_systems() {
        let errors = check_src(
            r#"
            component Score { points: int = 0 }
            fn bad() -> int {
              return require(self, Score).points
            }
            fn main() -> nil { print(bad()) }
        "#,
        );
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("Undefined variable 'self'")),
            "self must not leak outside system bodies, got: {:?}",
            errors
        );
    }
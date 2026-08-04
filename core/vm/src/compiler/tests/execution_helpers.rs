
    use crate::compiler::*;
    use crate::lexer::Lexer;
    use crate::parser::{Parser, ParserOptions};
    use crate::vm::VM;

    fn run_source(src: &str) -> Vec<String> {
        run_source_result(src).expect("expected program to run successfully")
    }

    fn run_source_result(src: &str) -> Result<Vec<String>, String> {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = Parser::new(tokens).parse();
        let compiler = Compiler::new();
        let result = compiler.compile(&program).unwrap();
        let mut vm = VM::new();
        vm.load_compile_result(result);
        vm.run(0)?;
        Ok(vm.print_buffer.clone())
    }

    fn run_source_result_with_compat(src: &str) -> Result<Vec<String>, String> {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = Parser::new(tokens)
            .with_options(ParserOptions {
                compat_v0_5_dx: true,
            })
            .parse();
        let compiler = Compiler::new();
        let result = compiler.compile(&program).unwrap();
        let mut vm = VM::new();
        vm.load_compile_result(result);
        vm.run(0)?;
        Ok(vm.print_buffer.clone())
    }

    #[test]
    fn phase_declaration_accepts_bracket_and_brace_forms() {
        // spec §3.5.1 and the changelog spell phases with brackets
        // (`phase P [A, B]`, matching `schedule [...]`); the parser
        // historically only accepted braces. Both must parse and behave
        // identically.
        let brackets = run_source(
            r#"
            component W { n: 0 }
            resource T { a: 0 }
            system A(w: W, t: mut T) { t.a = t.a + w.n }
            spawn(W { n: 5 })
            phase Front [A]
            schedule [Front]
            print(f"a={res(T).a}")
            "#,
        );
        let braces = run_source(
            r#"
            component W { n: 0 }
            resource T { a: 0 }
            system A(w: W, t: mut T) { t.a = t.a + w.n }
            spawn(W { n: 5 })
            phase Front { A }
            schedule [Front]
            print(f"a={res(T).a}")
            "#,
        );
        assert_eq!(brackets, braces);
        assert_eq!(brackets, vec!["a=5"]);
    }

    #[test]
    fn compile_function_scope_bug() {
        let output = run_source(
            r#"
            fn bar() {
                let x = 10
            }
            fn foo() {
                let x = 5
                bar()
                return x
            }
            print(foo())
        "#,
        );
        assert_eq!(output, vec!["5"]);
    }

    #[test]
    fn compile_closure_capture_bug() {
        let output = run_source(
            r#"
            fn make_adder(base) {
                let add = fn(n) {
                    return base + n
                }
                return add
            }
            let add10 = make_adder(10)
            let add20 = make_adder(20)
            print(add10(5))
            print(add20(5))
        "#,
        );
        assert_eq!(output, vec!["15", "25"]);
    }

    #[test]
    fn compile_closure_mutation_persists_across_calls() {
        let output = run_source(
            r#"
            fn make_counter() {
                let mut n = 0
                let inc = fn() {
                    n = n + 1
                    return n
                }
                return inc
            }
            let c = make_counter()
            print(c())
            print(c())
        "#,
        );
        assert_eq!(output, vec!["1", "2"]);
    }

    #[test]
    fn compile_closure_mutation_updates_outer_local() {
        let output = run_source(
            r#"
            fn bump_once() {
                let mut x = 0
                let inc = fn() {
                    x = x + 1
                }
                inc()
                return x
            }
            print(bump_once())
        "#,
        );
        assert_eq!(output, vec!["1"]);
    }

    #[test]
    fn let_rec_local_factorial() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rec fact = fn(n) {
                    if n <= 1 {
                        return 1
                    }
                    return n * fact(n - 1)
                }
                print(fact(5))
            }
        "#,
        );
        assert_eq!(output, vec!["120"]);
    }

    #[test]
    fn let_rec_global_factorial() {
        let output = run_source(
            r#"
            let rec fact = fn(n) {
                if n <= 1 {
                    return 1
                }
                return n * fact(n - 1)
            }
            print(fact(6))
        "#,
        );
        assert_eq!(output, vec!["720"]);
    }

    #[test]
    fn let_rec_captures_outer_variable() {
        let output = run_source(
            r#"
            fn make_searcher(target) {
                let rec search = fn(items, idx) {
                    if idx >= len(items) {
                        return -1
                    }
                    if items[idx] == target {
                        return idx
                    }
                    return search(items, idx + 1)
                }
                return search
            }
            let find = make_searcher(30)
            print(find([10, 20, 30, 40], 0))
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn let_rec_mutual_via_closure_mutation() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let mut is_even = fn(n) { return false }
                let rec is_odd = fn(n) {
                    if n == 0 { return false }
                    return is_even(n - 1)
                }
                is_even = fn(n) {
                    if n == 0 { return true }
                    return is_odd(n - 1)
                }
                print(is_even(4))
                print(is_odd(3))
            }
        "#,
        );
        assert_eq!(output, vec!["true", "true"]);
    }

    #[test]
    fn compile_hello() {
        let output = run_source(r#"print("Hello from compiled Rad!")"#);
        assert_eq!(output, vec!["Hello from compiled Rad!"]);
    }

    #[test]
    fn compile_arithmetic() {
        let output = run_source("print(2 + 3 * 4)");
        assert_eq!(output, vec!["14"]);
    }

    #[test]
    fn compile_let_and_global() {
        let output = run_source("let x = 10\nlet y = 20\nprint(x + y)");
        assert_eq!(output, vec!["30"]);
    }

    #[test]
    fn compile_if_else() {
        let output = run_source(
            r#"
            let x = 5
            if x > 3 {
                print("big")
            } else {
                print("small")
            }
        "#,
        );
        assert_eq!(output, vec!["big"]);
    }

    #[test]
    fn compile_while_loop() {
        let output = run_source(
            r#"
            let mut i = 0
            while i < 3 {
                print(i)
                i = i + 1
            }
        "#,
        );
        assert_eq!(output, vec!["0", "1", "2"]);
    }

    #[test]
    fn compile_function_call() {
        let output = run_source(
            r#"
            fn double(x) {
                return x * 2
            }
            print(double(21))
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn compile_list_ops() {
        let output = run_source(
            r#"
            let items = [10, 20, 30]
            print(len(items))
            print(items[1])
        "#,
        );
        assert_eq!(output, vec!["3", "20"]);
    }

    #[test]
    fn compile_string_concat() {
        let output = run_source(
            r#"
            let a = "hello"
            let b = " world"
            print(a + b)
        "#,
        );
        assert_eq!(output, vec!["hello world"]);
    }

    #[test]
    fn compile_comparison() {
        let output = run_source(
            r#"
            print(10 > 5)
            print(3 == 3)
            print(1 != 2)
        "#,
        );
        assert_eq!(output, vec!["true", "true", "true"]);
    }

    #[test]
    fn compile_nested_fn() {
        let output = run_source(
            r#"
            fn apply(f, x) {
                return f(x)
            }
            fn inc(n) {
                return n + 1
            }
            print(apply(inc, 41))
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn compile_for_loop() {
        let output = run_source(
            r#"
            let items = [10, 20, 30]
            for x in items {
                print(x)
            }
        "#,
        );
        assert_eq!(output, vec!["10", "20", "30"]);
    }

    #[test]
    fn compile_for_loop_empty() {
        let output = run_source(
            r#"
            let items = []
            for x in items {
                print(x)
            }
            print("done")
        "#,
        );
        assert_eq!(output, vec!["done"]);
    }

    #[test]
    fn compile_for_with_computation() {
        let output = run_source(
            r#"
            let items = [1, 2, 3, 4, 5]
            let mut total = 0
            for x in items {
                total = total + x
            }
            print(total)
        "#,
        );
        assert_eq!(output, vec!["15"]);
    }

    #[test]
    fn compile_break_in_while() {
        let output = run_source(
            r#"
            let mut i = 0
            while i < 100 {
                if i == 3 {
                    break
                }
                print(i)
                i = i + 1
            }
            print("done")
        "#,
        );
        assert_eq!(output, vec!["0", "1", "2", "done"]);
    }

    #[test]
    fn compile_break_in_for() {
        let output = run_source(
            r#"
            let items = [10, 20, 30, 40, 50]
            for x in items {
                if x == 30 {
                    break
                }
                print(x)
            }
            print("done")
        "#,
        );
        assert_eq!(output, vec!["10", "20", "done"]);
    }

    #[test]
    fn compile_nested_for() {
        let output = run_source(
            r#"
            let rows = [1, 2]
            let cols = [10, 20]
            for r in rows {
                for c in cols {
                    print(r + c)
                }
            }
        "#,
        );
        assert_eq!(output, vec!["11", "21", "12", "22"]);
    }

    #[test]
    fn compile_pipe_builtin() {
        let output = run_source(
            r#"
            let result = [3, 1, 2] |> sort
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[1, 2, 3]"]);
    }

    #[test]
    fn compile_pipe_fn() {
        let output = run_source(
            r#"
            fn double(x) {
                return x * 2
            }
            let result = 21 |> double
            print(result)
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn compile_range_builtin() {
        let output = run_source(
            r#"
            let items = range(5)
            print(items)
        "#,
        );
        assert_eq!(output, vec!["[0, 1, 2, 3, 4]"]);
    }

    #[test]
    fn compile_range_start_end() {
        let output = run_source(
            r#"
            let items = range(2, 5)
            print(items)
        "#,
        );
        assert_eq!(output, vec!["[2, 3, 4]"]);
    }

    #[test]
    fn compile_map_filter_reduce() {
        let output = run_source(
            r#"
            fn double(x) {
                return x * 2
            }
            fn is_big(x) {
                return x > 4
            }
            fn add(a, b) {
                return a + b
            }
            let items = [1, 2, 3, 4, 5]
            let doubled = map(items, double)
            print(doubled)
            let big = filter(items, is_big)
            print(big)
            let total = reduce(items, 0, add)
            print(total)
        "#,
        );
        assert_eq!(output, vec!["[2, 4, 6, 8, 10]", "[5]", "15"]);
    }

    #[test]
    fn compile_push_sort_reverse() {
        let output = run_source(
            r#"
            let items = [3, 1, 2]
            let items = push(items, 0)
            print(items)
            let sorted = sort(items)
            print(sorted)
            let rev = reverse(sorted)
            print(rev)
        "#,
        );
        assert_eq!(output, vec!["[3, 1, 2, 0]", "[0, 1, 2, 3]", "[3, 2, 1, 0]"]);
    }

    #[test]
    fn compile_slice_builtin() {
        let output = run_source(
            r#"
            let items = [10, 20, 30, 40, 50]
            let s = slice(items, 1, 4)
            print(s)
        "#,
        );
        assert_eq!(output, vec!["[20, 30, 40]"]);
    }

    #[test]
    fn compile_min_max_abs() {
        let output = run_source(
            r#"
            print(min(3, 7))
            print(max(3, 7))
            print(abs(-5))
        "#,
        );
        assert_eq!(output, vec!["3", "7", "5"]);
    }

    #[test]
    fn compile_int_division_truncates_for_int_operands() {
        let output = run_source(
            r#"
            print(10 / 2)
            print(5 / 2)
        "#,
        );
        assert_eq!(output, vec!["5", "2"]);
    }

    #[test]
    fn compile_contains_keys() {
        let output = run_source(
            r#"
            let items = [1, 2, 3]
            print(contains(items, 2))
            print(contains(items, 5))
        "#,
        );
        assert_eq!(output, vec!["true", "false"]);
    }

    #[test]
    fn compile_map_entries_merge_group_by() {
        let output = run_source(
            r#"
            fn parity(n) {
                if n % 2 == 0 {
                    return "even"
                }
                return "odd"
            }
            let a = {"x": 1}
            let b = {"y": 2, "x": 3}
            let m = merge(a, b)
            print(m["x"])
            print(len(entries(m)))
            let m2 = remove_key(m, "x")
            print(m2["y"])
            print(len(entries(m2)))
            let grouped = group_by([1, 2, 3, 4], parity)
            print(grouped["even"])
            print(grouped["odd"])
        "#,
        );
        assert_eq!(output, vec!["3", "2", "2", "1", "[2, 4]", "[1, 3]"]);
    }

    #[test]
    fn compile_typeof_str_int_float() {
        let output = run_source(
            r#"
            print(typeof(42))
            print(typeof("hi"))
            print(str(123))
            print(int(3.7))
            print(float(5))
        "#,
        );
        assert_eq!(output, vec!["int", "str", "123", "3", "5.0"]);
    }

    #[test]
    fn compile_component_entity() {
        let output = run_source(
            r#"
            component Health {
                hp: 100
            }
            entity player {
                Health { hp: 50 }
            }
            let h = get(player, Health) |> unwrap
            print(h.hp)
        "#,
        );
        assert_eq!(output, vec!["50"]);
    }

    #[test]
    fn compile_system_mutation_in_main_does_not_corrupt_stack() {
        let output = run_source(
            r#"
            component Position { x: 0.0, y: 0.0 }
            component Velocity { dx: 2.0, dy: 0.0 }
            entity player {
                Position { x: 0.0, y: 0.0 },
                Velocity { dx: 2.0, dy: 0.0 }
            }
            system Physics(pos: mut Position, vel: Velocity) {
                pos.x = pos.x + vel.dx
            }
            fn process_scores(scores) {
                let filtered = filter(scores, fn(x) { return x > 1 })
                print(len(filtered))
            }
            fn main() -> nil {
                Physics()
                print((get(player, Position) |> unwrap).x)
                process_scores([1, 2, 3])
            }
        "#,
        );
        assert_eq!(output, vec!["2.0", "2"]);
    }

    #[test]
    fn forward_reference_to_top_level_fn_resolves() {
        // Calling a top-level fn before its declaration statement used to
        // trap on `nil` at runtime even though the checker resolved the
        // call; top-level fn definitions are now hoisted ahead of top-level
        // statements, matching the documented forward-reference guarantee.
        let out = run_source(
            r#"
            let doubled = twice(21)
            print(f"{doubled}")
            fn twice(n: int) -> int {
                return n * 2
            }
        "#,
        );
        assert_eq!(out, vec!["42"]);
    }

    #[test]
    fn hoisted_fn_calling_later_system_still_compiles_as_system_call() {
        // A hoisted fn body compiles before every non-fn declaration, so it
        // must learn "Physics is a system" from the metadata pre-pass, not
        // from compile order — otherwise Physics() compiles as a plain
        // global call and traps on nil (the regression that forced the
        // first attempt at hoisting to be reverted).
        let output = run_source(
            r#"
            fn tick() {
                Physics()
            }
            component Position { x: 0.0 }
            component Velocity { dx: 2.0 }
            entity player { Position { x: 0.0 }, Velocity { dx: 2.0 } }
            system Physics(pos: mut Position, vel: Velocity) {
                pos.x = pos.x + vel.dx
            }
            tick()
            print((get(player, Position) |> unwrap).x)
        "#,
        );
        assert_eq!(output, vec!["2.0"]);
    }

    #[test]
    fn hoisted_main_can_reference_a_declared_system_value() {
        let output = run_source(
            r#"
            component Position { x: 0 }
            system Advance(pos: mut Position) {
                pos.x = pos.x + 1
            }
            fn main() -> nil {
                let player = spawn(Position { x: 4 })
                let future = simulate(fork(), [system::Advance], 1)
                print((peek(future, player, Position) |> unwrap).x)
            }
        "#,
        );
        assert_eq!(output, vec!["5"]);
    }

    #[test]
    fn hoisted_fn_assigning_later_immutable_global_rejected() {
        // The mutability of a global declared after the fn must still reach
        // the hoisted body: without the pre-pass this assignment compiled
        // silently because `x` was not in global_mutability yet.
        let msg = compile_err(
            r#"
            fn f() {
                x = 20
            }
            let x = 10
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn hoisted_fn_assigning_later_mutable_global_allowed() {
        let output = run_source(
            r#"
            fn bump() {
                x = x + 1
            }
            let mut x = 10
            bump()
            print(x)
        "#,
        );
        assert_eq!(output, vec!["11"]);
    }

    #[test]
    fn hoisted_fn_scheduling_later_phase_expands_it() {
        // `schedule [Front]` inside a fn must expand the phase even though
        // the `phase` declaration compiles after the hoisted body.
        let output = run_source(
            r#"
            fn advance() {
                schedule [Front]
            }
            component W { n: 0 }
            resource T { a: 0 }
            system A(w: W, t: mut T) { t.a = t.a + w.n }
            phase Front [A]
            spawn(W { n: 5 })
            advance()
            print(f"a={res(T).a}")
        "#,
        );
        assert_eq!(output, vec!["a=5"]);
    }

    #[test]
    fn hoisting_preserves_statement_and_spawn_order() {
        // Hoisting moves only the DefGlobal of constant fn values; every
        // observable top-level effect (prints, spawns, queries) must keep
        // its source order.
        let output = run_source(
            r#"
            component Tag { v: 0 }
            print("first")
            fn helper() { return len(query { Tag }) }
            spawn(Tag { v: 1 })
            print(f"after one spawn: {helper()}")
            fn helper2() { return helper() * 10 }
            spawn(Tag { v: 2 })
            print(f"after two spawns: {helper2()}")
        "#,
        );
        assert_eq!(
            output,
            vec!["first", "after one spawn: 1", "after two spawns: 20"]
        );
    }

    #[test]
    fn compile_emit_in_main_does_not_corrupt_stack() {
        let output = run_source(
            r#"
            event Ping { k }
            on Ping(e) {
                print(e.k)
            }
            state DoorState {
                Locked { on unlock -> Open }
                Open { on lock -> Locked }
            }
            fn first_even(xs) {
                let ys = filter(xs, fn(x) { return x % 2 == 0 })
                return ys[0]
            }
            fn main() -> nil {
                emit Ping { k: 10 }
                emit Ping { k: 20 }
                flush_events()
                let door = DoorState::Locked
                print(door)
                print(first_even([1, 2, 3]))
            }
        "#,
        );
        assert_eq!(output, vec!["10", "20", "DoorState::Locked", "2"]);
    }

    #[test]
    fn compile_index_assign_writes_back_for_list_and_map() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let mut xs = [10, 20, 30]
                xs[0] = 99
                print(xs[0])
                let mut m = {"a": 1}
                m["a"] = 42
                print(m["a"])
            }
        "#,
        );
        assert_eq!(output, vec!["99", "42"]);
    }

    #[test]
    fn compile_spawn_builtin_attaches_components() {
        let output = run_source(
            r#"
            component Pos { x: 0 }
            component Name { value: "" }

            fn main() -> nil {
                let e1 = spawn("p1", Pos { x: 7 }, Name { value: "hero" })
                let p1 = get(e1, Pos) |> unwrap
                let n1 = get(e1, Name) |> unwrap
                print(p1.x)
                print(n1.value)

                let e2 = spawn(Pos { x: 9 })
                let p2 = get(e2, Pos) |> unwrap
                print(p2.x)
            }
        "#,
        );
        assert_eq!(output, vec!["7", "hero", "9"]);
    }

    #[test]
    fn compile_index_assign_writeback_for_global_and_upvalue() {
        let output = run_source(
            r#"
            let mut xs = [1, 2]

            fn bump_global() {
                xs[1] = 9
            }

            fn main() -> nil {
                bump_global()
                print(xs[1])

                let mut ys = [3, 4]
                let set_inner = fn() {
                    ys[0] = 8
                }
                set_inner()
                print(ys[0])
            }
        "#,
        );
        assert_eq!(output, vec!["9", "8"]);
    }

    #[test]
    fn compile_nested_index_assign_writes_back() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let mut nested = [[1, 2], [3, 4]]
                nested[0][1] = 99
                print(nested[0][1])

                let mut m = {"a": [10, 20]}
                m["a"][0] = 77
                print(m["a"][0])
            }
        "#,
        );
        assert_eq!(output, vec!["99", "77"]);
    }

    #[test]
    fn compile_spawn_builtin_ignores_non_component_args() {
        let output = run_source(
            r#"
            component Pos { x: 0 }

            fn main() -> nil {
                let e = spawn(123, Pos { x: 5 })
                let p = get(e, Pos) |> unwrap
                print(p.x)
            }
        "#,
        );
        assert_eq!(output, vec!["5"]);
    }
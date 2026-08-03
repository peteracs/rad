#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
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

    #[test]
    fn compile_state_machine() {
        let output = run_source(
            r#"
            state Door {
                Locked {
                    on unlock -> Closed
                }
                Closed {
                    on open -> Open
                }
                Open {
                    on close -> Closed
                }
            }
            let mut d = Door::Locked
            d = transition(d, "unlock") |> unwrap
            print(d)
            d = transition(d, "open") |> unwrap
            print(d)
        "#,
        );
        assert_eq!(output, vec!["Door::Closed", "Door::Open"]);
    }

    #[test]
    fn compile_match_state() {
        let output = run_source(
            r#"
            state Light {
                On {
                    on toggle -> Off
                }
                Off {
                    on toggle -> On
                }
            }
            let l = Light::On
            match l {
                On => {
                    print("light is on")
                }
                Off => {
                    print("light is off")
                }
            }
        "#,
        );
        assert_eq!(output, vec!["light is on"]);
    }

    #[test]
    fn compile_match_variant() {
        let output = run_source(
            r#"
            type Shape {
                Circle { radius: 0.0 }
                Rect { w: 0.0, h: 0.0 }
            }
            let s = Shape::Circle { radius: 5.0 }
            match s {
                Circle => {
                    print("circle")
                }
                Rect => {
                    print("rect")
                }
            }
        "#,
        );
        assert_eq!(output, vec!["circle"]);
    }

    #[test]
    fn compile_match_expression_in_let_binding() {
        let output = run_source(
            r#"
            type Shape {
                Circle { radius: 0.0 }
                Rect { w: 0.0, h: 0.0 }
            }
            let s = Shape::Rect { w: 2.0, h: 3.0 }
            let label = match s {
                Circle => { "circle" }
                Rect => { "rect" }
            }
            print(label)
        "#,
        );
        assert_eq!(output, vec!["rect"]);
    }

    #[test]
    fn compile_plain_string_dollar_interpolation() {
        let output = run_source(
            r#"
            let city = "Neo Arcadia"
            let pop = 1200
            print("city=${city}, pop=${pop}")
        "#,
        );
        assert_eq!(output, vec!["city=Neo Arcadia, pop=1200"]);
    }

    #[test]
    fn compile_match_variant_bindings_and_locals() {
        let output = run_source(
            r#"
            type Payload {
                Data { first: 0, second: 0 }
            }
            let p = Payload::Data { first: 10, second: 20 }
            match p {
                Data { first, second } => {
                    print(first)
                    print(second)
                    let total = first + second
                    print(total)
                }
            }
        "#,
        );
        assert_eq!(output, vec!["10", "20", "30"]);
    }

    #[test]
    fn compile_match_nested_destructuring_and_guard() {
        let output = run_source_result_with_compat(
            r#"
            type Meta {
                Info { code: 0 }
            }
            type Ev {
                Alarm { meta: Meta::Info { code: 9 }, level: 0 }
            }
            let ev = Ev::Alarm { meta: Meta::Info { code: 9 }, level: 3 }
            match ev {
                Alarm { meta: { code }, level: sev } when sev > 2 => {
                    print(code)
                }
                Alarm { .. } => {
                    print("fallback")
                }
            }
        "#,
        )
        .expect("expected program to run successfully");
        assert_eq!(output, vec!["9"]);
    }

    #[test]
    fn compile_unwrap_option() {
        let output = run_source(
            r#"
            component Score {
                value: 0
            }
            entity p {
                Score { value: 42 }
            }
            let s = get(p, Score) |> unwrap
            print(s.value)
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn compile_for_loop_global_scope() {
        let output = run_source(
            r#"
            for x in [1, 2, 3] {
                print(x)
            }
        "#,
        );
        assert_eq!(output, vec!["1", "2", "3"]);
    }

    #[test]
    fn compile_state_machine_transition_chain() {
        let output = run_source(
            r#"
            state Door {
                Locked {
                    on unlock -> Closed
                }
                Closed {
                    on open -> Open
                }
                Open {
                    on close -> Closed
                }
            }
            let mut d = Door::Locked
            d = transition(d, "unlock") |> unwrap
            d = transition(d, "open") |> unwrap
            d = transition(d, "close") |> unwrap
            print(d)
        "#,
        );
        assert_eq!(output, vec!["Door::Closed"]);
    }

    #[test]
    fn compile_and_or_logic() {
        let output = run_source(
            r#"
            print(true and false)
            print(true or false)
            print(not true)
        "#,
        );
        assert_eq!(output, vec!["false", "true", "false"]);
    }

    #[test]
    fn compile_nested_break() {
        let output = run_source(
            r#"
            for x in [1, 2, 3] {
                if x == 2 {
                    let y = 99
                    break
                }
                print(x)
            }
            print("done")
        "#,
        );
        assert_eq!(output, vec!["1", "done"]);
    }

    fn compile_err(src: &str) -> String {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = Parser::new(tokens).parse();
        let compiler = Compiler::new();
        compiler.compile(&program).unwrap_err().message
    }

    #[test]
    fn immutable_local_reassign_rejected() {
        let msg = compile_err(
            r#"
            fn main() -> nil {
                let x = 1
                x = 2
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn mutable_local_reassign_allowed() {
        let output = run_source(
            r#"
            fn test() {
                let mut x = 1
                x = 2
                print(x)
            }
            test()
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn immutable_global_reassign_rejected() {
        let msg = compile_err("let x = 1\nx = 2");
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn mutable_global_reassign_allowed() {
        let output = run_source("let mut x = 1\nx = 2\nprint(x)");
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn fn_param_reassign_rejected() {
        let msg = compile_err(
            r#"
            fn f(a) {
                a = 10
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'a'"),
            "got: {msg}"
        );
    }

    #[test]
    fn immutable_index_assign_rejected() {
        let msg = compile_err(
            r#"
            fn main() -> nil {
                let xs = [1, 2, 3]
                xs[0] = 9
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'xs'"),
            "got: {msg}"
        );
    }

    #[test]
    fn immutable_field_assign_rejected() {
        let msg = compile_err(
            r#"
            component Health { hp: 0 }
            entity p { Health { hp: 1 } }
            fn main() -> nil {
                let h = get(p, Health) |> unwrap
                h.hp = 9
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'h'"),
            "got: {msg}"
        );
    }

    #[test]
    fn for_var_reassign_rejected() {
        let msg = compile_err(
            r#"
            fn test() {
                for x in [1, 2, 3] {
                    x = 99
                }
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn compile_for_loop_map_iterates_keys() {
        let output = run_source(
            r#"
            let m = {"a": 1, "b": 2, "c": 3}
            for k in m {
                print(k)
            }
        "#,
        );
        assert_eq!(output, vec!["a", "b", "c"]);
    }

    #[test]
    fn compile_recursion_reports_stack_overflow() {
        let err = run_source_result(
            r#"
            fn recurse(n) {
                return recurse(n + 1)
            }
            print(recurse(0))
        "#,
        )
        .expect_err("expected recursion to fail");
        assert!(err.contains("Stack overflow"), "unexpected error: {}", err);
    }

    #[test]
    fn compile_and_or_are_boolean() {
        let output = run_source(
            r#"
            let x = true and 42
            print(x)
            print(typeof(x))
            let y = false or 7
            print(y)
            print(typeof(y))
        "#,
        );
        assert_eq!(output, vec!["true", "bool", "true", "bool"]);
    }

    #[test]
    fn compile_emit_preserves_registration_order() {
        let output = run_source(
            r#"
            event Hit { target }
            on Hit(e) { print("A") }
            on Hit(e) { print("B") }
            on Hit(e) { print("C") }
            fn main() -> nil {
                emit Hit { target: 1 }
                flush_events()
            }
        "#,
        );
        assert_eq!(output, vec!["A", "B", "C"]);
    }

    #[test]
    fn compile_fstring_in_component_default_and_event_payload() {
        let output = run_source(
            r#"
            component Audit {
                msg: f"code {7}"
            }
            event Alert { message }
            on Alert(e) {
                print(e.message)
            }
            fn main() -> nil {
                let a = Audit {}
                emit Alert { message: f"got {a.msg}" }
                flush_events()
            }
        "#,
        );
        assert_eq!(output, vec!["got code 7"]);
    }

    #[test]
    fn compile_numeric_cross_type_equality() {
        let output = run_source(
            r#"
            print(1 == 1.0)
            print(1 != 1.0)
        "#,
        );
        assert_eq!(output, vec!["true", "false"]);
    }

    #[test]
    fn compile_match_fallthrough_keeps_subject_for_bindings() {
        let output = run_source(
            r#"
            type Payload {
                Other { n: 0 }
                Data { first: 0, second: 0 }
            }
            let p = Payload::Data { first: 10, second: 20 }
            match p {
                Other { n } => {
                    print(n)
                }
                Data { first, second } => {
                    print(first)
                    print(second)
                }
            }
        "#,
        );
        assert_eq!(output, vec!["10", "20"]);
    }

    #[test]
    fn immutable_function_reassign_rejected() {
        let msg = compile_err(
            r#"
            fn f() { return 1 }
            f = 42
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'f'"),
            "got: {msg}"
        );
    }

    #[test]
    fn immutable_component_reassign_rejected() {
        let msg = compile_err(
            r#"
            component Health { hp: 100 }
            Health = 42
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'Health'"),
            "got: {msg}"
        );
    }

    #[test]
    fn immutable_entity_reassign_rejected() {
        let msg = compile_err(
            r#"
            component Health { hp: 100 }
            entity player { Health { hp: 50 } }
            player = 42
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'player'"),
            "got: {msg}"
        );
    }

    #[test]
    fn immutable_captured_variable_reassign_rejected() {
        let msg = compile_err(
            r#"
            fn f() {
                let x = 0
                let g = fn() {
                    x = 1
                }
                g()
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn mutable_upvalue_shadows_immutable_global() {
        let output = run_source(
            r#"
            let x = 10
            fn outer() {
                let mut x = 5
                let inner = fn() {
                    x = 99
                    return x
                }
                return inner()
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["99"]);
    }

    #[test]
    fn mutable_upvalue_shadows_immutable_fn_global() {
        let output = run_source(
            r#"
            fn x() { return 0 }
            fn outer() {
                let mut x = 5
                let inner = fn() {
                    x = 42
                    return x
                }
                return inner()
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn mutable_upvalue_shadows_immutable_entity_global() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            entity player { Health { hp: 50 } }
            fn outer() {
                let mut player = "shadow"
                let inner = fn() {
                    player = "reassigned"
                    return player
                }
                return inner()
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["reassigned"]);
    }

    #[test]
    fn mutable_upvalue_shadows_immutable_component_global() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            fn outer() {
                let mut Health = "shadow"
                let inner = fn() {
                    Health = "ok"
                    return Health
                }
                return inner()
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["ok"]);
    }

    #[test]
    fn immutable_upvalue_shadows_mutable_global() {
        let msg = compile_err(
            r#"
            let mut x = 10
            fn outer() {
                let x = 5
                let inner = fn() {
                    x = 99
                }
                inner()
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn deeply_nested_mutable_upvalue_shadows_immutable_global() {
        let output = run_source(
            r#"
            let x = 0
            fn a() {
                let mut x = 1
                let b = fn() {
                    let c = fn() {
                        x = 777
                        return x
                    }
                    return c()
                }
                return b()
            }
            print(a())
        "#,
        );
        assert_eq!(output, vec!["777"]);
    }

    #[test]
    fn triple_nested_immutable_upvalue_rejected() {
        let msg = compile_err(
            r#"
            fn a() {
                let x = 1
                let b = fn() {
                    let c = fn() {
                        x = 2
                    }
                    c()
                }
                b()
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn field_assign_on_mutable_upvalue_shadowing_immutable_global() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            entity player { Health { hp: 50 } }
            fn outer() {
                let mut h = get(player, Health) |> unwrap
                let inner = fn() {
                    h.hp = 999
                }
                inner()
                return h.hp
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["999"]);
    }

    #[test]
    fn index_assign_on_mutable_upvalue_shadowing_immutable_global() {
        let output = run_source(
            r#"
            let xs = [0]
            fn outer() {
                let mut xs = [10, 20, 30]
                let inner = fn() {
                    xs[1] = 99
                }
                inner()
                return xs[1]
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["99"]);
    }

    #[test]
    fn no_upvalue_no_global_passes_mutability_check() {
        let output = run_source(
            r#"
            let mut x = 1
            x = 2
            print(x)
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn upvalue_mutable_global_absent() {
        let output = run_source(
            r#"
            fn outer() {
                let mut z = 0
                let inner = fn() {
                    z = 42
                    return z
                }
                return inner()
            }
            print(outer())
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn mutable_upvalue_two_closures_independent() {
        let output = run_source(
            r#"
            let x = 0
            fn make() {
                let mut x = 10
                let inc = fn() {
                    x = x + 1
                    return x
                }
                let dec = fn() {
                    x = x - 1
                    return x
                }
                return [inc(), dec()]
            }
            print(make())
        "#,
        );
        assert_eq!(output, vec!["[11, 10]"]);
    }

    #[test]
    fn immutable_global_no_upvalue_still_rejected() {
        let msg = compile_err(
            r#"
            let x = 10
            fn f() {
                x = 20
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'x'"),
            "got: {msg}"
        );
    }

    #[test]
    fn immutable_fn_global_no_upvalue_still_rejected() {
        let msg = compile_err(
            r#"
            fn greet() { return "hi" }
            fn f() {
                greet = 42
            }
        "#,
        );
        assert!(
            msg.contains("Cannot assign to immutable variable 'greet'"),
            "got: {msg}"
        );
    }

    #[test]
    fn constant_folding_arithmetic() {
        let output = run_source("print(2 + 3 * 4)");
        assert_eq!(output, vec!["14"]);
    }

    #[test]
    fn constant_folding_string_concat() {
        let output = run_source(r#"print("hello" + " " + "world")"#);
        assert_eq!(output, vec!["hello world"]);
    }

    #[test]
    fn constant_folding_comparison() {
        let output = run_source("print(10 > 5)\nprint(3 == 3)");
        assert_eq!(output, vec!["true", "true"]);
    }

    #[test]
    fn constant_folding_negation() {
        let output = run_source("print(-42)\nprint(not true)");
        assert_eq!(output, vec!["-42", "false"]);
    }

    #[test]
    fn dead_code_after_return_is_eliminated() {
        let output = run_source(
            r#"
            fn f() {
                return 1
                print("unreachable")
                return 2
            }
            print(f())
        "#,
        );
        assert_eq!(output, vec!["1"]);
    }

    #[test]
    fn dead_code_after_break_is_eliminated() {
        let output = run_source(
            r#"
            for x in [1, 2, 3] {
                if x == 2 {
                    break
                    print("unreachable")
                }
                print(x)
            }
        "#,
        );
        assert_eq!(output, vec!["1"]);
    }

    // === Integration stress tests for reconstructed lexer/parser ===

    #[test]
    fn compile_string_escape_sequences_run_correctly() {
        let output = run_source(r#"print("hello\tworld\n!")"#);
        assert_eq!(output, vec!["hello\tworld\n!"]);
    }

    #[test]
    fn compile_fstring_interpolation_runs_correctly() {
        let output = run_source(
            r#"
            let name = "rad"
            print(f"hello {name}!")
        "#,
        );
        assert_eq!(output, vec!["hello rad!"]);
    }

    #[test]
    fn compile_triple_fstring_runs_correctly() {
        let output = run_source(
            r#"
            let n = 3
            let code = f"""
if (x) { return ${n}; }
"""
            print(code)
        "#,
        );
        assert_eq!(output, vec!["\nif (x) { return 3; }\n"]);
    }

    #[test]
    fn compile_code_with_comments_runs_correctly() {
        let output = run_source(
            r#"
            // This is a line comment
            let x = 1
            /* This is a block comment */
            let y = 2
            /* Nested /* comments */ work */
            let z = x + y
            print(z)
        "#,
        );
        assert_eq!(output, vec!["3"]);
    }

    #[test]
    fn compile_range_with_dotdot() {
        let output = run_source(
            r#"
            for i in range(1, 4) {
                print(i)
            }
        "#,
        );
        assert_eq!(output, vec!["1", "2", "3"]);
    }

    #[test]
    fn compile_float_operations() {
        let output = run_source(
            r#"
            let a = 0.5
            let b = .5
            let c = 5.
            let d = 1.5e2
            print(a == b)
            print(c)
            print(d)
        "#,
        );
        assert_eq!(output, vec!["true", "5.0", "150.0"]);
    }

    #[test]
    fn compile_method_call_on_int_literal() {
        let output = run_source(
            r#"
            let x = [1, 2, 3]
            print(len(x))
        "#,
        );
        assert_eq!(output, vec!["3"]);
    }

    #[test]
    fn compile_empty_string() {
        let output = run_source(
            r#"
            let s = ""
            print(len(s))
        "#,
        );
        assert_eq!(output, vec!["0"]);
    }

    #[test]
    fn compile_question_mark_operator_on_result_ok() {
        let output = run_source(
            r#"
            type Result {
                Ok { value: int }
                Err { message: str }
            }
            fn try_get() -> Result {
                return Result::Ok { value: 42 }
            }
            fn use_try() -> Result {
                let v = try_get()?
                return Result::Ok { value: v }
            }
            let r = use_try()
            match r {
                Ok { value } => {
                    print(value)
                }
                Err { message } => {
                    print(message)
                }
            }
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn compile_question_mark_operator_on_result_err() {
        let output = run_source(
            r#"
            type Result {
                Ok { value: int }
                Err { message: str }
            }
            fn try_get() -> Result {
                return Result::Err { message: "failed" }
            }
            fn use_try() -> Result {
                let v = try_get()?
                return Result::Ok { value: v }
            }
            let r = use_try()
            match r {
                Ok { value } => {
                    print(value)
                }
                Err { message } => {
                    print(message)
                }
            }
        "#,
        );
        assert_eq!(output, vec!["failed"]);
    }

    #[test]
    fn compile_struct_basic_creation_and_field_access() {
        let output = run_source(
            r#"
            struct Point { x: 0.0, y: 0.0 }
            let p = Point { x: 3.0, y: 4.0 }
            print(p.x)
            print(p.y)
        "#,
        );
        assert_eq!(output, vec!["3.0", "4.0"]);
    }

    #[test]
    fn compile_struct_default_fields() {
        let output = run_source(
            r#"
            struct Config { debug: false, level: 1 }
            let c = Config {}
            print(c.debug)
            print(c.level)
        "#,
        );
        assert_eq!(output, vec!["false", "1"]);
    }

    #[test]
    fn compile_struct_spread_syntax() {
        let output = run_source(
            r#"
            struct Point { x: 0.0, y: 0.0 }
            fn main() -> nil {
                let p1 = Point { x: 1.0, y: 2.0 }
                let p2 = Point { x: 10.0, ..p1 }
                print(p2.x)
                print(p2.y)
            }
        "#,
        );
        assert_eq!(output, vec!["10.0", "2.0"]);
    }

    #[test]
    fn compile_struct_field_mutation() {
        let output = run_source(
            r#"
            struct Counter { count: 0 }
            let mut c = Counter { count: 5 }
            c.count = c.count + 1
            print(c.count)
        "#,
        );
        assert_eq!(output, vec!["6"]);
    }

    #[test]
    fn compile_struct_pass_to_function() {
        let output = run_source(
            r#"
            struct Point { x: 0.0, y: 0.0 }
            fn magnitude(p) {
                return p.x + p.y
            }
            let p = Point { x: 3.0, y: 4.0 }
            print(magnitude(p))
        "#,
        );
        assert_eq!(output, vec!["7.0"]);
    }

    #[test]
    fn compile_struct_return_from_function() {
        let output = run_source(
            r#"
            struct Point { x: 0.0, y: 0.0 }
            fn make_point(a, b) {
                return Point { x: a, y: b }
            }
            let p = make_point(5.0, 6.0)
            print(p.x)
            print(p.y)
        "#,
        );
        assert_eq!(output, vec!["5.0", "6.0"]);
    }

    #[test]
    fn compile_struct_coexists_with_component() {
        let output = run_source(
            r#"
            component Position { x: 0.0, y: 0.0 }
            struct Vec2 { x: 0.0, y: 0.0 }
            let v = Vec2 { x: 1.0, y: 2.0 }
            print(v.x)
            print(v.y)
        "#,
        );
        assert_eq!(output, vec!["1.0", "2.0"]);
    }

    #[test]
    fn compile_is_none_false_for_non_option() {
        let output = run_source(
            r#"
            print(is_none(42))
            print(is_none("hello"))
            print(is_none([1, 2]))
            type Option { Some { value: 0 } None { } }
            print(is_none(Option::None {}))
            print(is_none(Option::Some { value: 5 }))
        "#,
        );
        assert_eq!(output, vec!["false", "false", "false", "true", "false"]);
    }

    #[test]
    fn compile_now_unix_s_returns_positive_int() {
        let output = run_source(
            r#"
            let t = now_unix_s()
            print(typeof(t))
            print(t > 0)
        "#,
        );
        assert_eq!(output, vec!["int", "true"]);
    }

    #[test]
    fn compile_now_unix_ms_returns_positive_int() {
        let output = run_source(
            r#"
            let t = now_unix_ms()
            print(typeof(t))
            print(t > 0)
        "#,
        );
        assert_eq!(output, vec!["int", "true"]);
    }

    #[test]
    fn compile_read_file_write_file_roundtrip() {
        let output = run_source(
            r#"
            write_file("__test_roundtrip.txt", "hello rad")
            let content = read_file("__test_roundtrip.txt")
            print(content)
            remove_file("__test_roundtrip.txt")
        "#,
        );
        assert_eq!(output, vec!["hello rad"]);
    }

    #[test]
    fn compile_file_exists_and_remove() {
        let output = run_source(
            r#"
            write_file("__test_exists.txt", "x")
            print(file_exists("__test_exists.txt"))
            remove_file("__test_exists.txt")
            print(file_exists("__test_exists.txt"))
        "#,
        );
        assert_eq!(output, vec!["true", "false"]);
    }

    #[test]
    fn compile_typeof_for_all_primitives() {
        let output = run_source(
            r#"
            print(typeof(42))
            print(typeof(3.14))
            print(typeof("hi"))
            print(typeof(true))
            print(typeof(nil))
            print(typeof([1, 2]))
            print(typeof({"a": 1}))
        "#,
        );
        assert_eq!(
            output,
            vec!["int", "float", "str", "bool", "nil", "list", "map"]
        );
    }

    #[test]
    fn compile_basic_query_no_filter() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            entity unitA { Health { hp: 80 } }
            entity unitB { Health { hp: 20 } }
            fn main() -> nil {
                let all = query { Health }
                print(len(all))
            }
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn compile_query_filter_actually_filters() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            entity unitA { Health { hp: 80 } }
            entity unitB { Health { hp: 20 } }
            entity unitC { Health { hp: 60 } }
            fn main() -> nil {
                let strong = query { Health } where Health.hp > 50
                print(len(strong))
            }
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn compile_query_filter_multi_component() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity unitA {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            entity unitB {
                Health { hp: 20 },
                Armor { def: 2 }
            }
            entity unitC {
                Health { hp: 60 },
                Armor { def: 10 }
            }
            fn main() -> nil {
                let tanky = query { Health, Armor } where Armor.def >= 5
                print(len(tanky))
            }
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn compile_query_readonly_unpack() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity unitA {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            entity unitB {
                Health { hp: 20 },
                Armor { def: 2 }
            }
            fn main() -> nil {
                for id, h, a in query { Health, Armor } {
                    print(h.hp + a.def)
                }
            }
        "#,
        );
        // unitA: 80 + 5 = 85
        // unitB: 20 + 2 = 22
        assert_eq!(output, vec!["85", "22"]);
    }

    /// `EcsHas` skip jumps + `continue`: inner scopes must not run on skip; `continue` must pop
    /// entity + component locals and land after both `end_scope`s (see `compile_for_query_unpack`).
    #[test]
    fn compile_query_unpack_continue_skips_and_loop_depth() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity unitA {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            entity unitB {
                Health { hp: 20 },
                Armor { def: 2 }
            }
            fn main() -> nil {
                for id, h, a in query { mut Health, mut Armor } {
                    continue
                }
                print("done")
            }
        "#,
        );
        assert_eq!(output, vec!["done"]);
    }

    /// Read-only multi-binding unpack also uses `skip_jumps`; `continue` must still balance scopes.
    #[test]
    fn compile_query_unpack_readonly_continue_two_components() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity unitA {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            fn main() -> nil {
                for id, h, a in query { Health, Armor } {
                    continue
                }
                print("done")
            }
        "#,
        );
        assert_eq!(output, vec!["done"]);
    }

    #[test]
    fn compile_query_unpack_break_mut_two_components() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity unitA {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            entity unitB {
                Health { hp: 20 },
                Armor { def: 2 }
            }
            fn main() -> nil {
                for id, h, a in query { mut Health, mut Armor } {
                    print("once")
                    break
                }
                print("after")
            }
        "#,
        );
        assert_eq!(output, vec!["once", "after"]);
    }

    /// Removing a component mid-loop forces later iterations through the `JumpIfFalse` skip path;
    /// `continue` after `remove` must still leave the VM stack consistent across skip vs body.
    #[test]
    fn compile_query_unpack_remove_then_skip_with_continue() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 10 }
            fn main() -> nil {
                let e1 = spawn()
                set(e1, Health { hp: 100 })
                set(e1, Armor { def: 10 })
                let e2 = spawn()
                set(e2, Health { hp: 80 })
                set(e2, Armor { def: 5 })
                for id, h, a in query { mut Health, mut Armor } {
                    if id == e1 {
                        remove(e2, Armor)
                    }
                    continue
                }
                print("done")
            }
        "#,
        );
        assert_eq!(output, vec!["done"]);
    }

    #[test]
    fn compile_query_unpack_remove_then_break() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 10 }
            fn main() -> nil {
                let e1 = spawn()
                set(e1, Health { hp: 100 })
                set(e1, Armor { def: 10 })
                let e2 = spawn()
                set(e2, Health { hp: 80 })
                set(e2, Armor { def: 5 })
                for id, h, a in query { mut Health, mut Armor } {
                    if id == e1 {
                        remove(e2, Armor)
                        h.hp = 999
                        break
                    }
                }
                for id, h in query { Health } {
                    print(h.hp)
                }
            }
        "#,
        );
        // e1 mutated to 999, e2 remains 80.
        assert_eq!(output, vec!["999", "80"]);
    }

    #[test]
    fn compile_query_unpack_return_from_main() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity unitA {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            fn main() -> nil {
                for id, h, a in query { mut Health, mut Armor } {
                    if h.hp == 80 {
                        return
                    }
                }
                print("after")
            }
        "#,
        );
        assert_eq!(output, Vec::<String>::new());
    }

    /// Exercises `return` from inside a nested function's unpack loop, ensuring writebacks
    /// are correctly emitted by `compile_return` even when earlier entities hit the `EcsHas` skip path.
    #[test]
    fn compile_query_unpack_return_writeback_in_nested_fn() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }

            fn process() {
                for id, h, a in query { mut Health, mut Armor } {
                    if h.hp == 80 {
                        h.hp = 999
                        return
                    }
                }
            }

            fn main() -> nil {
                let e1 = spawn()
                set(e1, Health { hp: 50 }) // No Armor, triggers EcsHas skip path in process()

                let e2 = spawn()
                set(e2, Health { hp: 80 })
                set(e2, Armor { def: 5 })  // Matches, mutates, and triggers return

                process()

                for id, h in query { Health } {
                    print(h.hp)
                }
            }
        "#,
        );
        assert_eq!(output, vec!["50", "999"]);
    }

    #[test]
    fn compile_tuple_literal_basic() {
        let output = run_source(
            r#"
            let t = (1, 2, 3)
            print(t)
            print(typeof(t))
            print(len(t))
        "#,
        );
        assert_eq!(output, vec!["(1, 2, 3)", "tuple", "3"]);
    }

    #[test]
    fn compile_let_tuple_destructure() {
        let output = run_source(
            r#"
            fn pair() { return (10, 20) }
            let (a, b) = pair()
            print(a)
            print(b)
            let (x, y) = [1, 2]
            print(x + y)
        "#,
        );
        assert_eq!(output, vec!["10", "20", "3"]);
    }

    #[test]
    fn compile_tuple_indexing() {
        let output = run_source(
            r#"
            let t = ("hello", 42, true)
            print(t[0])
            print(t[1])
            print(t[2])
        "#,
        );
        assert_eq!(output, vec!["hello", "42", "true"]);
    }

    #[test]
    fn compile_tuple_equality() {
        let output = run_source(
            r#"
            let a = (1, 2)
            let b = (1, 2)
            let c = (1, 3)
            print(a == b)
            print(a == c)
        "#,
        );
        assert_eq!(output, vec!["true", "false"]);
    }

    #[test]
    fn compile_tuple_single_element() {
        let output = run_source(
            r#"
            let t = (42,)
            print(t)
            print(typeof(t))
        "#,
        );
        assert_eq!(output, vec!["(42,)", "tuple"]);
    }

    #[test]
    fn compile_tuple_empty() {
        let output = run_source(
            r#"
            let t = ()
            print(typeof(t))
            print(len(t))
        "#,
        );
        assert_eq!(output, vec!["tuple", "0"]);
    }

    #[test]
    fn compile_tuple_nested() {
        let output = run_source(
            r#"
            let t = (1, (2, 3))
            print(t[0])
            print(t[1])
        "#,
        );
        assert_eq!(output, vec!["1", "(2, 3)"]);
    }

    #[test]
    fn compile_query_select_single_component() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity warrior {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            entity mage {
                Health { hp: 50 },
                Armor { def: 2 }
            }
            fn main() -> nil {
                let healths = query { Health, Armor } select Health
                print(len(healths))
            }
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn compile_query_select_multi_component() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            component Armor { def: 0 }
            entity warrior {
                Health { hp: 80 },
                Armor { def: 5 }
            }
            fn main() -> nil {
                let result = query { Health, Armor } select Health, Armor
                print(len(result))
                let pair = result[0]
                print(typeof(pair))
            }
        "#,
        );
        assert_eq!(output, vec!["1", "tuple"]);
    }

    #[test]
    fn compile_query_select_with_filter() {
        let output = run_source(
            r#"
            component Health { hp: 100 }
            entity a { Health { hp: 80 } }
            entity b { Health { hp: 20 } }
            entity c { Health { hp: 60 } }
            fn main() -> nil {
                let strong = query { Health } select Health where Health.hp > 50
                print(len(strong))
            }
        "#,
        );
        assert_eq!(output, vec!["2"]);
    }

    #[test]
    fn compile_string_builtins_comprehensive() {
        let output = run_source(
            r#"
            print(to_upper("hello"))
            print(to_lower("WORLD"))
            print(trim("  hi  "))
            print(starts_with("abc", "ab"))
            print(ends_with("abc", "bc"))
            print(contains("hello world", "world"))
            print(replace("aXbXc", "X", "_"))
            print(split("a,b,c", ","))
        "#,
        );
        assert_eq!(
            output,
            vec![
                "HELLO",
                "world",
                "hi",
                "true",
                "true",
                "true",
                "a_b_c",
                "[\"a\", \"b\", \"c\"]"
            ]
        );
    }

    fn compile_with_warnings(src: &str) -> (Vec<String>, Vec<CompileWarning>) {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().0;
        let program = Parser::new(tokens).parse();
        let compiler = Compiler::new();
        let result = compiler.compile(&program).unwrap();
        let warnings = result.warnings;
        let mut vm = VM::new();
        vm.load_compile_result(CompileResult {
            chunks: result.chunks,
            systems: result.systems,
            handlers: result.handlers,
            migrations: result.migrations,
            state_machines: result.state_machines,
            intents: result.intents,
            resolvers: result.resolvers,
            constraints: result.constraints,
            layout_analysis: crate::compiler::layout_analysis::LayoutAnalysis::default(),
            materialization_plan: crate::compiler::materialization::MaterializationPlan::default(),
            component_layouts: result.component_layouts,
            component_field_types: result.component_field_types,
            indexed_component_fields: result.indexed_component_fields,
            transient_resources: result.transient_resources,
            component_versions: result.component_versions,
            variant_layouts: result.variant_layouts,
            global_names: result.global_names,
            warnings: Vec::new(),
            gc: result.gc,
        });
        vm.run(0).expect("program should run");
        (vm.print_buffer.clone(), warnings)
    }

    #[test]
    fn vectorized_pipeline_map_mul() {
        let output = run_source(
            r#"
            let xs = [1, 2, 3, 4]
            let result = xs |> map(fn(x) { x * 2 }) |> map(fn(x) { x + 1 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[3, 5, 7, 9]"]);
    }

    #[test]
    fn vectorized_pipeline_map_and_filter() {
        let output = run_source(
            r#"
            let xs = [1, 2, 3, 4, 5, 6]
            let result = xs |> map(fn(x) { x * 2 }) |> filter(fn(x) { x > 6 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[8, 10, 12]"]);
    }

    #[test]
    fn vectorized_pipeline_filter_and_map() {
        let output = run_source(
            r#"
            let xs = [10, 20, 30, 40, 50]
            let result = xs |> filter(fn(x) { x > 20 }) |> map(fn(x) { x - 5 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[25, 35, 45]"]);
    }

    #[test]
    fn vectorized_pipeline_arithmetic() {
        let output = run_source(
            r#"
            let xs = [10, 20, 30]
            let result = xs |> map(fn(x) { x / 2 + 1 }) |> map(fn(x) { x % 3 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[0, 2, 1]"]);
    }

    #[test]
    fn vectorized_pipeline_negation() {
        let output = run_source(
            r#"
            let xs = [1, -2, 3]
            let result = xs |> map(fn(x) { -x }) |> map(fn(x) { x * 2 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[-2, 4, -6]"]);
    }

    #[test]
    fn vectorized_pipeline_comparison_filter() {
        let output = run_source(
            r#"
            let xs = [1, 2, 3, 4, 5]
            let result = xs |> filter(fn(x) { x >= 2 }) |> filter(fn(x) { x <= 4 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[2, 3, 4]"]);
    }

    #[test]
    fn vectorized_pipeline_empty_list() {
        let output = run_source(
            r#"
            let xs: list = []
            let result = xs |> map(fn(x) { x * 2 }) |> filter(fn(x) { x > 0 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[]"]);
    }

    #[test]
    fn vectorized_pipeline_with_captured_variable() {
        let output = run_source(
            r#"
            let factor = 10
            let xs = [1, 2, 3]
            let result = xs |> map(fn(x) { x * factor }) |> filter(fn(x) { x > 15 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[20, 30]"]);
    }

    #[test]
    fn vectorized_pipeline_float_arithmetic() {
        let output = run_source(
            r#"
            let xs = [1.0, 2.0, 3.0]
            let result = xs |> map(fn(x) { x * 2.5 }) |> map(fn(x) { x + 0.5 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[3.0, 5.5, 8.0]"]);
    }

    #[test]
    fn vectorized_pipeline_equality_filter() {
        let output = run_source(
            r#"
            let xs = [1, 2, 3, 2, 1]
            let result = xs |> filter(fn(x) { x == 2 }) |> map(fn(x) { x * 10 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[20, 20]"]);
    }

    #[test]
    fn vectorized_pipeline_not_filter() {
        let output = run_source(
            r#"
            let xs = [true, false, true, false]
            let result = xs |> filter(fn(x) { !x }) |> map(fn(x) { 1 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[1, 1]"]);
    }

    /// `compile_vec_body` for `if`/`else` pushes cond, then, else (else on top); `exec_vec_select`
    /// pops false/else, then/true, mask — matching that emission order.
    #[test]
    fn vectorized_pipeline_map_if_else_scalar_branches() {
        let output = run_source(
            r#"
            let xs = [-1, 2, -3, 4]
            let result = xs |> map(fn(x) { if x > 0 { x } else { -x } })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[1, 2, 3, 4]"]);
    }

    #[test]
    fn vectorized_pipeline_map_if_else_then_filter() {
        let output = run_source(
            r#"
            let xs = [-2, 3, -4, 5]
            let result = xs |> map(fn(x) { if x > 0 { x * 2 } else { x } }) |> filter(fn(x) { x > 0 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[6, 10]"]);
    }

    #[test]
    fn w2505_warning_for_non_vectorizable_closure() {
        let (output, warnings) = compile_with_warnings(
            r#"
            fn complex(x) {
                if x > 0 { return x } else { return -x }
            }
            let xs = [1, -2, 3]
            let result = xs |> map(complex) |> map(fn(x) { x + 1 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[2, 3, 4]"]);
        assert!(
            warnings.iter().any(|w| w.message.contains("W2505")),
            "Expected W2505 warning for non-vectorizable pipeline, got: {:?}",
            warnings
        );
    }

    #[test]
    fn no_w2505_for_fully_vectorizable_pipeline() {
        let (output, warnings) = compile_with_warnings(
            r#"
            let xs = [1, 2, 3]
            let result = xs |> map(fn(x) { x * 2 }) |> filter(fn(x) { x > 3 })
            print(result)
        "#,
        );
        assert_eq!(output, vec!["[4, 6]"]);
        assert!(
            !warnings.iter().any(|w| w.message.contains("W2505")),
            "Should not emit W2505 for vectorizable pipeline"
        );
    }

    #[test]
    fn no_w2505_for_flat_map_pipeline() {
        let (_, warnings) = compile_with_warnings(
            r#"
            let xs = [[1, 2], [3, 4]]
            let result = xs |> flat_map(fn(x) { x }) |> map(fn(x) { x * 2 })
            print(result)
        "#,
        );
        assert!(
            !warnings.iter().any(|w| w.message.contains("W2505")),
            "Should not emit W2505 when FlatMap is present"
        );
    }

    #[test]
    fn load_column_ecs_test() {
        let output = run_source(
            r#"
            component Position { x: 0.0, y: 0.0 }

            fn main() -> nil {
                spawn(Position { x: 10.0, y: 20.0 })
                spawn(Position { x: 30.0, y: 40.0 })

                let entities = query { Position }
                let xs = entities |> map(fn(e) { (get(e, Position) |> unwrap).x }) |> map(fn(x) { x + 1.0 })
                print(xs)
            }
        "#,
        );
        assert_eq!(output, vec!["[11.0, 31.0]"]);
    }

    #[test]
    fn fstring_format_spec_float_precision() {
        let output = run_source(
            r#"
            let pi = 3.14159265
            print(f"{pi:.2f}")
        "#,
        );
        assert_eq!(output, vec!["3.14"]);
    }

    #[test]
    fn fstring_format_spec_int_zero_pad() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x:06d}")
        "#,
        );
        assert_eq!(output, vec!["000042"]);
    }

    #[test]
    fn fstring_format_spec_right_align() {
        let output = run_source(
            r#"
            let name = "hi"
            print(f"{name:>10}")
        "#,
        );
        assert_eq!(output, vec!["        hi"]);
    }

    #[test]
    fn fstring_format_spec_left_align() {
        let output = run_source(
            r#"
            let x = 5
            print(f"{x:<5d}")
        "#,
        );
        assert_eq!(output, vec!["5    "]);
    }

    #[test]
    fn fstring_format_spec_center_align() {
        let output = run_source(
            r#"
            let x = "ab"
            print(f"{x:^6}")
        "#,
        );
        assert_eq!(output, vec!["  ab  "]);
    }

    #[test]
    fn fstring_format_spec_hex() {
        let output = run_source(
            r#"
            let x = 255
            print(f"{x:#x}")
        "#,
        );
        assert_eq!(output, vec!["0xff"]);
    }

    #[test]
    fn fstring_format_spec_binary() {
        let output = run_source(
            r#"
            let x = 10
            print(f"{x:#b}")
        "#,
        );
        assert_eq!(output, vec!["0b1010"]);
    }

    #[test]
    fn fstring_format_spec_sign_plus() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x:+d}")
        "#,
        );
        assert_eq!(output, vec!["+42"]);
    }

    #[test]
    fn fstring_format_spec_percentage() {
        let output = run_source(
            r#"
            let x = 0.75
            print(f"{x:.1%}")
        "#,
        );
        assert_eq!(output, vec!["75.0%"]);
    }

    #[test]
    fn fstring_format_spec_fill_char() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x:*>6d}")
        "#,
        );
        assert_eq!(output, vec!["****42"]);
    }

    #[test]
    fn fstring_format_spec_string_truncation() {
        let output = run_source(
            r#"
            let s = "hello world"
            print(f"{s:.5}")
        "#,
        );
        assert_eq!(output, vec!["hello"]);
    }

    #[test]
    fn fstring_format_spec_no_spec() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x}")
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn fstring_format_spec_mixed_parts() {
        let output = run_source(
            r#"
            let x = 3.14159
            print(f"Pi is {x:.2f}!")
        "#,
        );
        assert_eq!(output, vec!["Pi is 3.14!"]);
    }

    #[test]
    fn fstring_format_spec_dollar_brace() {
        let output = run_source(
            r#"
            let x = 255
            print(f"${x:#06x}")
        "#,
        );
        assert_eq!(output, vec!["0x00ff"]);
    }

    #[test]
    fn fstring_format_spec_scientific() {
        let output = run_source(
            r#"
            let x = 12345.6789
            print(f"{x:.2e}")
        "#,
        );
        assert_eq!(output, vec!["1.23e+04"]);
    }

    #[test]
    fn format_value_builtin_direct() {
        let output = run_source(
            r#"
            print(format_value(42, "08d"))
        "#,
        );
        assert_eq!(output, vec!["00000042"]);
    }

    #[test]
    fn fstring_format_default_int_align() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x:10d}")
        "#,
        );
        assert_eq!(output, vec!["        42"]);
    }

    #[test]
    fn fstring_format_default_str_align() {
        let output = run_source(
            r#"
            let s = "hi"
            print(f"{s:10}")
        "#,
        );
        assert_eq!(output, vec!["hi        "]);
    }

    #[test]
    fn fstring_format_negative_with_plus() {
        let output = run_source(
            r#"
            let x = -42
            print(f"{x:+d}")
        "#,
        );
        assert_eq!(output, vec!["-42"]);
    }

    #[test]
    fn fstring_format_large_width() {
        let output = run_source(
            r#"
            let x = 1
            print(f"{x:50d}")
        "#,
        );
        let expected = format!("{:>50}", "1");
        assert_eq!(output, vec![expected]);
    }

    #[test]
    fn fstring_format_zero_pad_no_width() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x:0d}")
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn fstring_format_int_width_no_type() {
        let output = run_source(
            r#"
            let x = 42
            print(f"{x:10}")
        "#,
        );
        assert_eq!(output, vec!["        42"]);
    }

    #[test]
    fn fstring_format_sci_upper() {
        let output = run_source(
            r#"
            let x = 12345.6789
            print(f"{x:.2E}")
        "#,
        );
        assert_eq!(output, vec!["1.23E+04"]);
    }

    #[test]
    fn fstring_format_sci_negative_exp() {
        let output = run_source(
            r#"
            let x = 0.001
            print(f"{x:.2e}")
        "#,
        );
        assert_eq!(output, vec!["1.00e-03"]);
    }

    #[test]
    fn anonymous_entity_literal_basic() {
        let output = run_source(
            r#"
            component Health { hp: int = 0 }
            fn main() -> nil {
                let e = entity { Health { hp: 42 } }
                let h = require(e, Health)
                print(h.hp)
            }
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn anonymous_entity_literal_multiple_components() {
        let output = run_source(
            r#"
            component Health { hp: int = 0 }
            component Position { x: int = 0, y: int = 0 }
            fn main() -> nil {
                let e = entity { Health { hp: 10 }, Position { x: 3, y: 4 } }
                let h = require(e, Health)
                let p = require(e, Position)
                print(h.hp)
                print(p.x)
                print(p.y)
            }
        "#,
        );
        assert_eq!(output, vec!["10", "3", "4"]);
    }

    #[test]
    fn anonymous_entity_literal_as_argument() {
        let output = run_source(
            r#"
            component Tag { name: str = "" }
            fn show_tag(e: entity) -> nil {
                let t = require(e, Tag)
                print(t.name)
            }
            fn main() -> nil {
                show_tag(entity { Tag { name: "hello" } })
            }
        "#,
        );
        assert_eq!(output, vec!["hello"]);
    }

    #[test]
    fn anonymous_entity_literal_as_return_value() {
        let output = run_source(
            r#"
            component Label { text: str = "" }
            fn make_label(t: str) -> entity {
                return entity { Label { text: t } }
            }
            fn main() -> nil {
                let e = make_label("world")
                print(require(e, Label).text)
            }
        "#,
        );
        assert_eq!(output, vec!["world"]);
    }

    #[test]
    fn anonymous_entity_literal_nested() {
        let output = run_source(
            r#"
            component Inner { val: int = 0 }
            component Outer { child: entity = spawn() }
            fn main() -> nil {
                let e = entity {
                    Outer { child: entity { Inner { val: 99 } } }
                }
                let o = require(e, Outer)
                let i = require(o.child, Inner)
                print(i.val)
            }
        "#,
        );
        assert_eq!(output, vec!["99"]);
    }

    #[test]
    fn named_entity_still_works_after_anonymous_feature() {
        let output = run_source(
            r#"
            component Health { hp: int = 0 }
            entity player { Health { hp: 100 } }
            fn main() -> nil {
                print(require(player, Health).hp)
            }
        "#,
        );
        assert_eq!(output, vec!["100"]);
    }

    #[test]
    fn named_entity_literal_with_string_name() {
        let output = run_source(
            r#"
            component Health { hp: int = 0 }
            fn main() -> nil {
                let e = entity "hero" { Health { hp: 42 } }
                let found = get_entity("hero")
                print(require(found, Health).hp)
            }
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn named_entity_literal_with_variable_name() {
        let output = run_source(
            r#"
            component Tag { label: str = "" }
            fn make(name: str) -> entity {
                return entity name { Tag { label: "ok" } }
            }
            fn main() -> nil {
                let e = make("dynamic")
                let found = get_entity("dynamic")
                print(require(found, Tag).label)
            }
        "#,
        );
        assert_eq!(output, vec!["ok"]);
    }

    #[test]
    fn named_entity_literal_anonymous_still_works() {
        let output = run_source(
            r#"
            component Val { x: int = 0 }
            fn main() -> nil {
                let e = entity { Val { x: 7 } }
                print(require(e, Val).x)
            }
        "#,
        );
        assert_eq!(output, vec!["7"]);
    }

    #[test]
    fn named_entity_literal_variable_empty_body() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let name = "empty_one"
                let e = entity name {}
                let found = get_entity("empty_one")
                print(found == e)
            }
        "#,
        );
        assert_eq!(output, vec!["true"]);
    }

    #[test]
    fn entity_literal_with_variable_component_entry() {
        let output = run_source(
            r#"
            component Val { x: int = 0 }
            fn main() -> nil {
                let v = Val { x: 42 }
                let e = entity { v }
                print(require(e, Val).x)
            }
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn entity_literal_with_call_component_entry() {
        let output = run_source(
            r#"
            component Val { x: int = 0 }
            fn make_val() -> Val {
                return Val { x: 99 }
            }
            fn main() -> nil {
                let e = entity { make_val() }
                print(require(e, Val).x)
            }
        "#,
        );
        assert_eq!(output, vec!["99"]);
    }

    #[test]
    fn entity_literal_mixed_init_and_expr() {
        let output = run_source(
            r#"
            component Health { hp: int = 0 }
            component Tag { name: str = "" }
            fn main() -> nil {
                let t = Tag { name: "hero" }
                let e = entity { Health { hp: 100 }, t }
                print(require(e, Health).hp)
                print(require(e, Tag).name)
            }
        "#,
        );
        assert_eq!(output, vec!["100", "hero"]);
    }

    #[test]
    fn closure_destructure_works_in_filter_and_map() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [["a", 2], ["b", 1], ["c", 2]]
                let names = rows
                    |> filter(fn([name, phase]) { return phase == 2 })
                    |> map(fn([name, phase]) { return name })
                print(len(names))
                print(names[0])
                print(names[1])
            }
        "#,
        );
        assert_eq!(output, vec!["2", "a", "c"]);
    }

    #[test]
    fn closure_destructure_works_with_mixed_params() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [["x", 2], ["y", 3]]
                let total = rows |> reduce(0, fn(acc, [name, phase]) { return acc + phase })
                print(total)
            }
        "#,
        );
        assert_eq!(output, vec!["5"]);
    }

    #[test]
    fn for_loop_destructure_unpacks_each_row() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [[1, 2], [3, 4]]
                let mut sum = 0
                for [a, b] in rows {
                    sum = sum + a + b
                }
                print(sum)
            }
        "#,
        );
        assert_eq!(output, vec!["10"]);
    }

    #[test]
    fn closure_destructure_arity_mismatch_fails_loudly() {
        let err = run_source_result(
            r#"
            fn main() -> nil {
                let rows = [[1, 2]]
                let _ = rows |> map(fn([a, b, c]) { return a + b + c })
            }
        "#,
        )
        .expect_err("expected destructure arity mismatch to fail");
        assert!(
            err.contains("out of bounds"),
            "expected out-of-bounds error, got: {}",
            err
        );
    }

    #[test]
    fn closure_single_element_destructure() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [[10], [20], [30]]
                let out = rows |> map(fn([x]) { return x * 2 })
                print(out[0])
                print(out[1])
                print(out[2])
            }
        "#,
        );
        assert_eq!(output, vec!["20", "40", "60"]);
    }

    #[test]
    fn for_loop_destructure_with_index_value_pairs() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let pairs = [[0, "a"], [1, "b"], [2, "c"]]
                for [idx, val] in pairs {
                    print(f"{idx}:{val}")
                }
            }
        "#,
        );
        assert_eq!(output, vec!["0:a", "1:b", "2:c"]);
    }

    #[test]
    fn for_loop_destructure_tuple_rows() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [(1, 2), (3, 4), (5, 6)]
                let mut sum = 0
                for [a, b] in rows {
                    sum = sum + a + b
                }
                print(sum)
            }
        "#,
        );
        assert_eq!(output, vec!["21"]);
    }

    #[test]
    fn closure_both_params_destructured_in_reduce() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [[1, 2], [3, 4], [5, 6]]
                let out = rows |> reduce([0, 0], fn([sa, sb], [a, b]) {
                    return [sa + a, sb + b]
                })
                print(out[0])
                print(out[1])
            }
        "#,
        );
        assert_eq!(output, vec!["9", "12"]);
    }

    #[test]
    fn closure_three_params_all_destructured() {
        let output = run_source(
            r#"
            fn apply3(f, a, b, c) { return f(a, b, c) }
            fn main() -> nil {
                let r = apply3(
                    fn([x1, x2], [y1, y2], [z1, z2]) {
                        return x1 + y1 + z1 + x2 + y2 + z2
                    },
                    [1, 2], [3, 4], [5, 6]
                )
                print(r)
            }
        "#,
        );
        assert_eq!(output, vec!["21"]);
    }

    #[test]
    fn closure_underscore_discard_in_destructure() {
        let output = run_source(
            r#"
            fn main() -> nil {
                let rows = [[1, 2, 3], [4, 5, 6]]
                let mids = rows |> map(fn([_, mid, _]) { return mid })
                print(mids[0])
                print(mids[1])
            }
        "#,
        );
        assert_eq!(output, vec!["2", "5"]);
    }

    // ================================================================
    // BUG FIX VERIFICATION TESTS
    // ================================================================

    // BUG 1: Integer overflow now errors instead of silently wrapping
    #[test]
    fn bug1_integer_overflow_add() {
        let result = run_source_result(
            r#"
            let x = 9223372036854775807
            let y = x + 1
            print(y)
        "#,
        );
        assert!(result.is_err(), "overflow on add should error");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug1_integer_overflow_sub() {
        let result = run_source_result(
            r#"
            let x = -9223372036854775807
            let y = x - 2
            print(y)
        "#,
        );
        assert!(result.is_err(), "overflow on sub should error");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug1_integer_overflow_mul() {
        let result = run_source_result(
            r#"
            let x = 9223372036854775807
            let y = x * 2
            print(y)
        "#,
        );
        assert!(result.is_err(), "overflow on mul should error");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug1_normal_arithmetic_still_works() {
        let output = run_source(
            r#"
            print(100 + 200)
            print(500 - 100)
            print(10 * 20)
            print(-50 + 50)
        "#,
        );
        assert_eq!(output, vec!["300", "400", "200", "0"]);
    }

    // BUG 2: Double despawn now errors
    #[test]
    fn bug2_double_despawn_errors() {
        let result = run_source_result(
            r#"
            component HP { val: 100 }
            fn main() -> nil {
                let e = spawn(HP { val: 42 })
                despawn(e)
                despawn(e)
            }
        "#,
        );
        assert!(result.is_err(), "double despawn should error");
        assert!(result.unwrap_err().contains("non-existent entity"));
    }

    #[test]
    fn bug2_single_despawn_still_works() {
        let output = run_source(
            r#"
            component HP { val: 100 }
            fn main() -> nil {
                let e = spawn(HP { val: 42 })
                print("before")
                despawn(e)
                print("after")
            }
        "#,
        );
        assert_eq!(output, vec!["before", "after"]);
    }

    // BUG 3: set() / update() on despawned entity now errors
    #[test]
    fn bug3_set_on_despawned_entity_errors() {
        let result = run_source_result(
            r#"
            component HP { val: 100 }
            fn main() -> nil {
                let e = spawn(HP { val: 42 })
                despawn(e)
                set(e, HP { val: 999 })
            }
        "#,
        );
        assert!(result.is_err(), "set on despawned entity should error");
        assert!(result.unwrap_err().contains("non-existent entity"));
    }

    // BUG 4: -0.0 == 0.0 now works correctly (IEEE 754)
    #[test]
    fn bug4_negative_zero_equals_positive_zero() {
        let output = run_source(
            r#"
            let a = -0.0
            let b = 0.0
            if a == b {
                print("equal")
            } else {
                print("not equal")
            }
        "#,
        );
        assert_eq!(output, vec!["equal"]);
    }

    #[test]
    fn bug4_negative_zero_arithmetic() {
        let output = run_source(
            r#"
            let a = -0.0
            let b = 0.0
            let c = a + b
            if c == 0.0 {
                print("sum is zero")
            }
            if a == b {
                print("neg zero equals zero")
            }
            let d = -1.0 * 0.0
            if d == 0.0 {
                print("minus one times zero equals zero")
            }
        "#,
        );
        assert_eq!(
            output,
            vec![
                "sum is zero",
                "neg zero equals zero",
                "minus one times zero equals zero"
            ]
        );
    }

    #[test]
    fn bug4_normal_float_equality_still_works() {
        let output = run_source(
            r#"
            let a = 3.14
            let b = 3.14
            if a == b {
                print("equal")
            } else {
                print("not equal")
            }
        "#,
        );
        assert_eq!(output, vec!["equal"]);
    }

    // BUG 6: ? propagation from main() now errors
    #[test]
    fn bug6_question_mark_err_from_main_errors() {
        let result = run_source_result(
            r#"
            type Result {
                Ok { value: int }
                Err { value: str }
            }
            fn failing() -> Result {
                return Result::Err { value: "something broke" }
            }
            fn main() -> Result {
                let x = failing()?
                return Result::Ok { value: x }
            }
        "#,
        );
        assert!(
            result.is_err(),
            "Err propagated from main via ? should error"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("Unhandled error")
                || err.contains("something broke")
                || err.contains("propagated"),
            "Error message should mention the unhandled error: {}",
            err
        );
    }

    #[test]
    fn bug6_question_mark_ok_from_main_succeeds() {
        let output = run_source(
            r#"
            type Result {
                Ok { value: int }
                Err { value: str }
            }
            fn succeeding() -> Result {
                return Result::Ok { value: 42 }
            }
            fn main() -> Result {
                let x = succeeding()?
                print(x)
                return Result::Ok { value: x }
            }
        "#,
        );
        assert_eq!(output, vec!["42"]);
    }

    // ================================================================
    // BUG FIX ROUND 2 — VERIFICATION TESTS
    // ================================================================

    #[test]
    fn bug7_int_min_div_neg1_errors() {
        let result = run_source_result(
            r#"
            let x = -9223372036854775807 - 1
            let y = x / -1
            print(y)
        "#,
        );
        assert!(result.is_err(), "INT_MIN / -1 should error, not panic");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug7b_int_min_mod_neg1_errors() {
        let result = run_source_result(
            r#"
            let x = -9223372036854775807 - 1
            let y = x % -1
            print(y)
        "#,
        );
        assert!(result.is_err(), "INT_MIN % -1 should error, not panic");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug7_normal_div_still_works() {
        let output = run_source(
            r#"
            print(10 / 3)
            print(-10 / 3)
            print(100 % 7)
        "#,
        );
        assert_eq!(output, vec!["3", "-3", "2"]);
    }

    #[test]
    fn bug8_negate_int_min_errors() {
        let result = run_source_result(
            r#"
            let x = -9223372036854775807 - 1
            let y = -x
            print(y)
        "#,
        );
        assert!(result.is_err(), "negating INT_MIN should error");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug8_normal_negation_works() {
        let output = run_source(
            r#"
            print(-42)
            print(-(-100))
            print(-(0))
        "#,
        );
        assert_eq!(output, vec!["-42", "100", "0"]);
    }

    #[test]
    fn bug9_int_of_huge_float_errors() {
        let result = run_source_result(
            r#"
            let x = 1.0e19
            let y = int(x)
            print(y)
        "#,
        );
        assert!(
            result.is_err(),
            "int(1e19) should error, not silently saturate"
        );
        assert!(result.unwrap_err().contains("out of i64 range"));
    }

    #[test]
    fn bug9_int_of_normal_float_works() {
        let output = run_source(
            r#"
            print(int(3.14))
            print(int(-2.7))
            print(int(42.0))
        "#,
        );
        assert_eq!(output, vec!["3", "-2", "42"]);
    }

    #[test]
    fn bug10_int_div_builtin_min_neg1_errors() {
        let result = run_source_result(
            r#"
            let x = -9223372036854775807 - 1
            let y = int_div(x, -1)
            print(y)
        "#,
        );
        assert!(result.is_err(), "int_div(INT_MIN, -1) should error");
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug11_abs_int_min_errors() {
        let result = run_source_result(
            r#"
            let x = -9223372036854775807 - 1
            let y = abs(x)
            print(y)
        "#,
        );
        assert!(
            result.is_err(),
            "abs(INT_MIN) should error, not return negative"
        );
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn bug11_normal_abs_works() {
        let output = run_source(
            r#"
            print(abs(-42))
            print(abs(100))
            print(abs(0))
            print(abs(-3.14))
        "#,
        );
        assert_eq!(output, vec!["42", "100", "0", "3.14"]);
    }

    #[test]
    fn math_round_floor_ceil() {
        let output = run_source(
            r#"
            print(round(2.5))
            print(round(-2.5))
            print(round(2.4))
            print(round(7))
            print(floor(-1.2))
            print(floor(1.8))
            print(ceil(-1.2))
            print(ceil(1.2))
        "#,
        );
        assert_eq!(output, vec!["3", "-3", "2", "7", "-2", "1", "-1", "2"]);
    }

    #[test]
    fn math_sqrt_pow() {
        let output = run_source(
            r#"
            print(sqrt(144.0))
            print(sqrt(2))
            print(pow(2, 10))
            print(pow(2.0, -1.0))
        "#,
        );
        assert_eq!(output, vec!["12.0", "1.4142135623730951", "1024", "0.5"]);
    }

    #[test]
    fn math_sqrt_negative_errors() {
        let result = run_source_result("print(sqrt(-1.0))");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("sqrt() of negative number"));
    }

    #[test]
    fn math_pow_int_overflow_errors() {
        let result = run_source_result("print(pow(10, 200))");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Integer overflow"));
    }

    #[test]
    fn to_fixed_formats_negatives_correctly() {
        let output = run_source(
            r#"
            print(to_fixed(-143.90, 2))
            print(to_fixed(3.14159, 3))
            print(to_fixed(2.0, 0))
            print(to_fixed(5, 2))
        "#,
        );
        assert_eq!(output, vec!["-143.90", "3.142", "2", "5.00"]);
    }

    #[test]
    fn json_round_trip_struct_and_collections() {
        let output = run_source(
            r#"
            struct Task { id: int = 0, text: str = "", done: bool = false }
            let t = Task { id: 7, text: "buy milk", done: true }
            let s = json_stringify(t)
            print(s)
            let back = json_parse(s) |> unwrap
            print(back["id"])
            print(back["text"])
            print(back["done"])
            let nested = {"a": [1, 2], "b": [3]}
            let n2 = json_parse(json_stringify(nested)) |> unwrap
            print(n2["a"][1])
            print(n2["b"][0])
        "#,
        );
        assert_eq!(
            output,
            vec![
                "{\"done\":true,\"id\":7,\"text\":\"buy milk\"}",
                "7",
                "buy milk",
                "true",
                "2",
                "3"
            ]
        );
    }

    #[test]
    fn json_parse_invalid_returns_none() {
        let output = run_source(
            r#"
            print(is_none(json_parse("{nope")))
            print(is_none(json_parse("null")))
            print(json_parse("[1, 2.5, \"x\", null, true]") |> unwrap)
        "#,
        );
        assert_eq!(output, vec!["true", "false", "[1, 2.5, \"x\", nil, true]"]);
    }

    #[test]
    fn json_stringify_rejects_non_finite() {
        let result = run_source_result(
            r#"
            let huge = pow(10.0, 400.0)
            print(json_stringify(huge))
        "#,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("non-finite"));
    }
}

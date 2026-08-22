#[test]
fn maintained_views_ordered_indexes_and_negative_provenance_stay_coherent() {
    let output = run_source(
        r#"
            opaque type ManagerGid = u32

            component Live {}
            component Grave {}
            component Identity {
                ordered indexed manager: ManagerGid = ManagerGid(u32(0)),
            }

            materialized view LiveByManager {
                depends [Live, Identity]
                key Identity.manager
            }

            transaction Retire(target: entity) {
                requires has(target, Live)
                changes_only [Live, Grave]
                remove(target, Live)
                set(target, Grave {})
                ensures not has(target, Live)
                ensures has(target, Grave)
            }

            let low = spawn(Live {}, Identity { manager: ManagerGid(u32(10)) })
            let duplicate_a = spawn(Live {}, Identity { manager: ManagerGid(u32(20)) })
            let duplicate_b = spawn(Live {}, Identity { manager: ManagerGid(u32(20)) })
            let high = spawn(Live {}, Identity { manager: ManagerGid(u32(30)) })

            assert(first(Identity, "manager") == Some(low), "first is ascending")
            assert(last(Identity, "manager") == Some(high), "last is ascending")
            assert(lower_bound(Identity, "manager", ManagerGid(u32(20))) == Some(duplicate_a), "duplicate keys choose lowest entity id")
            assert(upper_bound(Identity, "manager", ManagerGid(u32(20))) == Some(high), "upper bound is strict")
            assert(previous(Identity, "manager", ManagerGid(u32(20))) == Some(low), "previous is strict")
            let duplicates = range(Identity, "manager", ManagerGid(u32(20)), ManagerGid(u32(30)))
            assert(duplicates == [duplicate_a, duplicate_b], "range is lower-inclusive and upper-exclusive")

            let before = revision(LiveByManager)
            assert(lookup(LiveByManager, ManagerGid(u32(10))) == Some(low), "view key lookup")
            print(why_in_view(LiveByManager, low))
            Retire(low)
            assert(revision(LiveByManager) > before, "view revision advances in the transaction")
            assert(len(changes_since(LiveByManager, before)) == 1, "one membership delta")
            print(why_removed(low, Live))
            print(why_not_in_view(LiveByManager, low))
            print(why_revision_changed(LiveByManager))

            remove(low, Identity)
            set(duplicate_a, Identity { manager: ManagerGid(u32(25)) })
            assert(first(Identity, "manager") == Some(duplicate_b), "replacement removes the prior ordered key")
            remove(high, Identity)
            remove(high, Identity)
            assert(last(Identity, "manager") == Some(duplicate_a), "erase-miss is stable")
            "#,
    );

    assert_eq!(output.len(), 4, "unexpected output: {output:?}");
    assert!(output[0].contains("is in LiveByManager"), "{}", output[0]);
    assert!(output[1].contains("transaction Retire"), "{}", output[1]);
    assert!(output[2].contains("missing Live"), "{}", output[2]);
    assert!(output[3].contains("revision"), "{}", output[3]);
}

#[test]
fn materialized_view_predicates_update_incrementally_and_explain_exclusion() {
    let output = run_source(
        r#"
            component Product { id: int = 0, active: bool = false, description: str = "" }
            component Inventory { available: int = 0 }
            component Seller { active: bool = false }

            materialized view SellableProducts {
                depends [Product, Inventory, Seller]
                key Product.id
                where Product.active == true
                    and Inventory.available > 0
                    and Seller.active == true
            }

            let product = spawn(
                Product { id: 42, active: true, description: "first" },
                Inventory { available: 3 },
                Seller { active: true }
            )
            assert(lookup(SellableProducts, 42) == Some(product), "eligible product enters")
            let stable = revision(SellableProducts)
            set(product, Product { id: 42, active: true, description: "renamed" })
            assert(revision(SellableProducts) == stable, "membership-neutral write keeps revision")

            set(product, Inventory { available: 0 })
            assert(lookup(SellableProducts, 42) == None, "predicate removal is immediate")
            print(why_not_in_view(SellableProducts, product))
            let removed = revision(SellableProducts)
            set(product, Inventory { available: 4 })
            assert(revision(SellableProducts) == removed + 1, "predicate recovery enters once")
            assert(lookup(SellableProducts, 42) == Some(product), "product re-enters")
            "#,
    );
    assert_eq!(output.len(), 1, "unexpected output: {output:?}");
    assert!(
        output[0].contains("Inventory.available > 0") && output[0].contains("current 0"),
        "{}",
        output[0]
    );
}

#[test]
fn field_writes_refresh_only_exact_view_key_and_predicate_dependencies() {
    let source = r#"
            component Product {
                id: int = 0,
                active: bool = false,
                description: str = "",
            }

            materialized view SellableProducts {
                depends [Product]
                key Product.id
                where Product.active == true
            }

            transaction Deactivate(product: entity) {
                requires has(product, Product)
                changes_only [Product]
                write_field(product, Product, "active", false)
                ensures read_field(product, Product, "active") == false
            }

            let product = spawn(Product { id: 1, active: true, description: "first" })
            let admitted = revision(SellableProducts)
            write_field(product, Product, "description", "renamed")
            assert(revision(SellableProducts) == admitted, "unrelated field is not invalidating")
            write_field(product, Product, "id", 2)
            assert(lookup(SellableProducts, 1) == None, "old key removed")
            assert(lookup(SellableProducts, 2) == Some(product), "new key published")
            Deactivate(product)
            assert(lookup(SellableProducts, 2) == None, "predicate field removes member")
            print(why_field(product, Product, "description"))
        "#;

    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize().0;
    let program = Parser::new(tokens).parse();
    let result = Compiler::new().compile(&program).unwrap();
    let mut vm = VM::new();
    vm.load_compile_result(result);
    vm.run(0).unwrap();

    assert_eq!(
        vm.get_world()
            .materialized_view_evaluation_count("SellableProducts"),
        Some(3),
        "spawn, key write, and predicate write each evaluate once; description does not"
    );
    assert_eq!(vm.print_buffer.len(), 1);
    assert!(
        vm.print_buffer[0].contains("description") && vm.print_buffer[0].contains("renamed"),
        "{}",
        vm.print_buffer[0]
    );
}

#[test]
fn fork_wire_roundtrip_preserves_materialized_view_history_and_future_maintenance() {
    let output = run_source(
        r#"
            component Product { id: int = 0 }
            component Inventory { available: int = 0 }

            materialized view SellableProducts {
                depends [Product, Inventory]
                key Product.id
                where Inventory.available > 0
            }

            let product = spawn(Product { id: 42 }, Inventory { available: 3 })
            let admitted = revision(SellableProducts)
            set(product, Inventory { available: 0 })
            let removed = revision(SellableProducts)
            assert(removed == admitted + 1, "source mutation removes exactly once")
            let before_wire = why_not_in_view(SellableProducts, product)

            let restored = fork_from_bytes(fork_to_bytes(fork())) |> unwrap
            commit(restored)

            assert(revision(SellableProducts) == removed, "wire keeps exact revision")
            assert(len(changes_since(SellableProducts, admitted)) == 1, "wire keeps change history")
            assert(why_not_in_view(SellableProducts, product) == before_wire, "wire keeps explanation")
            assert(lookup(SellableProducts, 42) == None, "wire keeps exclusion")

            set(product, Inventory { available: 5 })
            assert(revision(SellableProducts) == removed + 1, "restored view remains incremental")
            assert(lookup(SellableProducts, 42) == Some(product), "restored view republishes")
            print(why_revision_changed(SellableProducts))
            "#,
    );
    assert_eq!(output.len(), 1, "unexpected output: {output:?}");
    assert!(
        output[0].contains("entered") && output[0].contains("revision"),
        "{}",
        output[0]
    );
}

#[test]
fn world_save_load_reinstalls_compiled_materialized_views() {
    let output = run_source(
        r#"
            component Product { id: int = 0 }
            component Inventory { available: int = 0 }
            materialized view SellableProducts {
                depends [Product, Inventory]
                key Product.id
                where Inventory.available > 0
            }

            let product = spawn(Product { id: 42 }, Inventory { available: 0 })
            let saved = save_world()
            load_world(saved)
            assert(lookup(SellableProducts, 42) == None, "loaded exclusion is derived")
            print(why_not_in_view(SellableProducts, product))
            set(product, Inventory { available: 3 })
            assert(lookup(SellableProducts, 42) == Some(product), "loaded view stays incremental")
            "#,
    );
    assert_eq!(output.len(), 1, "unexpected output: {output:?}");
    assert!(output[0].contains("Inventory.available > 0"), "{}", output[0]);
}

#[test]
fn ordered_composite_keys_are_lexicographic_and_native_signed_order_is_numeric() {
    let output = run_source(
        r#"
            opaque type PriceTicks = i64
            opaque type Sequence = u64

            component Order {
                ordered indexed priority: (PriceTicks, Sequence) = (PriceTicks(i64(0)), Sequence(u64(0))),
                ordered indexed signed_price: PriceTicks = PriceTicks(i64(0)),
            }

            let negative = spawn(Order {
                priority: (PriceTicks(i64(-10)), Sequence(u64(2))),
                signed_price: PriceTicks(i64(-10)),
            })
            let earlier = spawn(Order {
                priority: (PriceTicks(i64(5)), Sequence(u64(1))),
                signed_price: PriceTicks(i64(5)),
            })
            let later = spawn(Order {
                priority: (PriceTicks(i64(5)), Sequence(u64(2))),
                signed_price: PriceTicks(i64(6)),
            })

            assert(PriceTicks(i64(-10)) < PriceTicks(i64(5)), "native signed comparison is numeric")
            assert(first(Order, "signed_price") == Some(negative), "signed native order is numeric")
            assert(first(Order, "priority") == Some(negative), "tuple first element leads")
            assert(lower_bound(Order, "priority", (PriceTicks(i64(5)), Sequence(u64(0)))) == Some(earlier), "composite lower bound")
            assert(next(Order, "priority", (PriceTicks(i64(5)), Sequence(u64(1)))) == Some(later), "sequence breaks equal-price ties")
            assert(range(Order, "priority", (PriceTicks(i64(5)), Sequence(u64(0))), (PriceTicks(i64(6)), Sequence(u64(0)))) == [earlier, later], "lexicographic bounded range")
            print("ordered composite ok")
            "#,
    );
    assert_eq!(output, ["ordered composite ok"]);
}

#[test]
fn allocation_free_view_visit_is_stable_and_rejects_membership_invalidation() {
    let output = run_source(
        r#"
            component Active {}
            component Position { x: int = 0 }
            materialized view ActiveTrips { depends [Active, Position] }

            fn advance(target: entity) -> nil {
                let position: Position = require(target, Position)
                set(target, Position { x: position.x + 1 })
            }
            let first_trip = spawn(Active {}, Position { x: 1 })
            let second_trip = spawn(Active {}, Position { x: 4 })
            visit_view(ActiveTrips, advance)
            assert(require(first_trip, Position).x == 2, "first visited once")
            assert(require(second_trip, Position).x == 5, "second visited once")
            print("stable visit ok")
            "#,
    );
    assert_eq!(output, ["stable visit ok"]);

    let error = run_source_result(
        r#"
            component Active {}
            materialized view ActiveTrips { depends [Active] }
            fn invalidate(target: entity) -> nil { remove(target, Active) }
            spawn(Active {})
            spawn(Active {})
            visit_view(ActiveTrips, invalidate)
            "#,
    )
    .expect_err("membership-changing callback must invalidate traversal");
    assert!(error.contains("was invalidated by its callback"), "{error}");
}

#[test]
fn event_delivery_modes_and_lifecycle_barriers_are_explicit() {
    let output = run_source(
        r#"
            resource Counter { value: int = 0 }
            event Pulse { amount: int }

            on Pulse(evt) {
                let current = res(Counter)
                set_resource(Counter, Counter { value: current.value + evt.amount })
            }

            signal sync Pulse { amount: 1 }
            print(res(Counter).value)
            emit next Pulse { amount: 2 }
            print(res(Counter).value)
            flush_events()
            print(res(Counter).value)
            emit phase(Ready) Pulse { amount: 4 }
            flush_events()
            print(res(Counter).value)
            enter_phase("Ready")
            print(res(Counter).value)
            "#,
    );
    assert_eq!(output, vec!["1", "1", "3", "3", "7"]);
}

#[test]
fn non_reentrant_and_completion_barriers_fail_closed() {
    let recursive = run_source_result(
        r#"
            event Pulse {}
            @non_reentrant
            on Pulse(evt) { signal sync Pulse {} }
            signal sync Pulse {}
            "#,
    )
    .unwrap_err();
    assert!(recursive.contains("Non-reentrant handler"), "{recursive}");

    let late = run_source_result(
        r#"
            event Pulse {}
            @must_complete_before(Ready)
            on Pulse(evt) {}
            enter_phase("Ready")
            signal sync Pulse {}
            "#,
    )
    .unwrap_err();
    assert!(
        late.contains("must complete before lifecycle phase 'Ready'"),
        "{late}"
    );
}

#[test]
fn completed_phase_and_exactly_once_delivery_fail_closed() {
    let late_phase_emit = run_source_result(
        r#"
            event Pulse {}
            enter_phase("Ready")
            emit phase(Ready) Pulse {}
            "#,
    )
    .unwrap_err();
    assert!(
            late_phase_emit.contains(
                "Cannot emit event 'Pulse' to lifecycle phase 'Ready': that phase has already completed"
            ),
            "{late_phase_emit}"
        );

    let duplicate = run_source_result(
        r#"
            event SnapshotReady {}
            @exactly_once
            on SnapshotReady(evt) {}
            signal sync SnapshotReady {}
            signal sync SnapshotReady {}
            "#,
    )
    .unwrap_err();
    assert!(
        duplicate
            .contains("Exactly-once handler for 'SnapshotReady' received a duplicate delivery"),
        "{duplicate}"
    );
}

#[test]
fn stateful_models_are_deterministic_and_shrink_failures() {
    let run_model = |source: &str| {
        let tokens = Lexer::new(source).tokenize().0;
        let program = Parser::new(tokens).parse();
        let result = Compiler::new().compile(&program).unwrap();
        let mut vm = VM::new();
        vm.suppress_output();
        vm.load_compile_result(result);
        vm.run(0).unwrap();
        crate::test_runner::run_tests(&mut vm)
    };
    let success = run_model(
        r#"
            component Flag { active: bool = false }
            let subject = spawn(Flag { active: false })
            fn toggle() -> nil {
                let current = get(subject, Flag) |> unwrap
                set(subject, Flag { active: not current.active })
            }
            model Toggle {
                commands [toggle]
                invariant { return len(query { Flag }) == 1 }
                runs 1
                max_commands 16
                seed 99
            }
            "#,
    );
    assert_eq!(success.len(), 1);
    assert!(success[0].error.is_none(), "{:?}", success[0]);

    let event_observation = run_model(
        r#"
            component Pending {}
            component Committed {}
            event CommitObserved { subject: entity }
            let subject = spawn(Pending {})
            fn commit_once() -> nil {
                if has(subject, Pending) {
                    remove(subject, Pending)
                    set(subject, Committed {})
                    emit next CommitObserved { subject: subject }
                    flush_events()
                }
            }
            model EventTemporalObservation {
                commands [commit_once]
                invariant { return has(subject, Pending) != has(subject, Committed) }
                temporal {
                    eventually Committed,
                    exactly_once CommitObserved,
                }
                runs 2
                max_commands 4
                seed 123
            }
            "#,
    );
    assert_eq!(event_observation.len(), 1);
    assert!(
        event_observation[0].error.is_none(),
        "private model trials must observe their own event history: {:?}",
        event_observation[0]
    );

    let failure = run_model(
        r#"
            component Broken { count: int = 0 }
            let subject = spawn(Broken { count: 0 })
            fn break_invariant() -> nil {
                set(subject, Broken { count: 1 })
            }
            model BrokenModel {
                commands [break_invariant]
                invariant { return (get(subject, Broken) |> unwrap).count == 0 }
                runs 1
                max_commands 32
                seed 7
            }
            "#,
    )[0]
    .error
    .clone()
    .expect("broken model must fail");
    assert!(failure.contains("RAD_MODEL_FAILURE="), "{failure}");
    assert!(failure.contains("\"minimal_trace\":[{"), "{failure}");
    assert!(
        failure.contains("\"label\":\"break_invariant\""),
        "{failure}"
    );

    let preserved = run_model(
        r#"
            component Seen {}
            let subject = spawn()
            fn fail_before_observation() -> nil {
                assert(false, "original command failure")
                set(subject, Seen {})
            }
            model PreserveFailureIdentity {
                commands [fail_before_observation]
                invariant { return true }
                temporal { exactly_once Seen }
                runs 1
                max_commands 8
                seed 11
            }
        "#,
    )[0]
    .error
    .clone()
    .expect("command failure must be retained");
    assert!(preserved.contains("command 'fail_before_observation' failed"));
    assert!(preserved.contains("shrinking 8 generated command(s) to 1"));
    assert!(!preserved.contains("to 0"), "{preserved}");
}

#[test]
fn caught_calls_unwind_failed_nested_frames_before_the_caller_continues() {
    let output = run_source(
        r#"
            fn decode(_input: str) -> int {
                assert(false, "malformed host payload")
                return unwrap(None)
            }

            match host_try(decode, "payload") {
                Err(message) => assert(contains(message, "malformed host payload"), "original failure retained")
                Ok(_) => assert(false, "failed nested call unexpectedly succeeded")
            }
            print("continued")
        "#,
    );
    assert_eq!(output, vec!["continued"]);
}

#[test]
fn instruction_budget_is_enforced_at_runtime() {
    let error = run_source_result(
        r#"
            component Root {}
            fn work(value: int) -> int { return value + 1 }
            @budget(instructions: 1)
            system Limited(root: Root) {
                let value = work(1)
                let other = work(value)
            }
            let _root = spawn(Root {})
            Limited()
            "#,
    )
    .unwrap_err();
    assert!(
        error.contains("exceeded its @budget(instructions: 1)"),
        "{error}"
    );
}

/// Run a fused `visit_view` kernel over `rows` entities and report the work the
/// system was charged, or the error a too-small budget produced.
fn fused_kernel_work(rows: usize, budget: u64) -> Result<u64, String> {
    let source = FUSED_KERNEL_FIXTURE
        .replace("ROW_COUNT", &rows.to_string())
        .replace("BUDGET", &budget.to_string());
    // Compiled through the checked path on purpose: `@budget(instructions:)`
    // is installed from the checker's system metadata, so a checker-less
    // compile silently produces a system with no budget — the contract under
    // test would then never be enforced and the test would pass vacuously.
    let compiled = crate::test_support::compile_checked_source(
        &source,
        crate::parser::ParserOptions,
        crate::checker::CheckerOptions::default(),
    )?;
    let mut vm = VM::new_with_seed(1);
    vm.enable_system_metrics();
    vm.load_compile_result(compiled);
    vm.run(0).map_err(|error| error.to_string())?;
    let metric = &vm.system_execution_metrics().expect("metrics enabled")["Move"];
    // A budget that never reached the runtime would make every enforcement
    // assertion below pass vacuously.
    assert_eq!(
        metric.instruction_budget,
        Some(budget),
        "the declared @budget did not reach the runtime"
    );
    Ok(metric.max_instructions)
}

const FUSED_KERNEL_FIXTURE: &str = r#"
    component Active { active: bool = false }
    component Position owned { x: float = 0.0 }
    component Velocity { dx: float = 0.0 }

    materialized view Movers {
        depends [Active, Position, Velocity]
        where Active.active == true
    }

    fn advance(target: entity) -> nil writes owned [Position] {
        let x = read_field(target, Position, "x")
        let dx = read_field(target, Velocity, "dx")
        write_field(target, Position, "x", x + dx)
    }

    @budget(instructions: BUDGET)
    system Move(
        reads Active,
        reads Position,
        reads Velocity,
        writes Position,
    ) {
        visit_view(Movers, advance)
    }

    fn seed(count: int) -> nil writes owned [Position] {
        for _i in range(0, count) {
            spawn(
                Active { active: true },
                Position { x: 0.0 },
                Velocity { dx: 1.0 },
            )
        }
    }

    seed(ROW_COUNT)
    Move()
"#;

/// `visit_view` compiles to one fused `RunViewKernel` opcode. The per-opcode
/// metric therefore used to report 4 instructions for a system that mutated
/// hundreds of entities, while the `@budget` contract was separately charged
/// per row — the enforced bound and the reported number disagreed, and the
/// reported number is what a reader uses to size the budget.
#[test]
fn fused_view_kernel_charges_work_proportional_to_rows_visited() {
    let small = fused_kernel_work(60, 1_000_000).expect("60 rows run");
    let medium = fused_kernel_work(120, 1_000_000).expect("120 rows run");
    let large = fused_kernel_work(600, 1_000_000).expect("600 rows run");

    assert!(
        large > medium && medium > small,
        "more rows must cost more work: 60 -> {small}, 120 -> {medium}, 600 -> {large}"
    );

    // Differencing cancels the fixed system-body overhead, so a constant
    // per-row charge shows up as one slope across both intervals. A kernel that
    // hid its traversal beneath a single opcode would show a slope of zero.
    let near_slope = (medium - small) / 60;
    let far_slope = (large - medium) / 480;
    assert!(near_slope > 0, "per-row charge must be non-zero");
    assert_eq!(
        near_slope, far_slope,
        "per-row charge must be constant: {near_slope} over 60..120 rows, {far_slope} over 120..600"
    );
}

/// A view that matches nothing has no rows to write. Resolving write addresses
/// before checking for rows made that an error ("no live storage") whenever a
/// system ran before anything had been spawned.
#[test]
fn fused_view_kernel_over_an_empty_view_is_a_no_op() {
    let empty = fused_kernel_work(0, 1_000_000).expect("an empty view must run cleanly");
    let populated = fused_kernel_work(60, 1_000_000).expect("60 rows run");
    assert!(
        populated > empty,
        "an empty traversal must cost less than a populated one: {empty} vs {populated}"
    );
}

/// A budget the fused traversal cannot afford must fail, rather than being
/// hidden beneath a single opcode.
#[test]
fn fused_view_kernel_cannot_outrun_its_instruction_budget() {
    const ROWS: usize = 600;
    // A budget smaller than the row count cannot cover a traversal that charges
    // at least one unit per row. Deriving the budget from `max_instructions`
    // instead looked tighter but coupled the test to the ratio between two
    // different counters: the reported metric also counts opcodes that never
    // charge fuel, so that ratio is not a contract and drifts.
    let error = fused_kernel_work(ROWS, (ROWS / 2) as u64)
        .expect_err("a budget below one unit per row must not be affordable");
    assert!(
        error.contains("@budget(instructions:"),
        "overrun must be attributed to the budget contract: {error}"
    );
    // ...and the same traversal completes when the budget is ample, so the
    // failure above is the budget and not the fixture.
    fused_kernel_work(ROWS, 1_000_000).expect("600 rows run unbudgeted");
}

#[test]
fn system_metrics_measure_instructions_and_guest_allocations_per_invocation() {
    let source = r#"
            @budget(instructions: 100)
            system Tick() {
                let value = 20 + 22
                assert(value == 42, "system executes")
            }
            Tick()
            Tick()
        "#;
    let mut lexer = Lexer::new(source);
    let program = Parser::new(lexer.tokenize().0).parse();
    let compiled = Compiler::new()
        .compile(&program)
        .expect("compile metric fixture");
    let mut vm = VM::new_with_seed(1);
    vm.enable_system_metrics();
    vm.load_compile_result(compiled);
    vm.run(0).expect("run metric fixture");
    let metric = &vm.system_execution_metrics().expect("metrics enabled")["Tick"];
    assert_eq!(metric.invocations, 2);
    assert!(metric.total_instructions > 0);
    assert!(metric.max_instructions <= 100);
    assert_eq!(metric.instruction_budget, Some(100));
    assert_eq!(metric.total_guest_allocations, 0);
    assert_eq!(metric.max_guest_allocations, 0);
    assert_eq!(metric.total_guest_allocated_bytes, 0);
    assert!(metric.native_allocation_meter_supported);
    assert_eq!(metric.total_managed_backing_allocations, 0);
    assert_eq!(metric.total_host_boundary_allocations, 0);
}

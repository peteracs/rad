#[cfg(test)]
mod tests {
    use super::{CausalityLedger, Cause, WriteKind, WriteRecord, WriteSummary};
    use crate::parser::ParserOptions;
    use crate::vm::VM;

    fn run(src: &str) -> VM {
        let result = crate::test_support::compile_source(src, ParserOptions)
            .expect("parse and compile");
        let mut vm = VM::new();
        vm.suppress_output();
        vm.load_compile_result(result);
        vm.run(0).expect("run");
        vm
    }

    #[test]
    fn top_level_write_explains_itself() {
        let vm = run(r#"
            component Health { hp: 100 }
            let hero = spawn("hero", Health { hp: 100 })
            set(hero, Health { hp: 50 })
            print(why(hero, Health))
        "#);
        let out = &vm.print_buffer[0];
        assert!(out.contains("Health of hero = { hp: 50 }"), "got: {}", out);
        assert!(out.contains("(set in frame 0)"), "got: {}", out);
        assert!(out.contains("<- by top-level code"), "got: {}", out);
    }

    #[test]
    fn spawn_provenance_when_never_set() {
        let vm = run(r#"
            component Pos { x: 0 }
            let e = spawn("rock", Pos { x: 7 })
            print(why(e, Pos))
        "#);
        let out = &vm.print_buffer[0];
        assert!(out.contains("(spawned in frame 0)"), "got: {}", out);
        assert!(out.contains("<- by top-level code"), "got: {}", out);
    }

    #[test]
    fn handler_chain_walks_back_through_two_events() {
        // Hit -> (handler emits) Robbed -> (handler sets) Gold. The chain
        // must surface both events and end at top-level code.
        let vm = run(r#"
            component Gold { amount: 50 }
            event Hit { amount }
            event Robbed { loss }
            let hero = spawn("hero", Gold { amount: 50 })
            on Hit(e) {
                emit Robbed { loss: e.amount }
            }
            on Robbed(e) {
                set(hero, Gold { amount: 0 })
            }
            emit Hit { amount: 10 }
            flush_events()
            flush_events()
            print(why(hero, Gold))
        "#);
        let out = &vm.print_buffer[0];
        assert!(out.contains("Gold of hero = { amount: 0 }"), "got: {}", out);
        assert!(out.contains("(set in frame 2)"), "got: {}", out);
        assert!(out.contains("<- by `on Robbed` handler"), "got: {}", out);
        assert!(
            out.contains("Robbed { loss: 10 } emitted in frame 1"),
            "got: {}",
            out
        );
        assert!(out.contains("<- by `on Hit` handler"), "got: {}", out);
        assert!(
            out.contains("Hit { amount: 10 } emitted in frame 0"),
            "got: {}",
            out
        );
        assert!(out.contains("<- by top-level code"), "got: {}", out);
    }

    #[test]
    fn system_writeback_attributes_to_the_system() {
        let vm = run(r#"
            component Health { hp: 100 }
            system Decay(h: mut Health) {
                h = Health { hp: h.hp - 1 }
            }
            let hero = spawn("hero", Health { hp: 100 })
            Decay()
            print(why(hero, Health))
        "#);
        let out = &vm.print_buffer[0];
        assert!(out.contains("Health of hero = { hp: 99 }"), "got: {}", out);
        assert!(out.contains("<- by system Decay"), "got: {}", out);
    }

    #[test]
    fn resource_writes_chain_through_handlers() {
        let vm = run(r#"
            resource Treasury { gold: 0 }
            event Loot { amount }
            on Loot(e) {
                let t = get_resource(Treasury) |> unwrap
                set_resource(Treasury, Treasury { gold: t.gold + e.amount })
            }
            emit Loot { amount: 25 }
            flush_events()
            print(why_resource(Treasury))
        "#);
        let out = &vm.print_buffer[0];
        assert!(
            out.contains("resource Treasury = { gold: 25 }"),
            "got: {}",
            out
        );
        assert!(out.contains("<- by `on Loot` handler"), "got: {}", out);
        assert!(
            out.contains("Loot { amount: 25 } emitted in frame 0"),
            "got: {}",
            out
        );
        assert!(out.contains("<- by top-level code"), "got: {}", out);
    }

    #[test]
    fn unwritten_values_say_so() {
        let vm = run(r#"
            component Pos { x: 0 }
            component Vel { dx: 0 }
            let e = spawn("rock", Pos { x: 1 })
            print(why(e, Vel))
        "#);
        let out = &vm.print_buffer[0];
        assert!(out.contains("no recorded write"), "got: {}", out);
    }

    #[test]
    fn simulation_forks_leave_no_provenance() {
        // The harder decay runs only inside simulate(): the main timeline's
        // ledger must still attribute Health to its spawn.
        let vm = run(r#"
            component Health { hp: 100 }
            system Decay(h: mut Health) {
                h = Health { hp: h.hp - 10 }
            }
            let hero = spawn("hero", Health { hp: 100 })
            let before = fork()
            let after = simulate(before, [system::Decay], 5)
            print(why(hero, Health))
        "#);
        let out = &vm.print_buffer[0];
        assert!(out.contains("(spawned in frame 0)"), "got: {}", out);
        assert!(!out.contains("by system Decay"), "got: {}", out);
    }

    #[test]
    fn eviction_uses_a_bounded_ring_and_advances_exactly_once_per_write() {
        // The retention cap exists so long-running processes don't OOM, so
        // eviction is the steady state. This type assertion is intentional:
        // `VecDeque::pop_front` is the standard library's amortized-O(1)
        // primitive. The previous Vec front-drain implementation cannot pass
        // this contract, while a wall-clock ratio can fail under unrelated CI
        // load and also compares a hash-free append with a cryptographically
        // committed eviction.
        fn require_ring_buffer<T>(_: &std::collections::VecDeque<T>) {}

        const CAP: usize = 10_000;
        const WRITES: usize = 200_000;
        let mut ledger = CausalityLedger::default();
        ledger.set_retention_cap(CAP);
        require_ring_buffer(&ledger.writes);

        for i in 0..WRITES {
            ledger.record_write(WriteRecord::local(
                0,
                Some(i as u32),
                None,
                "Hp",
                WriteSummary::full(format!("{{ hp: {} }}", i), smallvec::SmallVec::new()),
                WriteKind::Set,
                Cause::Main,
            ));
        }

        let evicted = WRITES - CAP;
        assert_eq!(ledger.writes.len(), CAP);
        assert_eq!(ledger.write_base, evicted);
        assert_eq!(ledger.write_watermark(), WRITES);
        assert_eq!(ledger.truncation().evicted_records, evicted as u64);
        assert_eq!(ledger.writes.front().and_then(|write| write.entity), Some(evicted as u32));
        assert_eq!(ledger.writes.back().and_then(|write| write.entity), Some((WRITES - 1) as u32));
    }

    #[test]
    fn retention_truncation_is_counted_hashed_and_deterministic() {
        fn ledger(last_value: i64) -> CausalityLedger {
            let mut ledger = CausalityLedger::default();
            ledger.set_retention_cap(2);
            for value in [1, 2, last_value] {
                ledger.record_write(WriteRecord::local(
                    0,
                    Some(7),
                    Some("job-7".to_string()),
                    "Lease",
                    WriteSummary::full(
                        format!("{{ token: {value} }}"),
                        smallvec::smallvec![(
                            "token".into(),
                            crate::causality::CausalScalar::Int(value)
                        )],
                    ),
                    WriteKind::Set,
                    Cause::Main,
                ));
            }
            ledger
        }

        let first = ledger(3);
        let same = ledger(3);
        let different = ledger(4);
        assert_eq!(first.truncation().evicted_records, 1);
        assert_ne!(first.truncation().digest, [0; 32]);
        assert_eq!(first.truncation(), same.truncation());
        assert_eq!(
            first.truncation(),
            different.truncation(),
            "the retained newest record does not change the evicted-history commitment"
        );

        let mut changed_eviction = CausalityLedger::default();
        changed_eviction.set_retention_cap(2);
        for value in [9, 2, 3] {
            changed_eviction.record_write(WriteRecord::local(
                0,
                Some(7),
                None,
                "Lease",
                WriteSummary::full(format!("{{ token: {value} }}"), Default::default()),
                WriteKind::Set,
                Cause::Main,
            ));
        }
        assert_ne!(first.truncation().digest, changed_eviction.truncation().digest);
    }

    #[test]
    fn commit_provenance_is_bounded_and_committed_to_the_marker() {
        let mut ledger = CausalityLedger::default();
        ledger.set_retention_cap(2);
        ledger.record_commit(1);
        ledger.record_commit(2);
        ledger.record_commit(3);

        assert_eq!(ledger.commits.len(), 2);
        assert_eq!(ledger.truncation().evicted_records, 1);
        assert_ne!(ledger.truncation().digest, [0; 32]);
    }

    #[test]
    fn despawn_matches_any_component_query() {
        let vm = run(r#"
            component Pos { x: 0 }
            event Cull { }
            let e = spawn("rock", Pos { x: 1 })
            on Cull(c) {
                despawn(e)
            }
            emit Cull { }
            flush_events()
            print(why(e, Pos))
        "#);
        let out = &vm.print_buffer[0];
        assert!(
            out.contains("rock was despawned in frame 1"),
            "got: {}",
            out
        );
        assert!(out.contains("<- by `on Cull` handler"), "got: {}", out);
    }
}

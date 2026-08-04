

#[cfg(test)]
mod tests {
    use super::{CausalityLedger, Cause, WriteKind};
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
    fn eviction_is_amortized_constant_time() {
        // The retention cap exists so long-running processes don't OOM —
        // which means the *eviction path* is the steady state of a long
        // process, not a rare corner. It must be O(1) per write. (It once
        // was a front-of-Vec drain per write: a full-window memmove each,
        // quadratic overall — the 1M-entity bench hung on world setup.)
        use std::time::Instant;
        let writes_n = 200_000usize;

        let mut under = CausalityLedger::default();
        under.set_retention_cap(1_000_000); // never evicts
        let t = Instant::now();
        for i in 0..writes_n {
            under.record_write(
                0,
                Some(i as u32),
                None,
                "Hp",
                format!("{{ hp: {} }}", i),
                WriteKind::Set,
                Cause::Main,
            );
        }
        let t_under = t.elapsed();

        let mut over = CausalityLedger::default();
        over.set_retention_cap(10_000); // evicts on ~95% of writes
        let t = Instant::now();
        for i in 0..writes_n {
            over.record_write(
                0,
                Some(i as u32),
                None,
                "Hp",
                format!("{{ hp: {} }}", i),
                WriteKind::Set,
                Cause::Main,
            );
        }
        let t_over = t.elapsed();

        assert_eq!(over.writes.len(), 10_000);
        // Generous bound: evicting writes may cost a small constant more
        // than appending ones (deallocation), but never a multiple.
        assert!(
            t_over < t_under * 4 + std::time::Duration::from_millis(50),
            "eviction must be amortized O(1): {:?} under cap vs {:?} evicting",
            t_under,
            t_over
        );
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

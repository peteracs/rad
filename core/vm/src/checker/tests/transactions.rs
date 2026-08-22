    #[test]
    fn transaction_rejects_transitive_write_outside_changes_only() {
        let (errors, report) = authority_src(
            r#"
            component Allowed {}
            component Hidden {}

            fn hidden_write(target: entity) -> nil {
                set(target, Hidden {})
            }

            transaction Rewrite(target: entity) {
                requires true
                changes_only [Allowed]
                hidden_write(target)
                ensures true
            }
            "#,
        );
        let violation = errors
            .iter()
            .find(|error| error.message.contains("violates its atomic effect boundary"))
            .unwrap_or_else(|| panic!("hidden write must be rejected: {errors:?}"));
        assert!(violation.message.contains("Hidden"));
        assert!(
            violation
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("hidden_write")),
            "diagnostic must contain the transitive authority path: {violation:?}"
        );
        let transaction = report
            .callables
            .values()
            .find(|callable| callable.kind == crate::types::AuthorityCallableKind::Transaction)
            .expect("transaction body has a first-class authority root");
        assert_eq!(transaction.transitive.writes, vec!["Hidden"]);
    }

    #[test]
    fn transaction_rejects_transitive_emit_io_async_and_unbounded_callback() {
        let (errors, _) = authority_src(
            r#"
            component Allowed {}
            event Published {}

            fn announce() -> nil { emit Published {} }
            fn output() -> nil { print("before commit") }
            async fn background() -> nil {}
            fn invoke(callback: fn() -> nil) -> nil { callback() }

            transaction Invalid(target: entity) {
                requires true
                changes_only [Allowed]
                announce()
                output()
                background()
                invoke(fn() -> nil { emit Published {} })
                ensures true
            }
            "#,
        );
        let violation = errors
            .iter()
            .find(|error| error.message.contains("violates its atomic effect boundary"))
            .unwrap_or_else(|| panic!("effect boundary must reject hidden effects: {errors:?}"));
        assert!(violation.message.contains("emits before commit"));
        assert!(violation.message.contains("IO before commit"));
        assert!(violation.message.contains("async execution before commit"));
    }

    #[test]
    fn post_commit_effects_are_excluded_from_atomic_body_but_reported_to_callers() {
        let (errors, report) = authority_src(
            r#"
            component Allowed {}
            event Published {}

            transaction Publish(target: entity) {
                requires true
                changes_only [Allowed]
                set(target, Allowed {})
                ensures true
                post_commit {
                    emit Published {}
                    print("published")
                }
            }
            "#,
        );
        assert!(
            !errors
                .iter()
                .any(|error| error.message.contains("violates its atomic effect boundary")),
            "post_commit is outside the atomic effect firewall: {errors:?}"
        );
        let atomic = report
            .callables
            .values()
            .find(|callable| callable.kind == crate::types::AuthorityCallableKind::Transaction)
            .unwrap();
        assert!(atomic.transitive.emits.is_empty());
        assert!(!atomic.transitive.io);
        let public = report.resolve("Publish").unwrap();
        assert_eq!(public.transitive.emits, vec!["Published"]);
        assert!(public.transitive.io);
    }

    #[test]
    fn post_commit_rejects_transitive_authoritative_state_writes() {
        let (errors, _) = authority_src(
            r#"
            component Count {}
            fn hidden_write(target: entity) -> nil { set(target, Count {}) }
            transaction Invalid(target: entity) {
                requires true
                changes_only [Count]
                set(target, Count {})
                ensures true
                post_commit { hidden_write(target) }
            }
            "#,
        );
        let violation = errors
            .iter()
            .find(|error| error.message.contains("violates its external-effect boundary"))
            .unwrap_or_else(|| panic!("post_commit write must be rejected: {errors:?}"));
        assert!(violation.message.contains("Count"));
        assert!(violation
            .hint
            .as_deref()
            .is_some_and(|hint| hint.contains("hidden_write")));
    }

    #[test]
    fn transaction_authorities_are_known_unique_and_support_structural_names() {
        let (errors, _) = authority_src(
            r#"
            component Allowed {}
            transaction Invalid(target: entity) {
                requires true
                changes_only [Allowed, Allowed, Missing, "$entities", "$entity_identity"]
                set(target, Allowed {})
                ensures true
            }
            "#,
        );
        assert!(errors.iter().any(|error| error.message.contains("more than once")));
        assert!(errors
            .iter()
            .any(|error| error.message.contains("unknown changes_only authority 'Missing'")));
        assert!(!errors.iter().any(|error| error.message.contains("'$entities'")));
    }

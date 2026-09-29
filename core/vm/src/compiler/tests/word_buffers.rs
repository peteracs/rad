    // Packed u32 word buffers (`words_*`) and the affine trapped-survival kernel.

    fn words_error(src: &str) -> String {
        run_source_result(src).expect_err("expected a runtime error")
    }

    #[test]
    fn words_construction_access_and_value_semantics() {
        let output = run_source(
            r#"
            let a: bytebuf = words_new(4, 7)
            let b: bytebuf = words_set(a, 2, 4000000000)
            print(words_len(a), bytebuf_len(a))
            print(words_get(a, 2), words_get(b, 2), words_get(b, 3))
        "#,
        );
        // words_set is functional: the original buffer is unchanged.
        assert_eq!(output, vec!["4 16", "7 4000000000 7"]);
    }

    #[test]
    fn words_reject_bad_values_indices_and_lengths() {
        assert!(words_error("let a: bytebuf = words_new(2, 4294967296)").contains("unsigned 32-bit"));
        assert!(words_error("let a: bytebuf = words_new(2, 0 - 1)").contains("non-negative"));
        assert!(words_error("print(words_get(words_new(2, 1), 2))").contains("out of bounds"));
        assert!(words_error("print(words_set(words_new(2, 1), 5, 1))").contains("out of bounds"));
        assert!(words_error("print(words_len(bytebuf_new(3)))").contains("multiple of 4"));
        assert!(words_error("print(words_add(words_new(2, 1), words_new(3, 1)))").contains("equal word lengths"));
    }

    #[test]
    fn words_gather_uses_the_affine_modular_index() {
        let output = run_source(
            r#"
            let mut src: bytebuf = words_new(6, 0)
            for i in range(6) { src = words_set(src, i, 10 + i) }
            // out[i] = src[(4 i + 2) mod 6]
            let g: bytebuf = words_gather(src, 6, 4, 2, 6)
            let mut row: list<int> = []
            for i in range(6) { row = push(row, words_get(g, i)) }
            print(row)
            // lift a length-3 prefix to length 6: out[i] = src[i mod 3]
            let lifted: bytebuf = words_gather(src, 6, 1, 0, 3)
            print(words_get(lifted, 4))
        "#,
        );
        assert_eq!(output, vec!["[12, 10, 14, 12, 10, 14]", "11"]);
        assert!(words_error("print(words_gather(words_new(3, 1), 2, 1, 0, 4))").contains("modulus"));
        assert!(words_error("print(words_gather(words_new(3, 1), 2, 1, 0, 0))").contains("modulus"));
    }

    #[test]
    fn words_elementwise_arithmetic_floors_and_traps_overflow() {
        let output = run_source(
            r#"
            let a: bytebuf = words_new(3, 7)
            let b: bytebuf = words_set(words_new(3, 5), 1, 9)
            let s: bytebuf = words_add(a, b)
            let m: bytebuf = words_min(a, b)
            let f: bytebuf = words_scale(a, 3, 1)          // floor(7 * 3 / 2) = 10
            print(words_get(s, 0), words_get(s, 1), words_get(m, 0), words_get(m, 1), words_get(f, 2))
        "#,
        );
        assert_eq!(output, vec!["12 16 5 7 10"]);
        assert!(words_error("print(words_add(words_new(1, 4000000000), words_new(1, 400000000)))").contains("overflowed"));
        assert!(words_error("print(words_scale(words_new(1, 4000000000), 2, 0))").contains("overflowed"));
        assert!(words_error("print(words_scale(words_new(1, 1), 1, 65))").contains("at most 64"));
    }

    #[test]
    fn words_add_strided_writes_only_the_strided_positions() {
        let output = run_source(
            r#"
            let t: bytebuf = words_add_strided(words_new(7, 1), words_new(2, 100), 1, 3)
            let mut row: list<int> = []
            for i in range(7) { row = push(row, words_get(t, i)) }
            print(row)
        "#,
        );
        assert_eq!(output, vec!["[1, 101, 1, 1, 101, 1, 1]"]);
        assert!(words_error("print(words_add_strided(words_new(7, 1), words_new(3, 1), 1, 3))").contains("out of bounds"));
        assert!(words_error("print(words_add_strided(words_new(7, 1), words_new(1, 1), 0, 0))").contains("stride must be positive"));
        assert!(words_error("print(words_add_strided(words_new(2, 4294967295), words_new(1, 1), 0, 1))").contains("overflowed"));
    }

    #[test]
    fn words_reductions_and_ratios() {
        let output = run_source(
            r#"
            let a: bytebuf = words_set(words_set(words_new(4, 6), 0, 2), 3, 9)
            let b: bytebuf = words_new(4, 3)
            print(words_min_value(a), words_max_value(a), words_count_gt(a, b))
            // floor(a[i] * 2^4 / b[i]): 2 -> 10, 6 -> 32, 9 -> 48
            print(words_min_ratio(a, b, 4), words_max_ratio(a, b, 4))
        "#,
        );
        assert_eq!(output, vec!["2 9 3", "10 48"]);
        assert!(words_error("print(words_min_value(words_new(0, 0)))").contains("empty"));
        assert!(words_error("print(words_min_ratio(words_new(2, 1), words_new(2, 0), 0))").contains("zero word"));
        assert!(words_error("print(words_max_ratio(words_new(2, 1), words_new(2, 1), 31))").contains("at most 30"));
    }

    #[test]
    fn words_digest_is_fnv1a_over_little_endian_bytes() {
        let output = run_source("print(words_digest(words_set(words_new(2, 1), 1, 258)))");
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in [1u8, 0, 0, 0, 2, 1, 0, 0] {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        assert_eq!(output, vec![(hash >> 2).to_string()]);
    }

    #[test]
    fn words_parallel_path_matches_the_sequential_definition() {
        // 200000 words exceeds the parallel threshold (65536). Results must equal
        // the element-by-element definition regardless of worker scheduling.
        let output = run_source(
            r#"
            let n: int = 200000
            let base: bytebuf = words_gather(words_set(words_set(words_new(3, 5), 1, 8), 2, 11), n, 1, 0, 3)
            let scaled: bytebuf = words_scale(base, 3, 1)             // 5->7, 8->12, 11->16
            let shifted: bytebuf = words_gather(scaled, n, 1, 1, n)   // rotate by one
            let combined: bytebuf = words_add_strided(shifted, words_new(int_div(n, 2), 1), 0, 2)
            print(words_min_value(combined), words_max_value(combined))
            print(words_count_gt(combined, words_new(n, 12)))
            print(words_digest(words_gather(base, n, 1, 0, n)) == words_digest(base))
        "#,
        );
        // shifted[i] = scaled[(i + 1) mod n]; even indices get +1.
        let n = 200_000usize;
        let mut above = 0;
        let mut lo = u32::MAX;
        let mut hi = 0;
        for i in 0..n {
            let base = [7u32, 12, 16][((i + 1) % n) % 3];   // the rotation wraps at n
            let v = base + if i % 2 == 0 { 1 } else { 0 };
            lo = lo.min(v);
            hi = hi.max(v);
            if v > 12 {
                above += 1;
            }
        }
        assert_eq!(output, vec![format!("{lo} {hi}"), above.to_string(), "true".to_string()]);
    }

    #[test]
    fn words_builtins_type_check_under_strict_types() {
        let output = crate::test_support::run_checked_source(
            r#"
            pure fn total(w: bytebuf) -> int { return words_max_value(words_add(w, w)) + words_len(w) }
            print(total(words_new(3, 2)))
        "#,
            ParserOptions,
            crate::checker::CheckerOptions::default(),
        )
        .expect("word kernels are pure and well typed");
        assert_eq!(output, vec!["7"]);
    }

    #[test]
    fn affine_trapped_survival_matches_the_independent_vm_computation() {
        // Values cross-checked against the pure-RAD implementation in
        // projects/dogfood/collatz-repulsion (exact_null.rad), L = 16, 3x+1.
        let output = run_source(
            r#"
            let raw: list<int> = affine_trapped_survival(16, 1, [32, 48, 64, 80, 96])
            let mut observed: list<int> = []
            for i in range(5) { observed = push(observed, raw[1 + 3 * i]) }
            print(observed)
            print(int_div(raw[2], 100))    // expected survivors at t = 32, truncated to tenths as in the VM run
        "#,
        );
        assert_eq!(output, vec!["[313, 113, 48, 12, 8]", "3139"]);
        assert!(words_error("print(affine_trapped_survival(16, 2, [40]))").contains("odd"));
        assert!(words_error("print(affine_trapped_survival(16, 1, [10]))").contains("mark"));
        assert!(words_error("print(affine_trapped_survival(1, 1, [10]))").contains("bits"));
    }

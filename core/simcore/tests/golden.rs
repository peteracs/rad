//! The P0 conformance gate: the compiled damage core must reproduce
//! the rad spec's golden corpus (golden_damage.txt) bit-for-bit.

use moba_simcore::{build_corpus, corpus_digest, resolve, DrTable, Lcg, POOL};

#[test]
fn golden_damage_bit_exact() {
    let fixture = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../projects/dogfood/moba/golden_damage.txt"
    ))
    .expect("golden_damage.txt (generate via: rad projects/dogfood/moba/damage_core.rad)");
    let expected: Vec<&str> = fixture.lines().collect();

    let mut rng = Lcg { s: 12345 };
    let mut ents = build_corpus(&mut rng);
    let dr = DrTable::default();

    let mut produced = vec![format!("D 0 {}", corpus_digest(&ents))];
    let mut k = 0;
    while k < 4096 {
        let src = rng.next(POOL as i64) as usize;
        let dst = rng.next(POOL as i64) as usize;
        let amount = 10_000 + rng.next(400_000);
        let dtype = rng.next(3);
        let via = rng.next(11);
        if src != dst {
            let mitigated = resolve(&mut ents, &dr, src, dst, amount, dtype, via);
            if k % 256 == 0 {
                produced.push(format!(
                    "R {k} {src} {dst} {mitigated} {}",
                    corpus_digest(&ents)
                ));
            }
        }
        k += 1;
    }
    produced.push(format!("D 1 {}", corpus_digest(&ents)));

    assert_eq!(produced.len(), expected.len(), "checkpoint count");
    for (i, (got, want)) in produced.iter().zip(&expected).enumerate() {
        assert_eq!(got, want, "DIVERGENCE at checkpoint {i}");
    }
}

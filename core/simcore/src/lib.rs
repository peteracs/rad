//! moba-simcore — browser-moba's damage resolution, compiled.
//! Mirror of `projects/dogfood/moba/damage_core.rad` (the debuggable spec),
//! statement-for-statement, i64 fixed-point. Conformance: the golden
//! corpus (golden_damage.txt) must reproduce bit-exactly (tests/).
//!
//! wasm ABI design for SPEED: ONE call per batch, flat typed arrays,
//! zero per-request crossings. The JS adapter converts float<->milli
//! once at the boundary and routes hook-bearing requests to the legacy
//! JS path (P0 strangler contract).

// `ents[e + 0]` is the hp field offset, written to line up with `e + 1`,
// `e + 15`, and the layout table above. Collapsing it to `ents[e]` would break
// that column alignment at every read site.
#![allow(clippy::identity_op)]

use wasm_bindgen::prelude::*;

pub const SHIELD_SLOTS: usize = 3;

/// Entity table, SoA. Layout per entity (stride = 17 i64):
///  0 hp, 1 hp_max, 2 armor_m, 3 mr_m,
///  4 apen_pct_pm, 5 apen_flat_m, 6 mpen_pct_pm, 7 mpen_flat_m,
///  8 lifesteal_pm, 9 spellvamp_pm, 10 amp_pm,
/// 11 klass (0 hero/1 unit/2 building), 12 is_ward, 13 is_turret,
/// 14 alive, 15..17 shield slots s0,s1,s2
pub const ENT_STRIDE: usize = 18;

/// dr table, per-mille: [hero_unit, hero_building, unit_hero,
/// unit_building, building_hero, building_unit]
#[derive(Clone, Copy)]
pub struct DrTable {
    pub hero_unit: i64,
    pub hero_building: i64,
    pub unit_hero: i64,
    pub unit_building: i64,
    pub building_hero: i64,
    pub building_unit: i64,
}

impl Default for DrTable {
    fn default() -> Self {
        DrTable {
            hero_unit: 1000,
            hero_building: 1000,
            unit_hero: 700,
            unit_building: 500,
            building_hero: 1050,
            building_unit: 1105,
        }
    }
}

#[inline]
fn class_ratio_pm(dr: &DrTable, sk: i64, dk: i64) -> i64 {
    match (sk, dk) {
        (0, 1) => dr.hero_unit,
        (0, 2) => dr.hero_building,
        (1, 0) => dr.unit_hero,
        (1, 2) => dr.unit_building,
        (2, 0) => dr.building_hero,
        (2, 1) => dr.building_unit,
        _ => 1000,
    }
}

#[inline]
fn mitigate(ents: &[i64], dtype: i64, src: usize, dst: usize, amount: i64) -> i64 {
    if dtype == 2 {
        return amount;
    }
    let s = src * ENT_STRIDE;
    let d = dst * ENT_STRIDE;
    let resist = if dtype == 0 {
        ents[d + 2] * (1000 - ents[s + 4]) / 1000 - ents[s + 5]
    } else {
        ents[d + 3] * (1000 - ents[s + 6]) / 1000 - ents[s + 7]
    };
    let resist = resist.max(0);
    amount * 100_000 / (100_000 + resist)
}

/// One request through the core. Returns the mitigated amount (the
/// damage event's payload). Mutates hp/shields/alive in `ents`.
pub fn resolve(
    ents: &mut [i64],
    dr: &DrTable,
    src: usize,
    dst: usize,
    amount: i64,
    dtype: i64,
    via: i64,
) -> i64 {
    let d = dst * ENT_STRIDE;
    let s = src * ENT_STRIDE;
    if ents[d + 14] == 0 {
        return 0;
    }
    // 1. class ratio: basic attacks only
    let mut scaled = amount;
    if via == 0 {
        scaled = scaled * class_ratio_pm(dr, ents[s + 11], ents[d + 11]) / 1000;
    }
    // 2. outgoing amp: physical/magic only
    if dtype != 2 {
        scaled = scaled * ents[s + 10] / 1000;
    }
    // 3. era ward clamp
    if scaled >= 1000 && ents[d + 12] == 1 {
        let self_destruct = via == 10 && src == dst;
        if !self_destruct && ents[s + 13] == 0 {
            scaled = if via == 0 { 1000 } else { 0 };
        }
    }
    // 4. mitigation
    let mitigated = mitigate(ents, dtype, src, dst, scaled);
    // 5. shields oldest-first
    let mut to_health = mitigated;
    for k in 0..SHIELD_SLOTS {
        if to_health <= 0 {
            break;
        }
        let sh = ents[d + 15 + k];
        if sh > 0 {
            let soak = sh.min(to_health);
            ents[d + 15 + k] = sh - soak;
            to_health -= soak;
        }
    }
    // 6. hp write, clamped
    let hp = (ents[d + 0] - to_health).max(0);
    ents[d + 0] = hp;
    if hp <= 0 {
        ents[d + 14] = 0;
    }
    // 7. vamp on the full mitigated amount, only while the source lives
    let vamp_pm = if via == 0 { ents[s + 8] } else { ents[s + 9] };
    if vamp_pm > 0 && ents[s + 0] > 0 {
        ents[s + 0] = (ents[s + 0] + mitigated * vamp_pm / 1000).min(ents[s + 1]);
    }
    mitigated
}

/// THE batch ABI (ONE wasm call per sim tick): Int32 only — BigInt64Array
/// crossings are slow in every engine, Int32Array is free, and every
/// value fits i32 (hp milli <= ~3.3M). Widened to i64 internally, same
/// exact arithmetic as the spec, narrowed on return.
/// requests = flat [src, dst, amount_milli, dtype, via] x n; returns
/// the mitigated amounts (one per request) for event emission.
#[wasm_bindgen]
pub fn resolve_batch_i32(ents32: &mut [i32], requests32: &[i32]) -> Vec<i32> {
    let dr = DrTable::default();
    let mut ents: Vec<i64> = ents32.iter().map(|&v| v as i64).collect();
    let n = requests32.len() / 5;
    let mut out = Vec::with_capacity(n);
    for r in 0..n {
        let q = &requests32[r * 5..r * 5 + 5];
        out.push(resolve(
            &mut ents,
            &dr,
            q[0] as usize,
            q[1] as usize,
            q[2] as i64,
            q[3] as i64,
            q[4] as i64,
        ) as i32);
    }
    for (dst, src) in ents32.iter_mut().zip(&ents) {
        *dst = *src as i32;
    }
    out
}

// ------------------------------------------------ golden corpus mirror

pub struct Lcg {
    pub s: i64,
}

impl Lcg {
    /// High-bit extraction — matches the rad spec's rng_next exactly
    /// (LCG low bits have tiny periods; see damage_core.rad).
    pub fn next(&mut self, modulo: i64) -> i64 {
        self.s = (self.s * 1103515245 + 12345) % 2147483648;
        (self.s / 65536) % modulo
    }
}

pub const POOL: usize = 64;

pub fn build_corpus(rng: &mut Lcg) -> Vec<i64> {
    let mut ents = vec![0i64; POOL * ENT_STRIDE];
    for i in 0..POOL {
        let e = i * ENT_STRIDE;
        let hp_max = 400_000 + rng.next(2_400_000);
        let mut sh = 0;
        if rng.next(4) == 0 {
            sh = rng.next(300_000);
        }
        ents[e + 0] = hp_max - rng.next(hp_max / 2);
        ents[e + 1] = hp_max;
        ents[e + 2] = rng.next(250_000);
        ents[e + 3] = rng.next(200_000);
        ents[e + 4] = rng.next(400);
        ents[e + 5] = rng.next(40_000);
        ents[e + 6] = rng.next(400);
        ents[e + 7] = rng.next(40_000);
        ents[e + 8] = rng.next(300);
        ents[e + 9] = rng.next(250);
        ents[e + 10] = 950 + rng.next(150);
        ents[e + 11] = rng.next(3);
        ents[e + 14] = 1;
        ents[e + 15] = sh;
    }
    for k in 0..4 {
        let w = (k * 7 + 2) * ENT_STRIDE;
        ents[w + 12] = 1;
        ents[w + 0] = 3000;
        ents[w + 1] = 3000;
        let t = (k * 9 + 5) * ENT_STRIDE;
        ents[t + 13] = 1;
        ents[t + 11] = 2;
    }
    ents
}

pub fn corpus_digest(ents: &[i64]) -> i64 {
    let mut acc: i64 = 0;
    for i in 0..POOL {
        let e = i * ENT_STRIDE;
        acc = (acc * 31 + ents[e + 0]) % 1_000_000_007;
        acc = (acc * 31 + ents[e + 15]) % 1_000_000_007;
        acc = (acc * 31 + ents[e + 14]) % 1_000_000_007;
    }
    acc
}

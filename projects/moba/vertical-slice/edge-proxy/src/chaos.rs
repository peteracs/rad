//! Debug-only network chaos emulation for the edge proxy.
//!
//! The proxy is the realistic place to model the public-internet path between a
//! browser and the RAD UDP authority: it sits on both directions of the
//! datagram stream and is the only hop that is conceptually "the network". In
//! production the proxy stays a dumb forwarder; chaos is opt-in via environment
//! variables and is fully bypassed (zero added allocation, zero spawned tasks)
//! unless explicitly enabled. That lets us stress local prediction, visual
//! interpolation, and the ack-bit prediction-delay loop against ping, loss, and
//! jitter without depending on a real degraded link.
//!
//! The decision logic is a pure function over a seeded PRNG so it is fully
//! deterministic and unit-tested; the async send/delay/drop wiring lives in
//! `main.rs`.

use std::env;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const ENV_LATENCY_MS: &str = "MOBA_RAD_CHAOS_LATENCY_MS";
const ENV_JITTER_MS: &str = "MOBA_RAD_CHAOS_JITTER_MS";
const ENV_LOSS_PCT: &str = "MOBA_RAD_CHAOS_LOSS_PCT";

const PARTS_PER_MILLION: u64 = 1_000_000;

/// Network conditions to emulate. `Copy` so it threads cheaply into per-session
/// pump tasks with no shared state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChaosConfig {
    /// Base one-way delay added to every delivered datagram, in milliseconds.
    latency_ms: u32,
    /// Symmetric delivery variance in milliseconds: the applied delay is
    /// `latency +/- U[0, jitter]`, clamped at zero. Non-zero jitter lets later
    /// datagrams overtake earlier ones, which is exactly the reordering the
    /// out-of-order guards must survive.
    jitter_ms: u32,
    /// Drop probability scaled to parts-per-million to keep the model integer
    /// and exactly reproducible.
    loss_ppm: u32,
}

/// What to do with a single datagram.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChaosVerdict {
    /// Silently drop the datagram (emulated packet loss).
    Drop,
    /// Deliver after the given delay. A zero delay means "forward immediately,
    /// preserving order" so the hot path can skip buffering entirely.
    Deliver(Duration),
}

impl ChaosConfig {
    /// Build from explicit values. `loss_pct` is clamped to `[0, 100]`.
    /// Passing all zeros yields fully transparent forwarding (`is_enabled` false).
    pub fn new(latency_ms: u32, jitter_ms: u32, loss_pct: f64) -> Self {
        let clamped = loss_pct.clamp(0.0, 100.0);
        let loss_ppm = (clamped / 100.0 * PARTS_PER_MILLION as f64).round() as u32;
        Self {
            latency_ms,
            jitter_ms,
            loss_ppm,
        }
    }

    /// Read the three chaos knobs from the environment. Unset or unparseable
    /// values fall back to "disabled" for that knob, so a partial configuration
    /// (e.g. loss only) is valid.
    pub fn from_env() -> Self {
        Self::new(
            env_u32(ENV_LATENCY_MS),
            env_u32(ENV_JITTER_MS),
            env_f64(ENV_LOSS_PCT),
        )
    }

    /// True when any knob would alter the stream. The proxy uses this to keep
    /// the production forwarding path completely untouched.
    pub fn is_enabled(&self) -> bool {
        self.latency_ms != 0 || self.jitter_ms != 0 || self.loss_ppm != 0
    }

    pub fn latency_ms(&self) -> u32 {
        self.latency_ms
    }

    pub fn jitter_ms(&self) -> u32 {
        self.jitter_ms
    }

    /// Loss probability as a percentage, for logging.
    pub fn loss_pct(&self) -> f64 {
        self.loss_ppm as f64 / PARTS_PER_MILLION as f64 * 100.0
    }

    /// Decide the fate of one datagram. Pure given `rng`, so it is deterministic
    /// and unit-tested. Loss is rolled first; survivors get a clamped,
    /// jittered delay.
    pub fn decide(&self, rng: &mut Rng) -> ChaosVerdict {
        if self.loss_ppm != 0 && rng.below(PARTS_PER_MILLION as u32) < self.loss_ppm {
            return ChaosVerdict::Drop;
        }

        let mut delay = self.latency_ms as i64;
        if self.jitter_ms != 0 {
            let span = self.jitter_ms * 2 + 1;
            delay += rng.below(span) as i64 - self.jitter_ms as i64;
        }
        if delay < 0 {
            delay = 0;
        }
        ChaosVerdict::Deliver(Duration::from_millis(delay as u64))
    }
}

/// Tiny dependency-free `xorshift64*` PRNG. Not cryptographic — it only needs to
/// be fast, allocation-free, and deterministic for tests. Each pump task owns
/// its own instance, so there is no shared mutable state on the hot path.
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Avoid the all-zero state, which xorshift cannot escape.
        Self {
            state: (seed ^ 0x9e37_79b9_7f4a_7c15) | 1,
        }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Uniform integer in `[0, bound)`. Returns 0 when `bound == 0`.
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() % bound as u64) as u32
    }
}

/// Seed a per-task PRNG from wall-clock time mixed with the session id and a
/// direction salt, so the two pump directions of one session draw independent
/// chaos streams and restarts do not replay the same pattern.
pub fn seeded_rng(session_id: u64, salt: u64) -> Rng {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or(0);
    Rng::new(nanos ^ session_id.wrapping_mul(0x0100_0000_01b3) ^ salt)
}

fn env_u32(name: &str) -> u32 {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(0)
}

fn env_f64(name: &str) -> f64 {
    env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_config_always_delivers_immediately() {
        let chaos = ChaosConfig::new(0, 0, 0.0);
        assert!(!chaos.is_enabled());
        let mut rng = Rng::new(1);
        for _ in 0..10_000 {
            assert_eq!(chaos.decide(&mut rng), ChaosVerdict::Deliver(Duration::ZERO));
        }
    }

    #[test]
    fn full_loss_drops_every_datagram() {
        let chaos = ChaosConfig::new(0, 0, 100.0);
        assert!(chaos.is_enabled());
        let mut rng = Rng::new(42);
        for _ in 0..10_000 {
            assert_eq!(chaos.decide(&mut rng), ChaosVerdict::Drop);
        }
    }

    #[test]
    fn zero_loss_never_drops() {
        let chaos = ChaosConfig::new(50, 0, 0.0);
        let mut rng = Rng::new(7);
        for _ in 0..10_000 {
            assert_eq!(
                chaos.decide(&mut rng),
                ChaosVerdict::Deliver(Duration::from_millis(50))
            );
        }
    }

    #[test]
    fn partial_loss_stays_within_a_sane_band() {
        // ~10% loss should drop meaningfully but not everything.
        let chaos = ChaosConfig::new(0, 0, 10.0);
        let mut rng = Rng::new(123);
        let mut drops = 0;
        let samples = 100_000;
        for _ in 0..samples {
            if chaos.decide(&mut rng) == ChaosVerdict::Drop {
                drops += 1;
            }
        }
        // Loose bounds: deterministic PRNG, but assert the rate is in the right ballpark.
        assert!(drops > samples * 5 / 100, "expected >5% drops, got {drops}");
        assert!(drops < samples * 15 / 100, "expected <15% drops, got {drops}");
    }

    #[test]
    fn jitter_stays_within_clamped_bounds() {
        let chaos = ChaosConfig::new(100, 15, 0.0);
        let mut rng = Rng::new(999);
        let mut saw_low = false;
        let mut saw_high = false;
        for _ in 0..100_000 {
            match chaos.decide(&mut rng) {
                ChaosVerdict::Deliver(delay) => {
                    let ms = delay.as_millis() as i64;
                    assert!((85..=115).contains(&ms), "delay {ms} outside [85, 115]");
                    if ms <= 86 {
                        saw_low = true;
                    }
                    if ms >= 114 {
                        saw_high = true;
                    }
                }
                ChaosVerdict::Drop => panic!("zero-loss config must never drop"),
            }
        }
        assert!(saw_low && saw_high, "jitter should span the full window");
    }

    #[test]
    fn jitter_never_produces_negative_delay() {
        // Jitter larger than latency must clamp at zero, not underflow.
        let chaos = ChaosConfig::new(5, 40, 0.0);
        let mut rng = Rng::new(2024);
        let mut saw_zero = false;
        for _ in 0..100_000 {
            if let ChaosVerdict::Deliver(delay) = chaos.decide(&mut rng) {
                if delay.is_zero() {
                    saw_zero = true;
                }
            }
        }
        assert!(saw_zero, "large jitter should sometimes clamp to zero delay");
    }

    #[test]
    fn loss_pct_round_trips_through_ppm() {
        assert_eq!(ChaosConfig::new(0, 0, 5.0).loss_pct(), 5.0);
        assert!(!ChaosConfig::new(0, 0, 0.0).is_enabled());
        // Out-of-range loss clamps instead of overflowing.
        assert_eq!(ChaosConfig::new(0, 0, 250.0).loss_pct(), 100.0);
    }
}

//! Batch front end for the shadow-cycle relaxation. These runs take seconds to
//! minutes, past RAD's native-call deadline, so they are not extension calls.
//! Writes one JSON receipt to stdout.

use rad_dogfood_math_kernels::shadow_walk::{ceiling, certify, Relaxation, ONE};
use serde_json::{json, Value};
use std::{env, fs};

const USAGE: &str = "usage: shadow_walk ceiling K PER_BIT TOP_BITS SHIFT BARRIERS.json\n       \
                     shadow_walk certify K PER_BIT TOP_BITS SHIFT BARRIERS.json REQUIREMENT";

fn number<T: std::str::FromStr>(text: &str, name: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("{name} must be a number, got {text:?}"))
}

fn run(args: &[String]) -> Result<Value, String> {
    let [command, k, per_bit, top_bits, shift, path, rest @ ..] = args else {
        return Err(USAGE.to_string());
    };
    let text = fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let barriers: Vec<i64> =
        serde_json::from_str(&text).map_err(|error| format!("{path}: {error}"))?;
    let relaxation = Relaxation {
        k: number(k, "K")?,
        per_bit: number(per_bit, "PER_BIT")?,
        top_bits: number(top_bits, "TOP_BITS")?,
        shift: number(shift, "SHIFT")?,
        barriers: &barriers,
    };
    let receipt = match (command.as_str(), rest) {
        ("ceiling", []) => json!(ceiling(&relaxation)?),
        ("certify", [requirement]) => {
            json!(certify(&relaxation, number(requirement, "REQUIREMENT")?)?)
        }
        _ => return Err(USAGE.to_string()),
    };
    Ok(json!({ "scale": ONE, "receipt": receipt }))
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(value) => println!("{value}"),
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}

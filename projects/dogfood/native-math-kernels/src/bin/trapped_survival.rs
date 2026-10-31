//! Batch front end for trapped-set survival. Large runs take minutes, past
//! RAD's native-call deadline, so RAD programs read the JSON receipts this
//! writes instead of calling the kernel.

use rad_dogfood_math_kernels::trapped_survival::trapped_survival;
use std::{env, fs};

const USAGE: &str = "usage: trapped_survival BITS D MARK[,MARK...] OUT.json";

fn run(args: &[String]) -> Result<(), String> {
    let [bits, d, marks, out] = args else {
        return Err(USAGE.to_string());
    };
    let bits = bits.parse().map_err(|_| USAGE.to_string())?;
    let d = d.parse().map_err(|_| USAGE.to_string())?;
    let marks = marks
        .split(',')
        .map(str::parse)
        .collect::<Result<Vec<usize>, _>>()
        .map_err(|_| USAGE.to_string())?;
    let receipt = trapped_survival(bits, d, &marks)?;
    let encoded = serde_json::to_string(&receipt).map_err(|error| error.to_string())?;
    fs::write(out, encoded).map_err(|error| format!("{out}: {error}"))
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if let Err(message) = run(&args) {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

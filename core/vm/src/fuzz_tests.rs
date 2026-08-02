//! D2 — the soundness gate. Structure-aware fuzzing of every decode path
//! that accepts bytes from outside the process:
//!
//!   - `radpack::open` / `radpack::open_file`  (the new inflate boundary)
//!   - `fork_from_bytes`                        (full fork codec)
//!   - `fork_apply`                             (delta codec, vs a real base)
//!   - `load_world`                             (save codec, v2 + drifted schema)
//!   - `merge_forks` (fed *decoded mutants*: forks that survived the codec
//!     are piped into the merge engine)
//!
//! Why structure-aware: every payload carries a blake3 digest, so blind
//! byte-flipping dies at the integrity check without ever reaching the
//! parser. The mutator therefore **re-seals half of its mutants with a
//! correct digest** — those penetrate to the JSON layer, schema validation,
//! the id allocator, and the migrate machinery. The other half keeps the
//! stale digest to exercise the rejection path itself.
//!
//! The corpus is grown, not hand-written: seeded rad programs use rad's own
//! RNG to build varied worlds (unicode names, negative ints, in-flight
//! events, despawn free-lists, multi-component entities) and print their
//! own wire payloads. RADTRACK's real artifacts (saves, bases, tapes) join
//! the corpus when present on disk.
//!
//! The contract under test is single: **malformed input is an `Err`, never
//! a panic.** Budget scales with RAD_FUZZ_ITERS (mutants per payload,
//! default 120 — CI-friendly seconds; thousands for soak runs).

use crate::compiler::Compiler;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::value::{Builtin, Value};
use crate::vm::VM;
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// harness plumbing
// ---------------------------------------------------------------------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn iters() -> usize {
    std::env::var("RAD_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120)
}

fn compile(src: &str) -> crate::compiler::CompileResult {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize().0;
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    assert!(
        parser.errors().is_empty(),
        "parse errors: {:?}",
        parser.errors()
    );
    Compiler::new().compile(&program).expect("compile")
}

fn fresh_vm(src: &str) -> VM {
    let mut vm = VM::new();
    vm.suppress_output();
    vm.set_random_seed(7);
    vm.load_compile_result(compile(src));
    vm.run(0).expect("decoder program must run");
    vm
}

// ---------------------------------------------------------------------------
// corpus
// ---------------------------------------------------------------------------

/// One grown corpus entry: the world's wire surfaces, all from one program.
struct Corpus {
    base_bytes: String,
    delta: String,
    full: String,
    save: String,
}

/// Decls shared by the corpus generator and the decoder VMs: decoders must
/// know the schema to validate against (and to revive in-flight Ping events).
const DECLS: &str = r#"
component Alpha { n: 0, f: 0.0, s: "", b: false }
component Beta  { x: 0, label: "" }
component Gamma { v: 0 }
resource Counter { total: 0, tag: "" }
event Ping { who: entity, amount: int }
"#;

/// A decoder whose Alpha drifted (added field, migrate declared) and whose
/// Beta drifted with NO migration — payloads carrying Beta must Err loudly,
/// Alpha rows must migrate. Both paths, zero panics.
const DRIFTED_DECLS: &str = r#"
component Alpha { n: 0, f: 0.0, s: "", b: false, extra: 0 }
component Beta  { x: 0 }
component Gamma { v: 0 }
resource Counter { total: 0, tag: "" }
event Ping { who: entity, amount: int }
migrate Alpha(old) {
    return Alpha { n: old["n"], f: old["f"], s: old["s"], b: old["b"], extra: 7 }
}
"#;

fn grow_corpus(seed: u64) -> Corpus {
    let src = format!(
        r#"{decls}
rand_seed({seed})
let n_ents = rand_int(3, 25)
let mut made = []
for i in range(0, n_ents) {{
    let mut s = ""
    let slen = rand_int(0, 12)
    for _k in range(0, slen) {{
        let cp = rand_int(32, 1200)
        s = s + chr(cp)
    }}
    let e = spawn(f"e{{i}}", Alpha {{ n: rand_int(-99999999, 99999999), f: rand_float(), s: s, b: rand_bool() }})
    if rand_bool() {{ set(e, Beta {{ x: rand_int(0, 100), label: s + "-β" }}) }}
    if rand_int(0, 4) == 0 {{ set(e, Gamma {{ v: i }}) }}
    made << e
}}
set_resource(Counter, Counter {{ total: rand_int(0, 1000000), tag: "κόσμος\n\"quoted\"" }})
for e in made {{
    if rand_int(0, 5) == 0 {{ despawn(e) }}
}}
let base = fork()
print(fork_to_bytes(base))
for e in entities(Alpha) {{
    if rand_int(0, 3) == 0 {{ update(e, Alpha) {{ n = rand_int(0, 999) }} }}
    if has(e, Beta) and rand_int(0, 6) == 0 {{ remove(e, Beta) }}
}}
let late = spawn("late", Alpha {{ n: 1, f: 2.5, s: "tail\n\"q\"", b: true }})
emit Ping {{ who: late, amount: 42 }}
print(fork_delta(base, fork()))
print(fork_to_bytes(fork()))
flush_events()
print(save_world())
"#,
        decls = DECLS,
        seed = seed
    );
    let mut vm = VM::new();
    vm.suppress_output();
    vm.set_random_seed(seed);
    vm.load_compile_result(compile(&src));
    vm.run(0).expect("corpus program must run");
    assert_eq!(vm.print_buffer.len(), 4, "corpus program prints 4 payloads");
    Corpus {
        base_bytes: vm.print_buffer[0].clone(),
        delta: vm.print_buffer[1].clone(),
        full: vm.print_buffer[2].clone(),
        save: vm.print_buffer[3].clone(),
    }
}

// ---------------------------------------------------------------------------
// mutation
// ---------------------------------------------------------------------------

/// Split a legacy payload into (tag, body) so the body can be mutated and
/// re-sealed with a CORRECT digest — mutants then reach the layers behind
/// the integrity check.
fn split_payload(payload: &str) -> Option<(String, String)> {
    // RADWORLD3 (like RADFORK2/RADDELTA1) carries the digest between tag and
    // body; RADWORLD2 is the digest-less legacy form.
    for tag in ["RADFORK2", "RADDELTA1", "RADWORLD3"] {
        if let Some(rest) = payload.strip_prefix(&format!("{} ", tag)) {
            let (_digest, body) = rest.split_once(' ')?;
            return Some((tag.to_string(), body.to_string()));
        }
    }
    payload
        .strip_prefix("RADWORLD2 ")
        .map(|body| ("RADWORLD2".to_string(), body.to_string()))
}

fn reseal(tag: &str, body: &str) -> String {
    let digest = blake3::hash(body.as_bytes()).to_hex();
    match tag {
        "RADWORLD2" => format!("{} {}", tag, body),
        _ => format!("{} {} {}", tag, digest.as_str(), body),
    }
}

const NUMBER_BOMBS: &[&str] = &[
    "99999999999999999999",
    "-99999999999999999999",
    "4294967296",
    "18446744073709551616",
    "-1",
    "1e308",
    "-1e308",
    "2147483648",
    "1152921504606846976",
];

/// One mutation step. Output is always valid UTF-8 (rad strings are UTF-8,
/// so invalid byte sequences are not representable as decoder input —
/// `from_utf8_lossy` is the honest normalization).
fn mutate_once(rng: &mut Rng, input: &str) -> String {
    let mut b = input.as_bytes().to_vec();
    if b.is_empty() {
        return "[".to_string();
    }
    match rng.below(9) {
        0 => {
            b.truncate(rng.below(b.len()));
        }
        1 => {
            let start = rng.below(b.len());
            let end = (start + 1 + rng.below(64)).min(b.len());
            b.drain(start..end);
        }
        2 => {
            let start = rng.below(b.len());
            let end = (start + 1 + rng.below(64)).min(b.len());
            let chunk: Vec<u8> = b[start..end].to_vec();
            let at = rng.below(b.len());
            b.splice(at..at, chunk);
        }
        3 => {
            let at = rng.below(b.len());
            b[at] ^= 1 << rng.below(8);
        }
        4 => {
            // structural token swap
            let (from, to): (u8, u8) = match rng.below(6) {
                0 => (b'[', b'{'),
                1 => (b'{', b'['),
                2 => (b']', b'}'),
                3 => (b',', b':'),
                4 => (b'"', b'\''),
                _ => (b':', b','),
            };
            if let Some(pos) = b.positions_nth(from, rng.below(8)) {
                b[pos] = to;
            }
        }
        5 => {
            // number bomb: replace a digit run
            let s = String::from_utf8_lossy(&b).into_owned();
            let from = rng.below(s.len().max(1));
            if let Some(start) = s
                .char_indices()
                .find(|&(i, c)| c.is_ascii_digit() && i >= from)
                .map(|(i, _)| i)
            {
                let end = s[start..]
                    .find(|c: char| !c.is_ascii_digit())
                    .map(|e| start + e)
                    .unwrap_or(s.len());
                let bomb = NUMBER_BOMBS[rng.below(NUMBER_BOMBS.len())];
                return format!("{}{}{}", &s[..start], bomb, &s[end..]);
            }
            return s;
        }
        6 => {
            // inject noise
            let noise: &[u8] = match rng.below(5) {
                0 => b"\\u0000",
                1 => br#","x":["#,
                2 => "💥".as_bytes(),
                3 => b"[[[[[[[[",
                _ => b"null",
            };
            let at = rng.below(b.len());
            b.splice(at..at, noise.iter().copied());
        }
        7 => {
            // splice with itself, shifted
            let at = rng.below(b.len());
            let chunk: Vec<u8> = b[..rng.below(b.len()).min(128)].to_vec();
            b.splice(at..at, chunk);
        }
        _ => {
            // swap two halves around a pivot
            let at = rng.below(b.len());
            b.rotate_left(at);
        }
    }
    String::from_utf8_lossy(&b).into_owned()
}

/// Byte-level mutation for binary corpora (zstd tapes). No UTF-8
/// normalization here — `open_file` takes `&[u8]` and must survive
/// arbitrary bytes.
fn mutate_raw(rng: &mut Rng, input: &[u8]) -> Vec<u8> {
    let mut b = input.to_vec();
    if b.is_empty() {
        return vec![0xFF];
    }
    match rng.below(6) {
        0 => b.truncate(rng.below(b.len())),
        1 => {
            let start = rng.below(b.len());
            let end = (start + 1 + rng.below(256)).min(b.len());
            b.drain(start..end);
        }
        2 => {
            let at = rng.below(b.len());
            b[at] ^= 1 << rng.below(8);
        }
        3 => {
            // corrupt the zstd/deflate stream body, keep the header intact
            if b.len() > 32 {
                let at = 24 + rng.below(b.len() - 24);
                b[at] = b[at].wrapping_add(1 + rng.below(255) as u8);
            }
        }
        4 => {
            // declared-length lies: perturb early header bytes
            let at = rng.below(b.len().min(24));
            b[at] = rng.next() as u8;
        }
        _ => {
            let at = rng.below(b.len());
            let chunk: Vec<u8> = b[..rng.below(b.len()).min(512)].to_vec();
            b.splice(at..at, chunk);
        }
    }
    b
}

trait PositionsNth {
    fn positions_nth(&self, needle: u8, n: usize) -> Option<usize>;
}
impl PositionsNth for [u8] {
    fn positions_nth(&self, needle: u8, n: usize) -> Option<usize> {
        self.iter()
            .enumerate()
            .filter(|(_, &b)| b == needle)
            .map(|(i, _)| i)
            .nth(n)
    }
}

/// Produce one decoder input from a corpus payload: mutate the body 1-3
/// times, then choose an envelope strategy.
fn make_mutant(rng: &mut Rng, payload: &str) -> String {
    let (tag, body) = match split_payload(payload) {
        Some(p) => p,
        None => return mutate_once(rng, payload),
    };
    let mut mutated = body;
    for _ in 0..(1 + rng.below(3)) {
        mutated = mutate_once(rng, &mutated);
    }
    match rng.below(10) {
        // re-seal with a correct digest: reaches parser + semantics
        0..=4 => reseal(&tag, &mutated),
        // pack the mutated body: exercises envelope + parser together
        5 => crate::radpack::seal(&tag, &mutated),
        // stale digest: exercises the rejection path
        6..=7 => match tag.as_str() {
            "RADWORLD2" => format!("{} {}", tag, mutated),
            _ => format!("{} {} {}", tag, "0".repeat(64), mutated),
        },
        // mutate the whole envelope (headers, digest field, everything)
        _ => mutate_once(rng, payload),
    }
}

// ---------------------------------------------------------------------------
// the gate
// ---------------------------------------------------------------------------

/// Drive one decoder builtin with one input; a panic is a finding. The
/// input is dumped so any crash is immediately reproducible.
fn must_not_panic(
    vm: &mut VM,
    builtin: Builtin,
    mk_args: impl FnOnce(&mut VM) -> Vec<Value>,
) -> bool {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let args = mk_args(vm);
        let _ = vm.call_builtin(builtin, args);
    }));
    result.is_ok()
}

fn report_finding(target: &str, input: &str) -> String {
    let dir = std::env::temp_dir().join("rad_fuzz_findings");
    let _ = std::fs::create_dir_all(&dir);
    let name = format!("{}_{}.bin", target, blake3::hash(input.as_bytes()).to_hex());
    let path = dir.join(&name);
    let _ = std::fs::write(&path, input);
    format!(
        "PANIC in {} — repro input saved to {} ({} bytes)",
        target,
        path.display(),
        input.len()
    )
}

#[test]
fn fuzz_decode_paths_never_panic() {
    // read side: fuzz tests run in parallel with each other but never
    // overlap a leak-lab slope measurement (we are the loudest allocator
    // in the binary and the lab's byte counter is global)
    let _lab = crate::leak_lab::LAB.read().unwrap();
    let budget = iters();
    let decoder = format!("{}\nlet _ready = 1", DECLS);
    let drifted = format!("{}\nlet _ready = 1", DRIFTED_DECLS);

    let mut rng = Rng(0x0D2_50DA_F00D_0001);
    let mut cases = 0usize;
    let mut findings: Vec<String> = Vec::new();

    // disk corpus: RADTRACK's real artifacts, if this checkout has run them
    let mut disk_corpus: Vec<String> = Vec::new();
    for p in [
        "../../projects/dogfood/radtrack/demo/wa2/world.radw",
        "../../projects/dogfood/radtrack/demo/wa2/base.radw",
        "../../projects/dogfood/radtrack/demo/server_world.radw",
    ] {
        if let Ok(s) = std::fs::read_to_string(p) {
            disk_corpus.push(s);
        }
    }

    // Reconstruct the genuine base fork inside `vm` (values are heap-bound,
    // so this must rerun after every VM rebuild).
    fn decode_base(vm: &mut VM, bytes: &str) -> Value {
        let b = Value::from_string(vm.gc_mut(), bytes.to_string());
        let v = vm
            .call_builtin(Builtin::ForkFromBytes, vec![b])
            .expect("base bytes decode");
        unwrap_ok(&v)
    }

    for seed in 1..=6u64 {
        let corpus = grow_corpus(seed * 0x9E37_79B9);
        let mut vm = fresh_vm(&decoder);
        let mut vm_drift = fresh_vm(&drifted);
        let mut base_fork = decode_base(&mut vm, &corpus.base_bytes);

        let payloads: Vec<&String> = vec![
            &corpus.full,
            &corpus.delta,
            &corpus.save,
            &corpus.base_bytes,
        ];
        for payload in payloads.into_iter().chain(disk_corpus.iter()) {
            for _ in 0..budget {
                let mutant = make_mutant(&mut rng, payload);
                cases += 4;

                // radpack::open — no VM needed, must never panic
                if catch_unwind(|| {
                    let _ = crate::radpack::open(&mutant);
                })
                .is_err()
                {
                    findings.push(report_finding("radpack_open", &mutant));
                }
                if catch_unwind(|| {
                    let _ = crate::radpack::open_file(mutant.as_bytes());
                })
                .is_err()
                {
                    findings.push(report_finding("radpack_open_file", &mutant));
                }

                // fork_from_bytes
                let m = mutant.clone();
                if !must_not_panic(&mut vm, Builtin::ForkFromBytes, |vm| {
                    vec![Value::from_string(vm.gc_mut(), m)]
                }) {
                    findings.push(report_finding("fork_from_bytes", &mutant));
                    vm = fresh_vm(&decoder);
                    base_fork = decode_base(&mut vm, &corpus.base_bytes);
                }

                // fork_apply against the genuine base
                let m = mutant.clone();
                let bf = base_fork;
                if !must_not_panic(&mut vm, Builtin::ForkApply, |vm| {
                    vec![bf, Value::from_string(vm.gc_mut(), m)]
                }) {
                    findings.push(report_finding("fork_apply", &mutant));
                    vm = fresh_vm(&decoder);
                    base_fork = decode_base(&mut vm, &corpus.base_bytes);
                }

                // load_world, against matching AND drifted schemas
                let m = mutant.clone();
                if !must_not_panic(&mut vm, Builtin::LoadWorld, |vm| {
                    vec![Value::from_string(vm.gc_mut(), m)]
                }) {
                    findings.push(report_finding("load_world", &mutant));
                    vm = fresh_vm(&decoder);
                    base_fork = decode_base(&mut vm, &corpus.base_bytes);
                }
                let m = mutant.clone();
                if !must_not_panic(&mut vm_drift, Builtin::LoadWorld, |vm| {
                    vec![Value::from_string(vm.gc_mut(), m)]
                }) {
                    findings.push(report_finding("load_world_drifted", &mutant));
                    vm_drift = fresh_vm(&drifted);
                }

                if findings.len() > 16 {
                    break; // enough evidence; don't drown the report
                }
            }
        }
    }

    assert!(
        findings.is_empty(),
        "fuzzer found {} panic(s) across {} cases:\n{}",
        findings.len(),
        cases,
        findings.join("\n")
    );
    eprintln!("fuzz_decode_paths_never_panic: {} cases, 0 panics", cases);
}

/// The binary boundary: recorded tapes are zstd/deflate envelopes read by
/// `radpack::open_file`. Mutate RADTRACK's real incident tapes (and a
/// synthetic one) at the byte level — header lies, stream corruption,
/// truncation — and require an `Err`, never a panic, from the inflater.
#[test]
fn fuzz_binary_tape_envelope_never_panics() {
    let _lab = crate::leak_lab::LAB.read().unwrap();
    let budget = iters() * 4;
    let mut rng = Rng(0x0D2_50DA_F00D_0003);
    let mut corpora: Vec<Vec<u8>> = Vec::new();

    for p in [
        "../../projects/dogfood/radtrack/demo/incident.radr",
        "../../projects/dogfood/radtrack/demo/incident_v1.radr",
    ] {
        if let Ok(bytes) = std::fs::read(p) {
            corpora.push(bytes);
        }
    }
    // synthetic tape so the test bites even on a fresh checkout
    let body = format!("{{\"frames\":[{}]}}", "1,".repeat(4000) + "1");
    corpora.push(crate::radpack::seal_file("RADTAPE1", &body));

    let mut cases = 0usize;
    let mut findings = 0usize;
    for corpus in &corpora {
        for _ in 0..budget {
            let mutant = mutate_raw(&mut rng, corpus);
            cases += 1;
            if catch_unwind(|| {
                let _ = crate::radpack::open_file(&mutant);
            })
            .is_err()
            {
                findings += 1;
                let dir = std::env::temp_dir().join("rad_fuzz_findings");
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(
                    dir.join(format!("open_file_{}.bin", blake3::hash(&mutant).to_hex())),
                    &mutant,
                );
            }
        }
    }
    assert_eq!(
        findings, 0,
        "open_file panicked on {} of {} byte-mutated tapes (repros in temp/rad_fuzz_findings)",
        findings, cases
    );
    eprintln!(
        "fuzz_binary_tape_envelope_never_panics: {} cases over {} corpora, 0 panics",
        cases,
        corpora.len()
    );
}

/// Semantically poisoned payloads: shapes random mutation almost never
/// synthesizes — allocator lies, duplicate ids, u64-max ids, arity
/// mismatches, name collisions. Each body is sealed with a CORRECT digest
/// (the lie is in the semantics, not the bytes) and fed to both codecs.
/// Anything the decoder *accepts* must then survive re-encoding: a corrupt
/// world that loads but crashes on the next save_world is the same bug
/// with a delay on it.
#[test]
fn fuzz_adversarial_semantic_payloads() {
    let _lab = crate::leak_lab::LAB.read().unwrap();
    // bodies are valid JSON in the real wire shape, schema = Alpha/Beta/Gamma
    let schema = r#"[["Alpha",["n","f","s","b"]],["Beta",["x","label"]],["Gamma",["v"]],["Counter",["total","tag"]]]"#;
    let row = |id: &str, name: &str| {
        format!(
            r#"[{id},{name},[["Alpha",[1,0.5,"s",true]]]]"#,
            id = id,
            name = name
        )
    };
    let body = |entities: String, free_ids: &str, next_id: &str| {
        format!(
            r#"{{"entities":[{entities}],"events":[],"free_ids":{free_ids},"next_id":{next_id},"resources":[],"schema":{schema},"prov":[]}}"#
        )
    };

    let poisons: Vec<(&str, String)> = vec![
        ("duplicate_ids", body(format!("{},{}", row("0", "\"a\""), row("0", "\"b\"")), "[]", "1")),
        ("free_alive_id", body(row("0", "\"a\""), "[0]", "1")),
        ("allocator_behind", body(format!("{},{}", row("5", "\"a\""), row("9", "\"b\"")), "[]", "2")),
        ("u64_max_id", body(row("18446744073709551615", "\"a\""), "[]", "1")),
        ("negative_id", body(row("-1", "\"a\""), "[]", "1")),
        ("float_id", body(row("0.5", "\"a\""), "[]", "1")),
        ("negative_next_id", body(row("0", "\"a\""), "[]", "-7")),
        ("nested_next_id", body(row("0", "\"a\""), "[]", "[[[1]]]")),
        ("name_collision", body(format!("{},{}", row("0", "\"x\""), row("1", "\"x\"")), "[]", "2")),
        ("nul_in_name", body(row("0", "\"a\\u0000b\""), "[]", "1")),
        ("free_ids_flood", body(row("0", "\"a\""), &format!("[{}]", (1..5000).map(|i| i.to_string()).collect::<Vec<_>>().join(",")), "1")),
        ("arity_short", format!(r#"{{"entities":[[0,"a",[["Alpha",[1]]]]],"events":[],"free_ids":[],"next_id":1,"resources":[],"schema":{schema},"prov":[]}}"#)),
        ("arity_long", format!(r#"{{"entities":[[0,"a",[["Alpha",[1,0.5,"s",true,9,9,9]]]]],"events":[],"free_ids":[],"next_id":1,"resources":[],"schema":{schema},"prov":[]}}"#)),
        ("unknown_component", format!(r#"{{"entities":[[0,"a",[["Phantom",[1]]]]],"events":[],"free_ids":[],"next_id":1,"resources":[],"schema":{schema},"prov":[]}}"#)),
        ("event_ghost_entity", format!(r#"{{"entities":[],"events":[["Ping",[999999,1]]],"free_ids":[],"next_id":0,"resources":[],"schema":{schema},"prov":[]}}"#)),
        ("resource_arity", format!(r#"{{"entities":[],"events":[],"free_ids":[],"next_id":0,"resources":[["Counter",[1]]],"schema":{schema},"prov":[]}}"#)),
        ("schema_not_array", r#"{"entities":[],"events":[],"free_ids":[],"next_id":0,"resources":[],"schema":42,"prov":[]}"#.to_string()),
        ("everything_null", r#"{"entities":null,"events":null,"free_ids":null,"next_id":null,"resources":null,"schema":null,"prov":null}"#.to_string()),
    ];

    let decoder = format!("{}\nlet _ready = 1", DECLS);
    let mut vm = fresh_vm(&decoder);
    let mut findings: Vec<String> = Vec::new();
    let mut accepted = 0usize;

    for (label, raw_body) in &poisons {
        for sealed in [reseal("RADFORK2", raw_body), reseal("RADWORLD2", raw_body)] {
            let is_fork = sealed.starts_with("RADFORK2");
            let target: &str = if is_fork {
                "fork_from_bytes"
            } else {
                "load_world"
            };
            let builtin = if is_fork {
                Builtin::ForkFromBytes
            } else {
                Builtin::LoadWorld
            };
            let s = sealed.clone();
            if !must_not_panic(&mut vm, builtin, |vm| {
                vec![Value::from_string(vm.gc_mut(), s)]
            }) {
                findings.push(format!("{} PANIC on poison '{}'", target, label));
                vm = fresh_vm(&decoder);
                continue;
            }
            // If load_world swallowed the poison, re-encoding must not blow up.
            if !is_fork {
                let world_loaded = {
                    // load again, tracking acceptance via the Rust-level Result
                    let v = Value::from_string(vm.gc_mut(), sealed.clone());
                    vm.call_builtin(Builtin::LoadWorld, vec![v]).is_ok()
                };
                if world_loaded {
                    accepted += 1;
                    if !must_not_panic(&mut vm, Builtin::SaveWorld, |_| vec![]) {
                        findings.push(format!(
                            "save_world PANIC after accepting poison '{}'",
                            label
                        ));
                    } else if !must_not_panic(&mut vm, Builtin::Fork, |_| vec![]) {
                        findings.push(format!("fork PANIC after accepting poison '{}'", label));
                    }
                    vm = fresh_vm(&decoder); // clean slate for the next poison
                }
            }
        }
    }

    assert!(
        findings.is_empty(),
        "adversarial corpus found:\n{}",
        findings.join("\n")
    );
    eprintln!(
        "fuzz_adversarial_semantic_payloads: {} poisons x 2 codecs, {} accepted-and-survived-reencode, 0 panics",
        poisons.len(),
        accepted
    );
}

/// Fuzzing the fuzzer: prove the harness actually detects a panic and
/// rebuilds, so a forever-green gate can be trusted.
#[test]
fn fuzz_harness_detects_panics() {
    let mut vm = fresh_vm(&format!("{}\nlet _ready = 1", DECLS));
    let ok = must_not_panic(&mut vm, Builtin::ForkFromBytes, |_| {
        panic!("planted: the harness must catch this")
    });
    assert!(
        !ok,
        "a panic inside the driver must be reported as a finding"
    );
}

/// Mutants that *survive* decoding are well-formed-but-weird forks; pipe
/// them into the merge engine. merge_forks must produce Ok or conflicts —
/// never a panic — even when "theirs" came from a fuzzer.
#[test]
fn fuzz_merge_forks_with_decoded_mutants() {
    let _lab = crate::leak_lab::LAB.read().unwrap();
    let budget = iters();
    let decoder = format!("{}\nlet _ready = 1", DECLS);
    let mut rng = Rng(0x0D2_50DA_F00D_0002);
    let mut cases = 0usize;
    let mut survivors = 0usize;
    let mut findings: Vec<String> = Vec::new();

    fn decode_pair(vm: &mut VM, base_bytes: &str, full: &str) -> (Value, Value) {
        let b = Value::from_string(vm.gc_mut(), base_bytes.to_string());
        let base = vm
            .call_builtin(Builtin::ForkFromBytes, vec![b])
            .expect("base decodes");
        let f = Value::from_string(vm.gc_mut(), full.to_string());
        let ours = vm
            .call_builtin(Builtin::ForkFromBytes, vec![f])
            .expect("full decodes");
        (unwrap_ok(&base), unwrap_ok(&ours))
    }

    for seed in 1..=4u64 {
        let corpus = grow_corpus(seed * 0xC0FF_EE11);
        let mut vm = fresh_vm(&decoder);
        let (mut base_ok, mut ours_ok) = decode_pair(&mut vm, &corpus.base_bytes, &corpus.full);

        for _ in 0..budget {
            let mutant = make_mutant(&mut rng, &corpus.full);
            cases += 1;
            // decode the mutant; most die here (fine), survivors go to merge
            let mv = Value::from_string(vm.gc_mut(), mutant.clone());
            let decoded = match vm.call_builtin(Builtin::ForkFromBytes, vec![mv]) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let Some(theirs) = try_unwrap_ok(&decoded) else {
                continue;
            };
            survivors += 1;
            if !must_not_panic(&mut vm, Builtin::MergeForks, |_| {
                vec![base_ok, ours_ok, theirs]
            }) {
                findings.push(report_finding("merge_forks", &mutant));
                vm = fresh_vm(&decoder);
                let pair = decode_pair(&mut vm, &corpus.base_bytes, &corpus.full);
                base_ok = pair.0;
                ours_ok = pair.1;
            }
            if findings.len() > 8 {
                break;
            }
        }
    }

    assert!(
        findings.is_empty(),
        "merge fuzzer found {} panic(s) across {} cases ({} decoded survivors):\n{}",
        findings.len(),
        cases,
        survivors,
        findings.join("\n")
    );
    eprintln!(
        "fuzz_merge_forks_with_decoded_mutants: {} cases, {} survivors reached merge, 0 panics",
        cases, survivors
    );
}

/// `Result`/`Option` sums come back from builtins as sum values; the merge
/// and codec fuzz drivers need the payload out of `Ok(...)`.
fn unwrap_ok(v: &Value) -> Value {
    try_unwrap_ok(v).expect("expected Ok(...)")
}

fn try_unwrap_ok(v: &Value) -> Option<Value> {
    let st = v.as_sum_type()?;
    if st.variant == "Ok" {
        st.fields.get("value").copied()
    } else {
        None
    }
}

/// The gc_pause class of bug, as a standing stress test: with the collector
/// firing at EVERY allocation, run the codec + merge + simulate machinery
/// under pressure. Any builtin holding unrooted Values across a nested rad
/// execution dies here, deterministically, instead of 1-in-3 in a browser.
#[test]
fn fuzz_gc_pressure_on_codec_and_merge() {
    let src = format!(
        r#"{decls}
rand_seed(99)
for i in range(0, 12) {{
    let e = spawn(f"g{{i}}", Alpha {{ n: i, f: 0.5, s: "pressure-κ", b: true }})
    set(e, Beta {{ x: i, label: "λ" }})
}}
set_resource(Counter, Counter {{ total: 1, tag: "gc" }})
let base = fork()
let bytes = fork_to_bytes(base)
for e in entities(Alpha) {{
    update(e, Alpha) {{ n = 777 }}
}}
emit Ping {{ who: get_entity("g3"), amount: 1 }}
let delta = fork_delta(base, fork())
let ours = fork()
flush_events()
match fork_from_bytes(bytes) {{
    Ok(remote) => {{
        match fork_apply(remote, delta) {{
            Ok(theirs) => {{
                match merge_forks(base, ours, theirs) {{
                    Ok(m) => {{ commit(m) }}
                    Err(cs) => {{ print(f"conflicts: {{len(cs)}}") }}
                }}
            }}
            Err(m) => {{ print(m) }}
        }}
    }}
    Err(m) => {{ print(m) }}
}}
let n = load_world(save_world())
print(f"alive {{n}}")
"#,
        decls = DECLS
    );
    let mut vm = VM::new();
    vm.suppress_output();
    vm.set_random_seed(99);
    // every allocation is a collection point: unrooted Rust locals get swept
    vm.gc.set_collect_threshold_for_test(0);
    vm.load_compile_result(compile(&src));
    vm.run(0)
        .expect("codec + merge under GC pressure must survive");
    let last = vm.print_buffer.last().cloned().unwrap_or_default();
    assert!(last.starts_with("alive "), "got: {:?}", vm.print_buffer);
}

//! Regressions for the first two gaps a search program runs into: a visited
//! set kept in a map (GH 0006) and ordering two strings (GH 0007). Both were
//! found by `projects/dogfood/andrews-curtis`.

use crate::vm::VM;

fn run(src: &str) -> VM {
    let compiled = crate::test_support::compile_checked_source(
        src,
        crate::parser::ParserOptions,
        crate::checker::CheckerOptions::default(),
    )
    .expect("parse, check, and compile");
    let mut vm = VM::new();
    vm.suppress_output();
    vm.load_compile_result(compiled);
    vm.run(0).expect("run");
    vm
}

const INSERTS: u64 = 4000;

fn visited_set(inserts: u64) -> String {
    format!(
        r#"
fn main() -> nil {{
    let mut seen = {{}}
    let mut k = 0
    while k < {inserts} {{
        seen[f"state-{{k}}"] = k
        k = k + 1
    }}
    print(len(seen))
}}
"#
    )
}

#[test]
fn a_map_insert_is_charged_for_its_entry_not_for_the_whole_map() {
    let vm = run(&visited_set(INSERTS));
    assert_eq!(vm.print_buffer, vec![INSERTS.to_string()]);
    // Each insert allocates one map object. Charging it the whole map, as
    // before, costs about 30 bytes times the entries already present, which
    // averages 60 KB an insert here and 240 MB in total. Charging the entry
    // and the copied trie path is under 1 KB an insert, key string included.
    let per_insert = vm.gc.total_allocated_bytes() / INSERTS;
    assert!(per_insert < 2048, "{per_insert} bytes charged per insert");
}

#[test]
fn a_surviving_map_is_priced_in_full_at_the_next_collection() {
    // A global keeps the map alive across the collection.
    let mut vm = run(&format!(
        r#"
let mut seen = {{}}
let mut k = 0
while k < {INSERTS} {{
    seen[f"state-{{k}}"] = k
    k = k + 1
}}
let _ = gc_collect()
print(len(seen))
"#
    ));
    assert_eq!(vm.print_buffer, vec![INSERTS.to_string()]);
    // 4000 entries: a key of at least 7 bytes and one value word each.
    let floor = (INSERTS as usize) * (7 + std::mem::size_of::<crate::value::Value>());
    assert!(
        vm.gc.bytes_allocated() >= floor,
        "live map priced at {} bytes, below its {} byte floor",
        vm.gc.bytes_allocated(),
        floor
    );
    // A second collection finds nothing derived and changes nothing.
    let settled = vm.gc.bytes_allocated();
    vm.collect_cycles();
    assert_eq!(vm.gc.bytes_allocated(), settled);
}

#[test]
fn strings_order_by_byte() {
    let vm = run(r#"
fn least(a: str, b: str) -> str {
    if a < b { return a }
    return b
}
print(least("XYxy", "XYXy"))
print("ab" <= "ab")
print("b" > "ab")
print("" >= "a")
"#);
    assert_eq!(vm.print_buffer, vec!["XYXy", "true", "true", "false"]);
}

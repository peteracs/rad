use crate::gc::GcHeap;
use crate::value::{Builtin, MapKey, MapStorage, Value};
use crate::vm::builtins_impl::*;
use crate::vm::VM;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn make_comp(gc: &mut GcHeap, name: &str, fields: HashMap<String, Value>) -> Value {
    let mut layout = Vec::new();
    let mut values = Vec::new();
    for (k, v) in fields {
        layout.push(k);
        values.push(v);
    }
    Value::component(gc, name.to_string(), Arc::new(layout), values)
}

/// Build a list without holding `&mut GcHeap` across nested `Value::…` calls (avoids E0499 in tests).
fn list_of(gc: &mut GcHeap, f: impl FnOnce(&mut GcHeap) -> Vec<Value>) -> Value {
    let items = f(gc);
    Value::list(gc, items)
}

#[test]
fn pop_returns_last_element() {
    let mut gc = GcHeap::default();
    let input = list_of(&mut gc, |gc| {
        vec![
            Value::from_int(gc, 1),
            Value::from_int(gc, 2),
            Value::from_int(gc, 3),
        ]
    });
    let out = bi_pop(&mut gc, vec![input]).expect("pop should succeed");
    assert_eq!(out, Value::from_int(&mut gc, 3));
}

#[test]
fn pop_single_element() {
    let mut gc = GcHeap::default();
    let input = list_of(&mut gc, |gc| vec![Value::from_int(gc, 42)]);
    let out = bi_pop(&mut gc, vec![input]).expect("pop should succeed");
    assert_eq!(out, Value::from_int(&mut gc, 42));
}

#[test]
fn pop_rejects_empty_list() {
    let mut gc = GcHeap::default();
    let input = Value::list(&mut gc, vec![]);
    let err = bi_pop(&mut gc, vec![input]).expect_err("empty pop must fail");
    assert_eq!(err, "pop() on empty list");
}

#[test]
fn push_returns_extended_list() {
    let mut gc = GcHeap::default();
    let input = list_of(&mut gc, |gc| vec![Value::from_int(gc, 1)]);
    let two = Value::from_int(&mut gc, 2);
    let out = bi_push(&mut gc, vec![input, two]).expect("push should succeed");
    assert_eq!(
        out,
        list_of(&mut gc, |gc| vec![
            Value::from_int(gc, 1),
            Value::from_int(gc, 2)
        ])
    );
}

#[test]
fn push_does_not_mutate_original() {
    let mut gc = GcHeap::default();
    let input = list_of(&mut gc, |gc| vec![Value::from_int(gc, 1)]);
    let alias = input;
    let two = Value::from_int(&mut gc, 2);
    let out = bi_push(&mut gc, vec![input, two]).expect("push should succeed");
    assert_eq!(alias.as_list().unwrap().len(), 1);
    assert_eq!(out.as_list().unwrap().len(), 2);
}

#[test]
fn merge_produces_combined_map() {
    let mut gc = GcHeap::default();
    let mut base = MapStorage::new();
    base.insert(
        MapKey::Str("users".to_string()),
        Value::list(&mut gc, vec![]),
    );
    let left = Value::map(&mut gc, base);

    let mut patch = MapStorage::new();
    patch.insert(
        MapKey::Str("count".to_string()),
        Value::from_int(&mut gc, 1),
    );
    let patch_map = Value::map(&mut gc, patch);
    let out = bi_merge(&mut gc, vec![left, patch_map]).expect("merge should succeed");
    assert_eq!(out.as_map().unwrap().len(), 2);
}

#[test]
fn input_rejects_more_than_one_argument() {
    let mut vm = VM::new();
    let a = Value::from_string(&mut vm.gc, "a".into());
    let b = Value::from_string(&mut vm.gc, "b".into());
    let err = vm
        .call_builtin(Builtin::Input, vec![a, b])
        .expect_err("input should reject extra args");
    assert_eq!(err, "input() accepts at most 1 argument");
}

#[test]
fn readline_rejects_arguments() {
    let mut vm = VM::new();
    let prompt = Value::from_string(&mut vm.gc, "prompt".into());
    let err = vm
        .call_builtin(Builtin::Readline, vec![prompt])
        .expect_err("readline should reject args");
    assert_eq!(err, "readline() takes no arguments");
}

#[test]
fn regex_helpers_work() {
    let mut gc = GcHeap::default();
    let re_pat = Value::from_string(&mut gc, "^a+$".to_string());
    let re_hay = Value::from_string(&mut gc, "aaa".to_string());
    let matched =
        bi_regex_is_match(&mut gc, vec![re_pat, re_hay]).expect("regex_is_match should succeed");
    assert_eq!(matched, Value::TRUE);

    let find_pat = Value::from_string(&mut gc, "a+".to_string());
    let find_hay = Value::from_string(&mut gc, "xxaaayy".to_string());
    let found =
        bi_regex_find(&mut gc, vec![find_pat, find_hay]).expect("regex_find should succeed");
    let st = found
        .as_sum_type()
        .expect("regex_find should return Option");
    assert_eq!(st.variant, "Some");
    assert_eq!(st.fields.get("value").and_then(|v| v.as_str()), Some("aaa"));
}

#[test]
fn regex_invalid_pattern_fails() {
    let mut gc = GcHeap::default();
    let g = &mut gc;
    let bad = Value::from_string(g, "(".to_string());
    let text = Value::from_string(g, "abc".to_string());
    let err = bi_regex_is_match(&mut gc, vec![bad, text]).expect_err("invalid regex should fail");
    assert!(err.contains("invalid pattern"));
}

#[test]
fn require_returns_component_when_present() {
    let mut vm = VM::new();
    let eid = vm
        .call_builtin(Builtin::Spawn, vec![])
        .expect("spawn should succeed");
    let score = {
        let g = &mut vm.gc;
        let mut fields = HashMap::new();
        fields.insert("points".to_string(), Value::from_int(g, 7));
        make_comp(g, "Score", fields)
    };
    vm.call_builtin(Builtin::Set, vec![eid, score])
        .expect("set should succeed");

    let name = Value::from_string(&mut vm.gc, "Score".to_string());
    let out = vm
        .call_builtin(Builtin::Require, vec![eid, name])
        .expect("require should succeed");
    let comp = out.as_component().expect("require returns component");
    assert_eq!(comp.type_name, "Score");
    let idx = comp.layout.iter().position(|n| n == "points").unwrap();
    let exp = Value::from_int(&mut vm.gc, 7);
    assert_eq!(comp.values.get(idx), Some(&exp));
}

#[test]
fn require_errors_when_missing() {
    let mut vm = VM::new();
    let eid = vm
        .call_builtin(Builtin::Spawn, vec![])
        .expect("spawn should succeed");
    let missing = Value::from_string(&mut vm.gc, "Missing".to_string());
    let err = vm
        .call_builtin(Builtin::Require, vec![eid, missing])
        .expect_err("require should fail for missing component");
    assert!(err.contains("missing component"));
}

#[test]
fn require_all_returns_components_in_order() {
    let mut vm = VM::new();
    let eid = vm
        .call_builtin(Builtin::Spawn, vec![])
        .expect("spawn should succeed");
    let comp_a = make_comp(&mut vm.gc, "A", HashMap::new());
    vm.call_builtin(Builtin::Set, vec![eid, comp_a])
        .expect("set A should succeed");
    let comp_b = make_comp(&mut vm.gc, "B", HashMap::new());
    vm.call_builtin(Builtin::Set, vec![eid, comp_b])
        .expect("set B should succeed");

    let name_b = Value::from_string(&mut vm.gc, "B".to_string());
    let name_a = Value::from_string(&mut vm.gc, "A".to_string());
    let out = vm
        .call_builtin(Builtin::RequireAll, vec![eid, name_b, name_a])
        .expect("require_all should succeed");
    let list = out.as_list().expect("require_all returns list");
    assert_eq!(list.len(), 2);
    assert_eq!(
        list.get(0)
            .unwrap()
            .as_component()
            .map(|c| c.type_name.as_str()),
        Some("B")
    );
    assert_eq!(
        list.get(1)
            .unwrap()
            .as_component()
            .map(|c| c.type_name.as_str()),
        Some("A")
    );
}

#[test]
fn entities_filters_by_all_requested_components() {
    let mut vm = VM::new();
    let e1 = vm
        .call_builtin(Builtin::Spawn, vec![])
        .expect("spawn e1 should succeed");
    let e2 = vm
        .call_builtin(Builtin::Spawn, vec![])
        .expect("spawn e2 should succeed");
    let c1a = make_comp(&mut vm.gc, "A", HashMap::new());
    vm.call_builtin(Builtin::Set, vec![e1, c1a])
        .expect("set e1 A should succeed");
    let c1b = make_comp(&mut vm.gc, "B", HashMap::new());
    vm.call_builtin(Builtin::Set, vec![e1, c1b])
        .expect("set e1 B should succeed");
    let c2a = make_comp(&mut vm.gc, "A", HashMap::new());
    vm.call_builtin(Builtin::Set, vec![e2, c2a])
        .expect("set e2 A should succeed");

    let name_a = Value::from_string(&mut vm.gc, "A".to_string());
    let name_b = Value::from_string(&mut vm.gc, "B".to_string());
    let out = vm
        .call_builtin(Builtin::Entities, vec![name_a, name_b])
        .expect("entities() should succeed");
    let ids = out.as_list().expect("entities() returns list");
    assert_eq!(ids.len(), 1);
    assert_eq!(ids.get(0).unwrap(), &e1);
}

#[test]
fn map_or_maps_some_and_uses_default_for_none() {
    let mut vm = VM::new();
    let i41 = Value::from_int(&mut vm.gc, 41);
    let some = wrap_option(&mut vm.gc, Some(i41));
    let none_str = Value::from_string(&mut vm.gc, "none".to_string());
    let str_fn = Value::from_builtin(&mut vm.gc, Builtin::Str);
    let mapped = vm
        .call_builtin(Builtin::MapOr, vec![some, none_str, str_fn])
        .expect("map_or some should succeed");
    assert_eq!(mapped.as_str(), Some("41"));

    let none = wrap_option(&mut vm.gc, None);
    let none_str2 = Value::from_string(&mut vm.gc, "none".to_string());
    let str_fn2 = Value::from_builtin(&mut vm.gc, Builtin::Str);
    let fallback = vm
        .call_builtin(Builtin::MapOr, vec![none, none_str2, str_fn2])
        .expect("map_or none should succeed");
    assert_eq!(fallback.as_str(), Some("none"));
}

#[test]
fn file_builtins_round_trip() {
    let mut vm = VM::new();
    let mut path = std::env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be monotonic enough for temp naming")
        .as_nanos();
    path.push(format!(
        "rad_vm_builtin_{}_{}.txt",
        std::process::id(),
        stamp
    ));
    let path_str = path.to_string_lossy().to_string();

    let path_val = Value::from_string(&mut vm.gc, path_str.clone());
    let body_val = Value::from_string(&mut vm.gc, "hello file".to_string());
    vm.call_builtin(Builtin::WriteFile, vec![path_val, body_val])
        .expect("write_file should succeed");

    let path_read = Value::from_string(&mut vm.gc, path_str);
    let content = vm
        .call_builtin(Builtin::ReadFile, vec![path_read])
        .expect("read_file should succeed");
    assert_eq!(content.as_str(), Some("hello file"));

    let _ = std::fs::remove_file(path);
}

#[test]
fn http_get_rejects_invalid_url() {
    let mut vm = VM::new();
    let bad_url = Value::from_string(&mut vm.gc, "not a url".to_string());
    let err = vm
        .call_builtin(Builtin::HttpGet, vec![bad_url])
        .expect_err("http_get should fail for invalid URL");
    assert!(err.contains("http_get() request failed"));
}

#[test]
fn now_unix_builtins_return_ints() {
    let mut gc = GcHeap::default();
    let ms = bi_now_unix_ms(&mut gc, vec![]).expect("now_unix_ms should succeed");
    let s = bi_now_unix_s(&mut gc, vec![]).expect("now_unix_s should succeed");
    let ms_v = ms.as_int().expect("now_unix_ms returns int");
    let s_v = s.as_int().expect("now_unix_s returns int");
    assert!(ms_v > 0);
    assert!(s_v > 0);
    assert!(ms_v >= s_v * 1000);
}

#[test]
fn random_seed_makes_stream_deterministic() {
    let mut vm_a = VM::new();
    let mut vm_b = VM::new();
    let seed_a = Value::from_int(&mut vm_a.gc, 12345);
    vm_a.call_builtin(Builtin::RandSeed, vec![seed_a])
        .expect("seed must succeed");
    let seed_b = Value::from_int(&mut vm_b.gc, 12345);
    vm_b.call_builtin(Builtin::RandSeed, vec![seed_b])
        .expect("seed must succeed");
    let a_lo = Value::from_int(&mut vm_a.gc, 1);
    let a_hi = Value::from_int(&mut vm_a.gc, 100);
    let a1 = vm_a
        .call_builtin(Builtin::RandInt, vec![a_lo, a_hi])
        .expect("rand_int should succeed");
    let b_lo = Value::from_int(&mut vm_b.gc, 1);
    let b_hi = Value::from_int(&mut vm_b.gc, 100);
    let b1 = vm_b
        .call_builtin(Builtin::RandInt, vec![b_lo, b_hi])
        .expect("rand_int should succeed");
    let a2 = vm_a
        .call_builtin(Builtin::RandBool, vec![])
        .expect("rand_bool should succeed");
    let b2 = vm_b
        .call_builtin(Builtin::RandBool, vec![])
        .expect("rand_bool should succeed");
    let a3 = vm_a
        .call_builtin(Builtin::RandFloat, vec![])
        .expect("rand_float should succeed");
    let b3 = vm_b
        .call_builtin(Builtin::RandFloat, vec![])
        .expect("rand_float should succeed");
    assert_eq!(a1, b1);
    assert_eq!(a2, b2);
    assert_eq!(a3, b3);
}

#[test]
fn rand_int_rejects_invalid_range() {
    let mut vm = VM::new();
    let lo = Value::from_int(&mut vm.gc, 10);
    let hi = Value::from_int(&mut vm.gc, 2);
    let err = vm
        .call_builtin(Builtin::RandInt, vec![lo, hi])
        .expect_err("rand_int should reject inverted bounds");
    assert!(err.contains("min <= max"));
}

#[test]
fn rand_float_is_in_unit_interval() {
    let mut vm = VM::new();
    let out = vm
        .call_builtin(Builtin::RandFloat, vec![])
        .expect("rand_float should succeed");
    let n = out.as_float().expect("rand_float should return float");
    assert!((0.0..1.0).contains(&n));
}

#[test]
fn pop_last_returns_only_removed_element() {
    let mut gc = GcHeap::default();
    let input = list_of(&mut gc, |gc| {
        vec![
            Value::from_int(gc, 1),
            Value::from_int(gc, 2),
            Value::from_int(gc, 3),
        ]
    });
    let out = bi_pop_last(&mut gc, vec![input]).expect("pop_last should succeed");
    assert_eq!(out, Value::from_int(&mut gc, 3));
}

#[test]
fn drop_last_returns_remaining_list() {
    let mut gc = GcHeap::default();
    let input = list_of(&mut gc, |gc| {
        vec![
            Value::from_int(gc, 1),
            Value::from_int(gc, 2),
            Value::from_int(gc, 3),
        ]
    });
    let out = bi_drop_last(&mut gc, vec![input]).expect("drop_last should succeed");
    assert_eq!(
        out,
        list_of(&mut gc, |gc| vec![
            Value::from_int(gc, 1),
            Value::from_int(gc, 2)
        ])
    );
}

#[test]
fn split_basic() {
    let mut gc = GcHeap::default();
    let hay = Value::from_string(&mut gc, "a,b,c".into());
    let delim = Value::from_string(&mut gc, ",".into());
    let out = bi_split(&mut gc, vec![hay, delim]).unwrap();
    assert_eq!(
        out,
        list_of(&mut gc, |gc| {
            vec![
                Value::from_string(gc, "a".into()),
                Value::from_string(gc, "b".into()),
                Value::from_string(gc, "c".into()),
            ]
        })
    );
}

#[test]
fn join_basic() {
    let mut gc = GcHeap::default();
    let list_arg = list_of(&mut gc, |gc| {
        vec![
            Value::from_string(gc, "x".into()),
            Value::from_string(gc, "y".into()),
        ]
    });
    let sep = Value::from_string(&mut gc, "-".into());
    let out = bi_join(&mut gc, vec![list_arg, sep]).unwrap();
    assert_eq!(out, Value::from_string(&mut gc, "x-y".into()));
}

#[test]
fn trim_strips_whitespace() {
    let mut gc = GcHeap::default();
    let s = Value::from_string(&mut gc, "  hi  ".into());
    let out = bi_trim(&mut gc, vec![s]).unwrap();
    assert_eq!(out, Value::from_string(&mut gc, "hi".into()));
}

#[test]
fn replace_all_occurrences() {
    let mut gc = GcHeap::default();
    let body = Value::from_string(&mut gc, "aaa".into());
    let from = Value::from_string(&mut gc, "a".into());
    let to = Value::from_string(&mut gc, "b".into());
    let out = bi_replace(&mut gc, vec![body, from, to]).unwrap();
    assert_eq!(out, Value::from_string(&mut gc, "bbb".into()));
}

#[test]
fn starts_with_and_ends_with() {
    let mut gc = GcHeap::default();
    let hello1 = Value::from_string(&mut gc, "hello".into());
    let hel = Value::from_string(&mut gc, "hel".into());
    let sw = bi_starts_with(&mut gc, vec![hello1, hel]).unwrap();
    assert_eq!(sw, Value::TRUE);

    let hello2 = Value::from_string(&mut gc, "hello".into());
    let llo = Value::from_string(&mut gc, "llo".into());
    let ew = bi_ends_with(&mut gc, vec![hello2, llo]).unwrap();
    assert_eq!(ew, Value::TRUE);
}

#[test]
fn append_concatenates_lists() {
    let mut gc = GcHeap::default();
    let left = list_of(&mut gc, |gc| vec![Value::from_int(gc, 1)]);
    let right = list_of(&mut gc, |gc| {
        vec![Value::from_int(gc, 2), Value::from_int(gc, 3)]
    });
    let out = bi_append(&mut gc, vec![left, right]).unwrap();
    assert_eq!(
        out,
        list_of(&mut gc, |gc| {
            vec![
                Value::from_int(gc, 1),
                Value::from_int(gc, 2),
                Value::from_int(gc, 3),
            ]
        })
    );
}

#[test]
fn zip_pairs_elements() {
    let mut gc = GcHeap::default();
    let nums = list_of(&mut gc, |gc| {
        vec![Value::from_int(gc, 1), Value::from_int(gc, 2)]
    });
    let strs = list_of(&mut gc, |gc| {
        vec![
            Value::from_string(gc, "a".into()),
            Value::from_string(gc, "b".into()),
        ]
    });
    let out = bi_zip(&mut gc, vec![nums, strs]).unwrap();
    let pair0 = list_of(&mut gc, |gc| {
        vec![Value::from_int(gc, 1), Value::from_string(gc, "a".into())]
    });
    let pair1 = list_of(&mut gc, |gc| {
        vec![Value::from_int(gc, 2), Value::from_string(gc, "b".into())]
    });
    assert_eq!(out, Value::list(&mut gc, vec![pair0, pair1]));
}

#[test]
fn zip_truncates_to_shorter() {
    let mut gc = GcHeap::default();
    let short = list_of(&mut gc, |gc| vec![Value::from_int(gc, 1)]);
    let long = list_of(&mut gc, |gc| {
        vec![Value::from_int(gc, 2), Value::from_int(gc, 3)]
    });
    let out = bi_zip(&mut gc, vec![short, long]).unwrap();
    let inner = list_of(&mut gc, |gc| {
        vec![Value::from_int(gc, 1), Value::from_int(gc, 2)]
    });
    assert_eq!(out, Value::list(&mut gc, vec![inner]));
}

#[test]
fn try_int_success_and_failure() {
    let mut gc = GcHeap::default();
    let s42 = Value::from_string(&mut gc, "42".into());
    let ok = bi_try_int(&mut gc, vec![s42]).unwrap();
    assert!(ok.as_sum_type().is_some());
    let st = ok.as_sum_type().unwrap();
    assert_eq!(st.variant, "Some");
    assert_eq!(st.fields.get("value").unwrap().as_int(), Some(42));

    let snope = Value::from_string(&mut gc, "nope".into());
    let fail = bi_try_int(&mut gc, vec![snope]).unwrap();
    let st = fail.as_sum_type().unwrap();
    assert_eq!(st.variant, "None");
}

#[test]
fn try_float_success_and_failure() {
    let mut gc = GcHeap::default();
    let sf = Value::from_string(&mut gc, "3.14".into());
    let ok = bi_try_float(&mut gc, vec![sf]).unwrap();
    let st = ok.as_sum_type().unwrap();
    assert_eq!(st.variant, "Some");

    let snope = Value::from_string(&mut gc, "nope".into());
    let fail = bi_try_float(&mut gc, vec![snope]).unwrap();
    let st = fail.as_sum_type().unwrap();
    assert_eq!(st.variant, "None");
}

//! This integration test is a separate crate on purpose: it can use only the
//! safe public embedding surface, never `pub(crate)` VM representation APIs.

use rad_vm::host_value::{FrozenMapKey, FrozenValue};
use rad_vm::vm::VM;

#[test]
fn owned_values_survive_the_source_vm_and_import_into_another_vm() {
    let exported = {
        let mut source = VM::new_with_seed(7);
        let value = FrozenValue::Map(vec![
            (
                FrozenMapKey::String("name".into()),
                FrozenValue::String("hero".into()),
            ),
            (
                FrozenMapKey::String("stats".into()),
                FrozenValue::List(vec![FrozenValue::Int(100), FrozenValue::Float(3.5)]),
            ),
        ]);
        source
            .import_value(&value)
            .expect("safe import")
            .to_owned()
            .expect("safe export")
    }; // source VM and its heap are gone

    let mut destination = VM::new_with_seed(7);
    let round_trip = destination
        .import_value(&exported)
        .expect("owned values have no source-heap pointer")
        .to_owned()
        .expect("destination export");
    assert_eq!(round_trip, exported);
}

#[test]
fn copied_host_values_do_not_create_mutable_vm_aliases() {
    let original = FrozenValue::Buffer("seed".into());
    let mut copy = original.clone();
    let FrozenValue::Buffer(buffer) = &mut copy else {
        panic!("buffer expected");
    };
    buffer.push_str("-changed");
    assert_eq!(original, FrozenValue::Buffer("seed".into()));

    let mut vm = VM::new_with_seed(7);
    let imported = vm
        .import_value(&copy)
        .expect("import")
        .to_owned()
        .expect("export");
    assert_eq!(imported, FrozenValue::Buffer("seed-changed".into()));
}

#[test]
fn adversarial_nan_payloads_never_become_host_objects() {
    for bits in [
        0x7FF0_0000_0000_0001,
        0x7FFC_0000_0000_0000,
        0x7FFF_FFFF_FFFF_FFFF,
        0xFFF0_0000_0000_0001,
        0xFFFC_0000_0000_0000,
        0xFFFF_FFFF_FFFF_FFFF,
    ] {
        let mut vm = VM::new_with_seed(7);
        let exported = vm
            .import_value(&FrozenValue::Float(f64::from_bits(bits)))
            .expect("NaN is a float")
            .to_owned()
            .expect("NaN export");
        assert!(matches!(exported, FrozenValue::Float(value) if value.is_nan()));
    }
}

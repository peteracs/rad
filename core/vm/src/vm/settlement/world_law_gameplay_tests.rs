const WORLD_LAW_RPG_SOURCE: &str = include_str!(
    "../../../../../projects/dogfood/world-law-rpg/main.rad"
);
const WORLD_LAW_RPG_RELATIONS: &str = include_str!(
    "../../../../../projects/dogfood/world-law-rpg/relations.rad"
);

fn dogfood_int(vm: &mut VM, function: &str) -> i64 {
    match vm.call_global(function, &[]).expect("dogfood command succeeds") {
        crate::host_value::FrozenValue::Int(value) => value,
        value => panic!("{function} returned {value:?}, expected int"),
    }
}

fn dogfood_bool(vm: &mut VM, function: &str) -> bool {
    match vm.call_global(function, &[]).expect("dogfood command succeeds") {
        crate::host_value::FrozenValue::Bool(value) => value,
        value => panic!("{function} returned {value:?}, expected bool"),
    }
}

fn dogfood_text(vm: &mut VM, function: &str) -> String {
    match vm.call_global(function, &[]).expect("dogfood command succeeds") {
        crate::host_value::FrozenValue::String(value) => value,
        value => panic!("{function} returned {value:?}, expected text"),
    }
}

fn dogfood_fact(
    relation: &str,
    tuple: Vec<crate::relation_runtime::FactValue>,
) -> crate::relation_runtime::FactKey {
    crate::relation_runtime::FactKey::new(format!("game::worldlaw::{relation}"), tuple)
}

#[test]
fn world_law_rpg_runs_the_complete_authoritative_derived_candidate() {
    use crate::constraint_types::{SettlementAttemptOutcome, VmFailure};
    use crate::relation_frontend::{compile, FrontendOptions};
    use crate::relation_runtime::{
        FactValue, OperationMetadata, PendingDespawn, RelationRuntimeManifest,
        RelationTransaction,
    };

    let mut vm = crate::causal_laws_tests::compile_vm(WORLD_LAW_RPG_SOURCE);
    vm.run(0).expect("initialize headless gameplay world");
    let artifacts = compile(
        WORLD_LAW_RPG_RELATIONS,
        &FrontendOptions {
            enabled: true,
            module_id: "game::worldlaw".into(),
            ..FrontendOptions::default()
        },
    )
    .expect("compile gameplay relations");
    let manifest = RelationRuntimeManifest::from_frontend(&artifacts)
        .expect("seal gameplay relation manifest");
    assert!(!manifest.rules().is_empty());
    vm.install_relation_frontend(&artifacts)
        .expect("install gameplay relations");

    vm.call_global_detailed("configure_world", &[])
        .expect("configure actors and threat through resolvers");
    vm.call_global_detailed("equip_anvil", &[])
        .expect("equip through resolver-owned facts");

    let hero = vm.world.get_entity_by_name("hero").unwrap();
    let merchant = vm.world.get_entity_by_name("merchant").unwrap();
    let anvil = vm.world.get_entity_by_name("anvil").unwrap();
    let hero_ref = vm.world.entity_ref(hero).unwrap();
    let merchant_ref = vm.world.entity_ref(merchant).unwrap();
    let anvil_ref = vm.world.entity_ref(anvil).unwrap();
    let encumbered = dogfood_fact("Encumbered", vec![FactValue::Entity(hero_ref)]);
    let danger = dogfood_fact("InDanger", vec![FactValue::Entity(hero_ref)]);
    assert!(vm.world.derived_relation_state().facts().contains_key(&encumbered));
    assert!(vm.world.derived_relation_state().facts().contains_key(&danger));

    let before_rejection = vm.observable_state_signature();
    let SettlementAttemptOutcome::Rejected(recorded_move) = vm
        .call_global_attempt("move_hero", &[])
        .expect("record encumbered movement rejection")
    else {
        panic!("encumbered movement must reject")
    };
    assert_eq!(
        recorded_move.rejection.violations[0].code,
        "movement.encumbered"
    );
    assert_eq!(before_rejection, vm.observable_state_signature());
    vm.replay_failed_attempt(&recorded_move)
        .expect("observational replay reproduces rejection");
    assert_eq!(before_rejection, vm.observable_state_signature());
    vm.replay_portable_failed_attempt(&recorded_move.portable_recipe())
        .expect("portable replay binds the same relation-aware checkpoint");
    assert_eq!(before_rejection, vm.observable_state_signature());

    vm.call_global_detailed("trade_anvil", &[])
        .expect("trade uses unique-key replacement");
    assert!(!vm.world.derived_relation_state().facts().contains_key(&encumbered));
    let merchant_owns = dogfood_fact(
        "Owns",
        vec![FactValue::Entity(merchant_ref), FactValue::Entity(anvil_ref)],
    );
    let merchant_weight = dogfood_fact(
        "TotalWeight",
        vec![FactValue::Entity(merchant_ref), FactValue::Int(40)],
    );
    assert!(vm.world.relation_state().assertions().contains_key(&merchant_owns));
    assert!(vm
        .world
        .derived_relation_state()
        .facts()
        .contains_key(&merchant_weight));
    vm.call_global_detailed("move_hero", &[])
        .expect("unencumbered movement commits");
    assert_eq!(dogfood_int(&mut vm, "hero_x"), 3);

    vm.call_global_detailed("root_hero", &[]).unwrap();
    let rooted_before = vm.observable_state_signature();
    let error = vm.call_global_detailed("move_hero", &[]).unwrap_err();
    assert!(matches!(
        error,
        VmFailure::SettlementRejected(ref rejection)
            if rejection.violations[0].code == "movement.rooted"
    ));
    assert_eq!(rooted_before, vm.observable_state_signature());
    vm.call_global_detailed("unroot_hero", &[]).unwrap();

    vm.call_global_detailed("silence_hero", &[]).unwrap();
    let silence_before = vm.observable_state_signature();
    let error = vm.call_global_detailed("cast_hero", &[]).unwrap_err();
    assert!(matches!(
        error,
        VmFailure::SettlementRejected(ref rejection)
            if rejection.violations[0].code == "casting.silenced"
    ));
    assert_eq!(silence_before, vm.observable_state_signature());
    vm.call_global_detailed("unsilence_hero", &[]).unwrap();
    vm.call_global_detailed("cast_hero", &[])
        .expect("unsilenced cast commits");
    assert_eq!(dogfood_int(&mut vm, "hero_mana"), 13);

    vm.call_global_detailed("shield_hero", &[]).unwrap();
    let shield_before = vm.observable_state_signature();
    let error = vm.call_global_detailed("strike_hero", &[]).unwrap_err();
    assert!(matches!(
        error,
        VmFailure::SettlementRejected(ref rejection)
            if rejection.violations[0].code == "combat.shielded"
    ));
    assert_eq!(shield_before, vm.observable_state_signature());
    vm.call_global_detailed("unshield_hero", &[]).unwrap();
    assert_eq!(dogfood_text(&mut vm, "bot_turn"), "attack");
    assert_eq!(dogfood_int(&mut vm, "hero_hp"), 80);
    assert_eq!(dogfood_text(&mut vm, "goblin_order"), "wait");

    let explanation = vm
        .call_global("explain_danger", &[])
        .expect("danger explanation renders");
    let crate::host_value::FrozenValue::String(explanation) = explanation else {
        panic!("why_fact must return text")
    };
    for expected in [
        "game::worldlaw::InDanger",
        "game::worldlaw::IncomingDamage",
        "game::worldlaw::Hostile",
        "game::worldlaw::AttackPower",
        "resolver `ResolveThreat`",
        "law `Threat`",
    ] {
        assert!(explanation.contains(expected), "missing {expected}:\n{explanation}");
    }
    assert!(dogfood_bool(&mut vm, "persistence_roundtrip"));
    assert!(vm.world.derived_relation_state().facts().contains_key(&danger));
    assert_eq!(dogfood_text(&mut vm, "explain_danger"), explanation);

    vm.world
        .apply_relation_transaction(&RelationTransaction {
            despawns: vec![PendingDespawn {
                entity: anvil_ref,
                metadata: OperationMetadata::cause("dogfood.item_destroyed"),
            }],
            ..RelationTransaction::default()
        })
        .expect("item lifecycle cascades through authoritative facts");
    assert!(!vm.world.entity_exists(anvil));
    assert!(!vm.world.relation_state().assertions().contains_key(&merchant_owns));
    assert!(!vm
        .world
        .derived_relation_state()
        .facts()
        .contains_key(&merchant_weight));
    assert!(vm
        .world
        .relation_state()
        .assertions()
        .keys()
        .all(|fact| !fact.tuple.contains(&FactValue::Entity(anvil_ref))));
}

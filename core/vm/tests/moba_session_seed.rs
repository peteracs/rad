// Reproduces the browser client's RAD bootstrap on the native target. The moba
// client loads `RadRuntime`, then `session_start(<concatenated shared sources +
// client main.rad>)`, then emits `SeedLocalAvatar { player_id }` with the
// client's persistent per-tab id (see app/matchIdentity.ts + radHost.create).
// Identity is host-owned, so the RAD source no longer seeds a hardcoded player 1
// at top level. If the SeedLocalAvatar event/handler is broken, the RAD world has
// no PlayerControlled entity, the render buffer is empty, and the local champion
// freezes at spawn while the server-fed ghost keeps moving.
//
// This drives the EXACT `session_start` + `session_emit` entry the browser uses
// (not the `rad <file>` runner), so a regression in the boot seed is caught here.

use rad_vm::wasm::RadRuntime;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn read(rel: &str) -> String {
    let path = format!("{ROOT}/{rel}");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

fn concatenated_client_sources() -> String {
    // Mirror projects/moba-rad/client/vite.config.ts radSourcesPlugin order.
    [
        read("../../projects/moba-rad/server/src/sim/components.rad"),
        read("../../projects/moba-rad/server/src/world/scene.rad"),
        read("../../projects/moba-rad/server/src/world/avatars.rad"),
        read("../../projects/moba-rad/server/src/sim/movement.rad"),
        read("../../projects/moba-rad/client/src/rad/main.rad"),
    ]
    .join("\n")
}

fn seed_local_avatar(rt: &mut RadRuntime, player_id: i64) {
    rt.session_emit("SeedLocalAvatar", &format!("{{\"player_id\":{player_id}}}"))
        .expect("session_emit SeedLocalAvatar");
    rt.session_pump().expect("session_pump");
}

#[test]
fn seed_local_avatar_event_seeds_the_local_avatar() {
    let source = concatenated_client_sources();

    let mut rt = RadRuntime::new();
    rt.session_start(&source)
        .expect("session_start must accept the concatenated client sources");
    seed_local_avatar(&mut rt, 7);

    let world = rt.get_world_snapshot();
    assert!(
        world.contains("PlayerControlled"),
        "SeedLocalAvatar must seed a PlayerControlled avatar (the client champion). \
         World snapshot was: {world}"
    );
    assert!(
        world.contains("\"player_id\":7"),
        "SeedLocalAvatar must seed the requested player_id, not a hardcoded one. \
         World snapshot was: {world}"
    );
}

#[test]
fn distinct_player_ids_seed_distinct_avatars() {
    // Two tabs => two distinct persistent ids => two distinct avatars. This is
    // the multiplayer-identity invariant that lets two clients see and sync with
    // each other instead of fighting over a single hardcoded player 1.
    let source = concatenated_client_sources();
    let mut rt = RadRuntime::new();
    rt.session_start(&source).expect("session_start");
    seed_local_avatar(&mut rt, 11);
    seed_local_avatar(&mut rt, 22);
    // Idempotent: re-emitting an existing id must NOT duplicate the avatar.
    seed_local_avatar(&mut rt, 11);

    let world = rt.get_world_snapshot();
    let count = world.matches("PlayerControlled").count();
    assert_eq!(
        count, 2,
        "two distinct ids must yield exactly two avatars (idempotent on repeats). \
         World snapshot was: {world}"
    );
}

#[test]
fn render_buffer_exposes_the_seeded_avatar() {
    let source = concatenated_client_sources();

    let mut rt = RadRuntime::new();
    rt.session_start(&source).expect("session_start");
    seed_local_avatar(&mut rt, 1);
    rt.session_render_buffer_refresh()
        .expect("render buffer refresh");

    // Header layout: [version, stride, count, ...]. The client reads this exact
    // buffer out of wasm memory; a count of 0 is precisely the frozen-champion
    // symptom (controlled avatar for player_id=1 not found, present ids []).
    // Header is 3 f32 and each avatar record is 9 f32, so a seeded world yields
    // at least 12; an empty world (the bug) yields exactly 3.
    let len = rt.session_render_buffer_f32_len();
    assert!(
        len >= 12,
        "render buffer must expose at least the seeded local avatar (header 3 + \
         stride 9), got f32_len={len} (3 == empty world == frozen champion)"
    );
}

// Regression: an AuthoritativeState whose float fields carry whole numbers
// (e.g. `y: 0`) arrives over JSON as ints. Written into the float-declared
// `Position.y` without coercion, the field's runtime tag became int, the render
// buffer's strict float read dropped the whole avatar, and the local champion
// vanished on the first reconciliation while the server ghost kept moving.
// Float-declared component fields must stay float at every update site.
#[test]
fn authoritative_state_with_integer_field_keeps_the_avatar_renderable() {
    let source = concatenated_client_sources();
    let mut rt = RadRuntime::new();
    rt.session_start(&source).expect("session_start");

    // `y: 0` and `target_y: 0` are whole numbers -> decoded as ints over JSON.
    rt.session_emit(
        "AuthoritativeState",
        r#"{"player_id":1,"command_id":1,"x":12.34,"y":0,"target_x":40,"target_y":0,"target_active":true}"#,
    )
    .expect("session_emit AuthoritativeState");
    rt.session_pump().expect("session_pump");

    rt.session_render_buffer_refresh().expect("render refresh");
    let len = rt.session_render_buffer_f32_len();
    assert!(
        len >= 12,
        "the avatar must survive an AuthoritativeState carrying integer-valued \
         float fields, got f32_len={len} (3 == dropped avatar == frozen champion)"
    );

    // The float field must actually be stored as a float (`0.0`, not `0`), or
    // client snapshots diverge from the float-typed authority and break
    // reconciliation convergence.
    let world = rt.get_world_snapshot();
    assert!(
        world.contains("\"y\":0.0"),
        "Position.y must be stored as a float after an integer-valued update. \
         World snapshot was: {world}"
    );
}

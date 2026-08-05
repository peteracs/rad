//! Browser presentation packet encoding.
//!
//! This module knows the current avatar projection, but knows nothing about
//! WebGPU. RAD owns deterministic presentation data; a browser host owns the
//! GPU device, buffers, pipelines, and recovery lifecycle.

use crate::world::{ComponentView, EntitySelectionError, WorldSnapshot};

pub(crate) const MAGIC: u32 = u32::from_le_bytes(*b"RADP");
pub(crate) const VERSION: u32 = 3;
pub(crate) const HEADER_WORDS: usize = 16;
pub(crate) const RECORD_WORDS: usize = 12;
pub(crate) const DEFAULT_MAX_RECORDS: u32 = 262_144;
pub(crate) const HARD_MAX_RECORDS: u32 = 1_048_576;
pub(crate) const DEFAULT_MAX_ENTITIES_SCANNED: u32 = 1_048_576;
pub(crate) const HARD_MAX_ENTITIES_SCANNED: u32 = 4_194_304;

const FLAG_NONE: u32 = 0;
const PACKET_KIND_FULL: u32 = 0;
const PACKET_KIND_DELTA: u32 = 1;
const HEADER_MAGIC: usize = 0;
const HEADER_VERSION: usize = 1;
const HEADER_RECORD_WORDS: usize = 2;
const HEADER_COUNT: usize = 3;
const HEADER_STREAM_ID_LOW: usize = 4;
const HEADER_STREAM_ID_HIGH: usize = 5;
const HEADER_SEQUENCE_LOW: usize = 6;
const HEADER_SEQUENCE_HIGH: usize = 7;
const HEADER_FRAME_LOW: usize = 8;
const HEADER_FRAME_HIGH: usize = 9;
const HEADER_PACKET_KIND: usize = 10;
const HEADER_BASE_SEQUENCE_LOW: usize = 11;
const HEADER_BASE_SEQUENCE_HIGH: usize = 12;
const HEADER_FLAGS: usize = 13;
const HEADER_RESERVED_0: usize = 14;
const HEADER_RESERVED_1: usize = 15;
const MODEL_UNKNOWN: u32 = 0;
const MODEL_CLOCKWORK_MAGE: u32 = 1;
const MODEL_UNKNOWN_NAME: &str = "";
const MODEL_CLOCKWORK_MAGE_NAME: &str = "clockwork_mage";

/// Encode one packet into caller-owned storage so steady-state frames reuse
/// their allocation. Header and records are all `u32` words:
///
/// ```text
/// header: magic, version, record_words, count,
///         stream_id_lo, stream_id_hi, sequence_lo, sequence_hi,
///         frame_lo, frame_hi, packet_kind, base_sequence_lo,
///         base_sequence_hi, flags, 0, 0
/// record: entity_slot, entity_generation, player_id,
///         x_f32_bits, y_f32_bits, target_x_f32_bits, target_y_f32_bits,
///         target_active, command_id_lo, command_id_hi, model_id, 0
/// ```
pub(crate) fn encode_avatar_packet(
    snapshot: &WorldSnapshot,
    stream_id: u64,
    sequence: u64,
    frame: u64,
    output: &mut Vec<u32>,
    entity_scratch: &mut Vec<u32>,
    max_records: u32,
    max_entities_scanned: u32,
) -> Result<(), String> {
    if max_records == 0 || max_records > HARD_MAX_RECORDS {
        output.clear();
        return Err(format!(
            "presentation.invalid_record_limit: expected 1..={HARD_MAX_RECORDS}, got {max_records}"
        ));
    }
    if max_entities_scanned == 0 || max_entities_scanned > HARD_MAX_ENTITIES_SCANNED {
        output.clear();
        return Err(format!(
            "presentation.invalid_entity_scan_limit: expected 1..={HARD_MAX_ENTITIES_SCANNED}, got {max_entities_scanned}"
        ));
    }
    if let Err(error) = snapshot.collect_sorted_entity_ids_with_components(
        &["PlayerControlled", "Position"],
        max_entities_scanned as usize,
        entity_scratch,
    ) {
        output.clear();
        return Err(match error {
            EntitySelectionError::LimitExceeded { actual } => format!(
                "presentation.entity_scan_limit: {actual} candidates exceed {max_entities_scanned}"
            ),
            EntitySelectionError::AllocationFailed => {
                "presentation.allocation_failed: entity selection".to_string()
            }
        });
    }

    output.clear();
    let result = (|| {
        let retained_capacity = usize::min(entity_scratch.len(), max_records as usize)
            .checked_mul(RECORD_WORDS)
            .and_then(|words| words.checked_add(HEADER_WORDS))
            .ok_or_else(|| "presentation.allocation_failed: packet size overflow".to_string())?;
        output
            .try_reserve(retained_capacity)
            .map_err(|_| "presentation.allocation_failed: avatar packet".to_string())?;
        output.resize(HEADER_WORDS, 0);

        let mut count = 0u32;
        for &entity in entity_scratch.iter() {
            let Some(player) = snapshot.component_view(entity, "PlayerControlled") else {
                continue;
            };
            let Some(player_id) = int_field(&player, "player_id") else {
                continue;
            };
            let Some(position) = snapshot.component_view(entity, "Position") else {
                continue;
            };
            let Some(x) = float_field(&position, "x") else {
                continue;
            };
            let Some(y) = float_field(&position, "y") else {
                continue;
            };
            if count == max_records {
                return Err(format!(
                    "presentation.record_limit: avatar packet exceeds {max_records} records"
                ));
            }
            let player_id = u32::try_from(player_id).map_err(|_| {
                format!(
                    "presentation.player_id_out_of_range: entity {entity} has player_id {player_id}"
                )
            })?;
            let entity_ref = snapshot
                .entity_ref(entity)
                .ok_or_else(|| format!("presentation.entity_lifetime_missing: entity {entity}"))?;

            let target = snapshot.component_view(entity, "MoveTarget");
            let target_x = target
                .as_ref()
                .and_then(|component| float_field(component, "x"))
                .unwrap_or(x);
            let target_y = target
                .as_ref()
                .and_then(|component| float_field(component, "y"))
                .unwrap_or(y);
            let target_active = target
                .as_ref()
                .and_then(|component| bool_field(component, "active"))
                .unwrap_or(false);
            let command_id = target
                .as_ref()
                .and_then(|component| int_field(component, "command_id"))
                .unwrap_or(0);
            let command_bits = command_id as u64;
            let render = snapshot.component_view(entity, "RenderAvatar");
            let model_id = render
                .as_ref()
                .and_then(|component| model_id_field(component, "model"))
                .unwrap_or(MODEL_UNKNOWN);

            output.extend_from_slice(&[
                entity_ref.slot,
                entity_ref.generation,
                player_id,
                finite_f32_bits(x, entity, "Position.x")?,
                finite_f32_bits(y, entity, "Position.y")?,
                finite_f32_bits(target_x, entity, "MoveTarget.x")?,
                finite_f32_bits(target_y, entity, "MoveTarget.y")?,
                u32::from(target_active),
                command_bits as u32,
                (command_bits >> 32) as u32,
                model_id,
                0,
            ]);
            count += 1;
        }

        output[HEADER_MAGIC] = MAGIC;
        output[HEADER_VERSION] = VERSION;
        output[HEADER_RECORD_WORDS] = RECORD_WORDS as u32;
        output[HEADER_COUNT] = count;
        output[HEADER_STREAM_ID_LOW] = stream_id as u32;
        output[HEADER_STREAM_ID_HIGH] = (stream_id >> 32) as u32;
        output[HEADER_SEQUENCE_LOW] = sequence as u32;
        output[HEADER_SEQUENCE_HIGH] = (sequence >> 32) as u32;
        output[HEADER_FRAME_LOW] = frame as u32;
        output[HEADER_FRAME_HIGH] = (frame >> 32) as u32;
        output[HEADER_PACKET_KIND] = PACKET_KIND_FULL;
        output[HEADER_BASE_SEQUENCE_LOW] = 0;
        output[HEADER_BASE_SEQUENCE_HIGH] = 0;
        output[HEADER_FLAGS] = FLAG_NONE;
        output[HEADER_RESERVED_0] = 0;
        output[HEADER_RESERVED_1] = 0;
        Ok(())
    })();
    if result.is_err() {
        output.clear();
    }
    result
}

pub(crate) fn descriptor_json() -> serde_json::Value {
    serde_json::json!({
        "avatar_instances": {
            "magic": MAGIC,
            "version": VERSION,
            "header_words": HEADER_WORDS,
            "record_words": RECORD_WORDS,
            "supported_flags": FLAG_NONE,
            "default_max_records": DEFAULT_MAX_RECORDS,
            "hard_max_records": HARD_MAX_RECORDS,
            "default_max_entities_scanned": DEFAULT_MAX_ENTITIES_SCANNED,
            "hard_max_entities_scanned": HARD_MAX_ENTITIES_SCANNED,
            "model_names": [MODEL_UNKNOWN_NAME, MODEL_CLOCKWORK_MAGE_NAME],
            "packet_kinds": {
                "full": PACKET_KIND_FULL,
                "delta": PACKET_KIND_DELTA
            },
            "header_fields": {
                "magic": HEADER_MAGIC,
                "version": HEADER_VERSION,
                "record_words": HEADER_RECORD_WORDS,
                "count": HEADER_COUNT,
                "stream_id_low": HEADER_STREAM_ID_LOW,
                "stream_id_high": HEADER_STREAM_ID_HIGH,
                "sequence_low": HEADER_SEQUENCE_LOW,
                "sequence_high": HEADER_SEQUENCE_HIGH,
                "frame_low": HEADER_FRAME_LOW,
                "frame_high": HEADER_FRAME_HIGH,
                "packet_kind": HEADER_PACKET_KIND,
                "base_sequence_low": HEADER_BASE_SEQUENCE_LOW,
                "base_sequence_high": HEADER_BASE_SEQUENCE_HIGH,
                "flags": HEADER_FLAGS,
                "reserved_0": HEADER_RESERVED_0,
                "reserved_1": HEADER_RESERVED_1
            },
            "fields": {
                "entity_slot": 0,
                "entity_generation": 1,
                "player_id": 2,
                "x": 3,
                "y": 4,
                "target_x": 5,
                "target_y": 6,
                "target_active": 7,
                "command_id_low": 8,
                "command_id_high": 9,
                "model_id": 10,
                "reserved": 11
            }
        }
    })
}

fn finite_f32_bits(value: f64, entity: u32, field: &str) -> Result<u32, String> {
    let value = value as f32;
    if value.is_finite() {
        Ok(value.to_bits())
    } else {
        Err(format!(
            "presentation.non_finite_coordinate: entity {entity} field {field}"
        ))
    }
}

fn float_field(component: &ComponentView<'_>, field_name: &str) -> Option<f64> {
    component.field(field_name)?.as_float()
}

fn int_field(component: &ComponentView<'_>, field_name: &str) -> Option<i64> {
    component.field(field_name)?.as_int()
}

fn bool_field(component: &ComponentView<'_>, field_name: &str) -> Option<bool> {
    component.field(field_name)?.as_bool()
}

fn model_id_field(component: &ComponentView<'_>, field_name: &str) -> Option<u32> {
    let value = component.field(field_name)?;
    value.as_str().map(render_model_id)
}

fn render_model_id(model: &str) -> u32 {
    if model == MODEL_CLOCKWORK_MAGE_NAME {
        MODEL_CLOCKWORK_MAGE
    } else {
        MODEL_UNKNOWN
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::ComponentData;
    use crate::world::World;
    use std::sync::Arc;

    #[test]
    fn packet_preserves_identity_generation_and_i64_command_bits() {
        let mut world = World::new();
        let mut gc = crate::gc::GcHeap::new();
        let entity = world.spawn_entity(Some("avatar")).unwrap();
        world.set_component(
            entity,
            ComponentData {
                type_name: "PlayerControlled".into(),
                layout: Arc::new(vec!["player_id".into()]),
                values: vec![crate::value::Value::int(7)],
            },
        );
        world.set_component(
            entity,
            ComponentData {
                type_name: "Position".into(),
                layout: Arc::new(vec!["x".into(), "y".into()]),
                values: vec![
                    crate::value::Value::from_float(12.5),
                    crate::value::Value::from_float(-4.0),
                ],
            },
        );
        world.set_component(
            entity,
            ComponentData {
                type_name: "MoveTarget".into(),
                layout: Arc::new(vec![
                    "x".into(),
                    "y".into(),
                    "active".into(),
                    "command_id".into(),
                ]),
                values: vec![
                    crate::value::Value::from_float(30.0),
                    crate::value::Value::from_float(40.0),
                    crate::value::Value::from_bool(true),
                    crate::value::Value::from_int(&mut gc, (1_i64 << 47) - 9),
                ],
            },
        );

        let mut packet = Vec::new();
        let mut entity_scratch = Vec::new();
        encode_avatar_packet(
            &world.snapshot(),
            0x1122_3344_5566_7788,
            0x8877_6655_4433_2211,
            u64::MAX - 2,
            &mut packet,
            &mut entity_scratch,
            DEFAULT_MAX_RECORDS,
            DEFAULT_MAX_ENTITIES_SCANNED,
        )
        .unwrap();
        assert_eq!(packet[0], MAGIC);
        assert_eq!(packet[1], VERSION);
        assert_eq!(packet[2], RECORD_WORDS as u32);
        assert_eq!(packet[3], 1);
        assert_eq!(packet[4], 0x5566_7788);
        assert_eq!(packet[5], 0x1122_3344);
        assert_eq!(packet[6], 0x4433_2211);
        assert_eq!(packet[7], 0x8877_6655);
        assert_eq!(packet[8], u32::MAX - 2);
        assert_eq!(packet[9], u32::MAX);
        assert_eq!(packet[10], PACKET_KIND_FULL);
        let row = &packet[HEADER_WORDS..];
        assert_eq!(row[0], entity);
        assert_eq!(row[1], 0);
        assert_eq!(row[2], 7);
        assert_eq!(f32::from_bits(row[3]), 12.5);
        assert_eq!(f32::from_bits(row[4]), -4.0);
        assert_eq!(
            u64::from(row[8]) | (u64::from(row[9]) << 32),
            (1_u64 << 47) - 9
        );
    }

    #[test]
    fn invalid_limit_and_encoding_failure_leave_no_partial_packet() {
        let world = World::new();
        let mut packet = vec![1, 2, 3];
        let mut entity_scratch = Vec::new();
        let error = encode_avatar_packet(
            &world.snapshot(),
            1,
            0,
            0,
            &mut packet,
            &mut entity_scratch,
            0,
            DEFAULT_MAX_ENTITIES_SCANNED,
        )
        .unwrap_err();
        assert!(error.starts_with("presentation.invalid_record_limit:"));
        assert!(packet.is_empty());

        let mut world = World::new();
        let first = world.spawn_entity(Some("first")).unwrap();
        let second = world.spawn_entity(Some("second")).unwrap();
        encode_avatar_packet(
            &world.snapshot(),
            1,
            0,
            0,
            &mut packet,
            &mut entity_scratch,
            DEFAULT_MAX_RECORDS,
            1,
        )
        .expect("non-presentation entities do not consume the candidate scan budget");
        assert_eq!(packet[3], 0);

        add_minimal_avatar(&mut world, first, 1);
        add_minimal_avatar(&mut world, second, 2);
        packet.extend_from_slice(&[4, 5, 6]);
        let error = encode_avatar_packet(
            &world.snapshot(),
            1,
            1,
            0,
            &mut packet,
            &mut entity_scratch,
            DEFAULT_MAX_RECORDS,
            1,
        )
        .unwrap_err();
        assert!(error.starts_with("presentation.entity_scan_limit:"));
        assert!(packet.is_empty());
    }

    fn add_minimal_avatar(world: &mut World, entity: u32, player_id: i64) {
        world.set_component(
            entity,
            ComponentData {
                type_name: "PlayerControlled".into(),
                layout: Arc::new(vec!["player_id".into()]),
                values: vec![crate::value::Value::int(player_id)],
            },
        );
        world.set_component(
            entity,
            ComponentData {
                type_name: "Position".into(),
                layout: Arc::new(vec!["x".into(), "y".into()]),
                values: vec![
                    crate::value::Value::from_float(0.0),
                    crate::value::Value::from_float(0.0),
                ],
            },
        );
    }
}

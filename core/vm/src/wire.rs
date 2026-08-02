//! Compact value codec for the fork wire format (v2).
//!
//! The v1 codec built a `serde_json::Value` tree with a `{"t": ..., "v": ...}`
//! envelope per scalar — measured at 246 ms / 1.45 MB for a 10k-entity world.
//! v2 writes canonical JSON directly into a `String` (no intermediate tree)
//! and spends bytes only where type fidelity demands them:
//!
//! - `nil` → `null`, `bool` → `true`/`false`, `int` → bare integer
//! - `float` → bare number that always carries `.` or `e` (so the decoder
//!   can tell it from an int; integral floats print as `1.0`)
//! - `str` → JSON string, `list` → JSON array
//! - `entity` → `{"e":id}`, `tuple` → `{"t":[...]}`
//! - `map` → `{"m":[[[tag,key],value],...]}` (keys sorted — canonical)
//! - sum type → `{"s":[type,variant,{fields sorted}]}`
//! - component → `{"c":[type,[layout],[values]]}`
//!
//! Encoding is deterministic: the same value produces the same bytes on
//! every machine, which is what makes the wire digest and the
//! re-encode-is-byte-identical guarantee possible.

use crate::causality::{
    Cause, EmitRecord, ProposalRecord, ResolutionRecord, SettlementRecord, WireProvenance,
    WriteKind, WriteRecord,
};
use crate::value::{Allocator, MapKey, MapStorage, Value};
use std::fmt::Write;

// ---------------------------------------------------------------------------
// Provenance section: the sender's ledger closure rides the fork payload so
// the receiver can answer why() for state it never computed.
//
//   "prov":[[writes...],[emits...],[settlements...],[proposals...],[resolutions...]]
//   write: [frame, entity|null, name|null, component, value, kind, cause, origin|null,
//           resolution_id|null]
//   emit:  [id, event, frame, payload, cause, origin|null]
//   cause: [0] main | [1, system] | [2, event, emit_id] ; kind: 0..=4
// ---------------------------------------------------------------------------

fn encode_cause_into(by: &Cause, out: &mut String) {
    match by {
        Cause::Main => out.push_str("[0]"),
        Cause::System { name } => {
            out.push_str("[1,");
            escape_json_into(out, name);
            out.push(']');
        }
        Cause::Handler { event, emit_id } => {
            out.push_str("[2,");
            escape_json_into(out, event);
            let _ = write!(out, ",{}]", emit_id);
        }
    }
}

fn encode_opt_str_into(s: &Option<String>, out: &mut String) {
    match s {
        Some(s) => escape_json_into(out, s),
        None => out.push_str("null"),
    }
}

pub fn encode_prov_into(prov: &WireProvenance, out: &mut String) {
    // Preserve the pre-RFC wire bytes for worlds without causal fan-in. The
    // five-section form is negotiated by the `causal_laws` capability and is
    // emitted only when it actually carries settlement records.
    let extended = !prov.settlements.is_empty()
        || !prov.proposals.is_empty()
        || !prov.resolutions.is_empty()
        || prov
            .writes
            .iter()
            .any(|write| write.resolution_id.is_some());
    out.push_str("[[");
    for (i, w) in prov.writes.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "[{},", w.frame);
        match w.entity {
            Some(e) => {
                let _ = write!(out, "{},", e);
            }
            None => out.push_str("null,"),
        }
        encode_opt_str_into(&w.entity_name, out);
        out.push(',');
        escape_json_into(out, &w.component);
        out.push(',');
        escape_json_into(out, &w.value);
        let kind = match w.kind {
            WriteKind::Set => 0,
            WriteKind::Spawn => 1,
            WriteKind::Despawn => 2,
            WriteKind::Remove => 3,
            WriteKind::Resource => 4,
        };
        let _ = write!(out, ",{},", kind);
        encode_cause_into(&w.by, out);
        out.push(',');
        encode_opt_str_into(&w.origin, out);
        if extended {
            out.push(',');
            match w.resolution_id {
                Some(id) => {
                    let _ = write!(out, "{}", id);
                }
                None => out.push_str("null"),
            }
        }
        out.push(']');
    }
    out.push_str("],[");
    for (i, e) in prov.emits.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "[{},", e.id);
        escape_json_into(out, &e.event);
        let _ = write!(out, ",{},", e.frame);
        escape_json_into(out, &e.payload);
        out.push(',');
        encode_cause_into(&e.by, out);
        out.push(',');
        encode_opt_str_into(&e.origin, out);
        out.push(']');
    }
    if !extended {
        out.push_str("]]");
        return;
    }
    out.push_str("],[");
    for (i, settlement) in prov.settlements.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "[{},{},", settlement.id, settlement.frame);
        encode_cause_into(&settlement.by, out);
        out.push(']');
    }
    out.push_str("],[");
    for (i, proposal) in prov.proposals.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "[{},{},", proposal.id, proposal.settlement_id);
        escape_json_into(out, &proposal.intent);
        let _ = write!(out, ",{},", proposal.key);
        escape_json_into(out, &proposal.payload);
        out.push(',');
        escape_json_into(out, &proposal.law);
        let _ = write!(out, ",{}]", proposal.source_line);
    }
    out.push_str("],[");
    for (i, resolution) in prov.resolutions.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "[{},{},", resolution.id, resolution.settlement_id);
        escape_json_into(out, &resolution.intent);
        let _ = write!(out, ",{},", resolution.key);
        escape_json_into(out, &resolution.resolver);
        out.push_str(",[");
        for (index, proposal_id) in resolution.proposal_ids.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            let _ = write!(out, "{}", proposal_id);
        }
        out.push_str("]]");
    }
    out.push_str("]]");
}

/// Write one component/resource row in wire layout: `"Type",[v0,v1,...]`.
/// The first occurrence of a type pins its wire layout in `schema`; later
/// instances (which can only differ in field order, never field set) remap
/// into it. Shared by the full fork codec and the delta codec.
pub fn write_row_into(
    schema: &mut std::collections::BTreeMap<String, std::sync::Arc<Vec<String>>>,
    data: &crate::value::ComponentData,
    out: &mut String,
) -> Result<(), String> {
    let wire_layout = schema
        .entry(data.type_name.clone())
        .or_insert_with(|| data.layout.clone())
        .clone();
    escape_json_into(out, &data.type_name);
    out.push_str(",[");
    let aligned =
        std::sync::Arc::ptr_eq(&wire_layout, &data.layout) || *wire_layout == *data.layout;
    for i in 0..wire_layout.len() {
        if i > 0 {
            out.push(',');
        }
        let v = if aligned {
            &data.values[i]
        } else {
            let f = &wire_layout[i];
            let pos = data.layout.iter().position(|n| n == f).ok_or_else(|| {
                format!(
                    "wire: instances of '{}' disagree on field '{}'",
                    data.type_name, f
                )
            })?;
            &data.values[pos]
        };
        encode_value_into(v, out)?;
    }
    out.push(']');
    Ok(())
}

fn decode_cause(j: &serde_json::Value) -> Result<Cause, String> {
    let arr = j.as_array().ok_or("prov: malformed cause")?;
    match arr.first().and_then(|v| v.as_u64()) {
        Some(0) => Ok(Cause::Main),
        Some(1) => Ok(Cause::System {
            name: arr
                .get(1)
                .and_then(|v| v.as_str())
                .ok_or("prov: malformed system cause")?
                .to_string(),
        }),
        Some(2) => Ok(Cause::Handler {
            event: arr
                .get(1)
                .and_then(|v| v.as_str())
                .ok_or("prov: malformed handler cause")?
                .to_string(),
            emit_id: arr
                .get(2)
                .and_then(|v| v.as_u64())
                .ok_or("prov: malformed handler cause")?,
        }),
        _ => Err("prov: unknown cause tag".into()),
    }
}

fn decode_opt_str(j: &serde_json::Value) -> Option<String> {
    j.as_str().map(String::from)
}

fn decode_u32(j: &serde_json::Value, field: &str) -> Result<u32, String> {
    let raw = j
        .as_u64()
        .ok_or_else(|| format!("{}: expected unsigned integer", field))?;
    u32::try_from(raw).map_err(|_| format!("{} exceeds u32", field))
}

pub fn decode_prov(j: &serde_json::Value) -> Result<WireProvenance, String> {
    let sections = j
        .as_array()
        .filter(|a| a.len() == 2 || a.len() == 5)
        .ok_or("prov: malformed section")?;
    let mut writes = Vec::new();
    for w in sections[0].as_array().ok_or("prov: malformed writes")? {
        let f = w
            .as_array()
            .filter(|a| a.len() == 8 || a.len() == 9)
            .ok_or("prov: malformed write")?;
        writes.push(WriteRecord {
            frame: f[0].as_u64().ok_or("prov: malformed write")?,
            entity: if f[1].is_null() {
                None
            } else {
                Some(decode_u32(&f[1], "prov: write entity")?)
            },
            entity_name: decode_opt_str(&f[2]),
            component: f[3].as_str().ok_or("prov: malformed write")?.to_string(),
            value: f[4].as_str().ok_or("prov: malformed write")?.to_string(),
            kind: match f[5].as_u64() {
                Some(0) => WriteKind::Set,
                Some(1) => WriteKind::Spawn,
                Some(2) => WriteKind::Despawn,
                Some(3) => WriteKind::Remove,
                Some(4) => WriteKind::Resource,
                _ => return Err("prov: unknown write kind".into()),
            },
            by: decode_cause(&f[6])?,
            origin: decode_opt_str(&f[7]),
            resolution_id: f.get(8).and_then(|value| value.as_u64()),
        });
    }
    let mut emits = Vec::new();
    for e in sections[1].as_array().ok_or("prov: malformed emits")? {
        let f = e
            .as_array()
            .filter(|a| a.len() == 6)
            .ok_or("prov: malformed emit")?;
        emits.push(EmitRecord {
            id: f[0].as_u64().ok_or("prov: malformed emit")?,
            event: f[1].as_str().ok_or("prov: malformed emit")?.to_string(),
            frame: f[2].as_u64().ok_or("prov: malformed emit")?,
            payload: f[3].as_str().ok_or("prov: malformed emit")?.to_string(),
            by: decode_cause(&f[4])?,
            origin: decode_opt_str(&f[5]),
        });
    }
    let mut settlements = Vec::new();
    let mut proposals = Vec::new();
    let mut resolutions = Vec::new();
    if sections.len() == 5 {
        for settlement in sections[2]
            .as_array()
            .ok_or("prov: malformed settlements")?
        {
            let fields = settlement
                .as_array()
                .filter(|fields| fields.len() == 3)
                .ok_or("prov: malformed settlement")?;
            settlements.push(SettlementRecord {
                id: fields[0].as_u64().ok_or("prov: malformed settlement")?,
                frame: fields[1].as_u64().ok_or("prov: malformed settlement")?,
                by: decode_cause(&fields[2])?,
            });
        }
        for proposal in sections[3].as_array().ok_or("prov: malformed proposals")? {
            let fields = proposal
                .as_array()
                .filter(|fields| fields.len() == 7)
                .ok_or("prov: malformed proposal")?;
            proposals.push(ProposalRecord {
                id: fields[0].as_u64().ok_or("prov: malformed proposal")?,
                settlement_id: fields[1].as_u64().ok_or("prov: malformed proposal")?,
                intent: fields[2]
                    .as_str()
                    .ok_or("prov: malformed proposal")?
                    .to_string(),
                key: decode_u32(&fields[3], "prov: proposal key")?,
                payload: fields[4]
                    .as_str()
                    .ok_or("prov: malformed proposal")?
                    .to_string(),
                law: fields[5]
                    .as_str()
                    .ok_or("prov: malformed proposal")?
                    .to_string(),
                source_line: decode_u32(&fields[6], "prov: proposal source line")?,
            });
        }
        for resolution in sections[4]
            .as_array()
            .ok_or("prov: malformed resolutions")?
        {
            let fields = resolution
                .as_array()
                .filter(|fields| fields.len() == 6)
                .ok_or("prov: malformed resolution")?;
            resolutions.push(ResolutionRecord {
                id: fields[0].as_u64().ok_or("prov: malformed resolution")?,
                settlement_id: fields[1].as_u64().ok_or("prov: malformed resolution")?,
                intent: fields[2]
                    .as_str()
                    .ok_or("prov: malformed resolution")?
                    .to_string(),
                key: decode_u32(&fields[3], "prov: resolution key")?,
                resolver: fields[4]
                    .as_str()
                    .ok_or("prov: malformed resolution")?
                    .to_string(),
                proposal_ids: fields[5]
                    .as_array()
                    .ok_or("prov: malformed resolution")?
                    .iter()
                    .map(|id| id.as_u64().ok_or("prov: malformed resolution"))
                    .collect::<Result<Vec<_>, _>>()?,
            });
        }
    }
    Ok(WireProvenance {
        origin: String::new(),
        writes,
        emits,
        settlements,
        proposals,
        resolutions,
    })
}

pub fn escape_json_into(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Append the canonical wire encoding of a map key (tag + payload, no
/// surrounding brackets). Tuple keys nest recursively as
/// `"t",[[tag,val],...]`.
fn encode_map_key_into(k: &MapKey, out: &mut String) {
    match k {
        MapKey::Str(s) => {
            out.push_str("\"s\",");
            escape_json_into(out, s);
        }
        MapKey::Int(n) => {
            let _ = write!(out, "\"i\",{}", n);
        }
        MapKey::Bool(b) => {
            let _ = write!(out, "\"b\",{}", b);
        }
        MapKey::Entity(e) => {
            let _ = write!(out, "\"e\",{}", e);
        }
        MapKey::Tuple(items) => {
            out.push_str("\"t\",[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push('[');
                encode_map_key_into(item, out);
                out.push(']');
            }
            out.push(']');
        }
    }
}

/// Decode a `[tag, payload]` map key pair (inverse of
/// `encode_map_key_into`).
fn decode_map_key(karr: &[serde_json::Value]) -> Result<MapKey, String> {
    let tag = karr
        .first()
        .and_then(|value| value.as_str())
        .ok_or_else(|| "wire codec: malformed map key tag".to_string())?;
    let payload = karr
        .get(1)
        .ok_or_else(|| "wire codec: missing map key payload".to_string())?;
    match tag {
        "s" => Ok(MapKey::Str(
            payload
                .as_str()
                .ok_or_else(|| "wire codec: malformed string map key".to_string())?
                .to_string(),
        )),
        "i" => Ok(MapKey::Int(payload.as_i64().ok_or_else(|| {
            "wire codec: malformed integer map key".to_string()
        })?)),
        "b" => Ok(MapKey::Bool(payload.as_bool().ok_or_else(|| {
            "wire codec: malformed boolean map key".to_string()
        })?)),
        "e" => Ok(MapKey::Entity(decode_u32(
            payload,
            "wire codec: entity map key",
        )?)),
        "t" => {
            let items = payload
                .as_array()
                .ok_or_else(|| "wire codec: malformed tuple map key".to_string())?;
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let pair = item
                    .as_array()
                    .filter(|a| a.len() == 2)
                    .ok_or_else(|| "wire codec: malformed tuple map key item".to_string())?;
                out.push(decode_map_key(pair)?);
            }
            Ok(MapKey::Tuple(out))
        }
        _ => Err(format!("wire codec: unknown map key tag '{}'", tag)),
    }
}

/// Append the canonical wire encoding of `v` to `out`.
pub fn encode_value_into(v: &Value, out: &mut String) -> Result<(), String> {
    if v.is_nil() {
        out.push_str("null");
        return Ok(());
    }
    if let Some(b) = v.as_bool() {
        out.push_str(if b { "true" } else { "false" });
        return Ok(());
    }
    if let Some(n) = v.as_int() {
        let _ = write!(out, "{}", n);
        return Ok(());
    }
    if let Some(x) = v.as_float() {
        if !x.is_finite() {
            return Err(format!("wire codec: cannot encode non-finite float {}", x));
        }
        if x.abs() >= 1e17 {
            // Shortest round-trip exponent form for extreme magnitudes.
            // Expanded decimal breaks here: f64::MAX expands to 309 digits
            // that serde_json rejects on re-parse ("number out of range"),
            // making the save/fork bytes permanently unloadable — and every
            // large float costs hundreds of digits besides. The exponent is
            // itself the float marker, and `{:e}` re-parses to the same
            // bits. Everything below the threshold keeps its existing text,
            // so digests of worlds holding everyday floats are unchanged.
            let _ = write!(out, "{:e}", x);
        } else if x == x.trunc() {
            // Force the mark that distinguishes float from int. The exact
            // decimal expansion of an f64 is finite, so this re-parses to
            // the same bits.
            let _ = write!(out, "{:.1}", x);
        } else {
            let _ = write!(out, "{}", x);
        }
        return Ok(());
    }
    if let Some(e) = v.as_entity_id() {
        let _ = write!(out, "{{\"e\":{}}}", e);
        return Ok(());
    }
    if let Some(s) = v.as_str() {
        escape_json_into(out, s);
        return Ok(());
    }
    if let Some(items) = v.as_list() {
        out.push('[');
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            encode_value_into(item, out)?;
        }
        out.push(']');
        return Ok(());
    }
    if let Some(items) = v.as_tuple() {
        out.push_str("{\"t\":[");
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            encode_value_into(item, out)?;
        }
        out.push_str("]}");
        return Ok(());
    }
    if let Some(m) = v.as_map() {
        let mut keys: Vec<&MapKey> = m.keys().collect();
        keys.sort();
        out.push_str("{\"m\":[");
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("[[");
            encode_map_key_into(k, out);
            out.push_str("],");
            encode_value_into(&m[k], out)?;
            out.push(']');
        }
        out.push_str("]}");
        return Ok(());
    }
    if let Some(st) = v.as_sum_type() {
        out.push_str("{\"s\":[");
        escape_json_into(out, &st.type_name);
        out.push(',');
        escape_json_into(out, &st.variant);
        out.push_str(",{");
        let mut keys: Vec<&String> = st.fields.keys().collect();
        keys.sort();
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            escape_json_into(out, k);
            out.push(':');
            encode_value_into(&st.fields[*k], out)?;
        }
        out.push_str("}]}");
        return Ok(());
    }
    if let Some(c) = v.as_component() {
        out.push_str("{\"c\":[");
        escape_json_into(out, &c.type_name);
        out.push_str(",[");
        for (i, f) in c.layout.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            escape_json_into(out, f);
        }
        out.push_str("],[");
        for (i, fv) in c.values.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            encode_value_into(fv, out)?;
        }
        out.push_str("]]}");
        return Ok(());
    }
    Err(format!(
        "wire codec: cannot encode {} (forks carry data, not code)",
        v.type_name()
    ))
}

/// Decode one wire value into the given allocator (gc heap for transient
/// values, `PersistentStore` for values that live in snapshots).
pub fn decode_value(gc: &mut dyn Allocator, j: &serde_json::Value) -> Result<Value, String> {
    use serde_json::Value as Json;
    let bad = |what: &str| format!("wire codec: malformed {} node: {}", what, j);
    match j {
        Json::Null => Ok(Value::NIL),
        Json::Bool(b) => Ok(Value::from_bool(*b)),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::from_int(gc, i))
            } else if let Some(x) = n.as_f64() {
                Ok(Value::from_float(x))
            } else {
                Err(bad("number"))
            }
        }
        Json::String(s) => Ok(Value::from_string(gc, s.clone())),
        Json::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(decode_value(gc, item)?);
            }
            Ok(Value::list(gc, out))
        }
        Json::Object(o) => {
            if o.len() != 1 {
                return Err(bad("tagged object"));
            }
            let (tag, body) = o.iter().next().unwrap();
            match tag.as_str() {
                "e" => Ok(Value::from_entity_id(
                    gc,
                    decode_u32(body, "wire codec: entity id")?,
                )),
                "t" => {
                    let items = body.as_array().ok_or_else(|| bad("tuple"))?;
                    let mut out = Vec::with_capacity(items.len());
                    for item in items {
                        out.push(decode_value(gc, item)?);
                    }
                    Ok(Value::tuple(gc, out))
                }
                "m" => {
                    let pairs = body.as_array().ok_or_else(|| bad("map"))?;
                    let mut m = MapStorage::new();
                    for kv in pairs {
                        let kv = kv
                            .as_array()
                            .filter(|a| a.len() == 2)
                            .ok_or_else(|| bad("map"))?;
                        let karr = kv[0]
                            .as_array()
                            .filter(|a| a.len() == 2)
                            .ok_or_else(|| bad("map key"))?;
                        let key = decode_map_key(karr)?;
                        m.insert(key, decode_value(gc, &kv[1])?);
                    }
                    Ok(Value::map(gc, m))
                }
                "s" => {
                    let parts = body
                        .as_array()
                        .filter(|a| a.len() == 3)
                        .ok_or_else(|| bad("sum"))?;
                    let ty = parts[0].as_str().ok_or_else(|| bad("sum"))?.to_string();
                    let var = parts[1].as_str().ok_or_else(|| bad("sum"))?.to_string();
                    let fields_obj = parts[2].as_object().ok_or_else(|| bad("sum"))?;
                    let mut fields = std::collections::HashMap::with_capacity(fields_obj.len());
                    for (k, fv) in fields_obj {
                        fields.insert(k.clone(), decode_value(gc, fv)?);
                    }
                    Ok(Value::sum_type(gc, ty, var, fields))
                }
                "c" => {
                    let parts = body
                        .as_array()
                        .filter(|a| a.len() == 3)
                        .ok_or_else(|| bad("component"))?;
                    let ty = parts[0]
                        .as_str()
                        .ok_or_else(|| bad("component"))?
                        .to_string();
                    let layout: Vec<String> = parts[1]
                        .as_array()
                        .ok_or_else(|| bad("component"))?
                        .iter()
                        .filter_map(|f| f.as_str().map(String::from))
                        .collect();
                    let vals_json = parts[2].as_array().ok_or_else(|| bad("component"))?;
                    if layout.len() != vals_json.len() {
                        return Err(bad("component"));
                    }
                    let mut values = Vec::with_capacity(vals_json.len());
                    for fv in vals_json {
                        values.push(decode_value(gc, fv)?);
                    }
                    Ok(Value::component(
                        gc,
                        ty,
                        std::sync::Arc::new(layout),
                        values,
                    ))
                }
                _ => Err(bad("tagged object")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::GcHeap;

    #[test]
    fn scalar_roundtrip_preserves_types() {
        let mut gc = GcHeap::new();
        let vals = vec![
            Value::NIL,
            Value::from_bool(true),
            Value::from_int(&mut gc, 42),
            Value::from_int(&mut gc, -7),
            Value::from_float(1.0), // integral float must stay float
            Value::from_float(2.5),
            Value::from_float(1e300),
            Value::from_string(&mut gc, "he said \"hi\"\n".to_string()),
            Value::from_entity_id(&mut gc, 9),
        ];
        for v in vals {
            let mut s = String::new();
            encode_value_into(&v, &mut s).unwrap();
            let j: serde_json::Value = serde_json::from_str(&s).unwrap();
            let back = decode_value(&mut gc, &j).unwrap();
            assert_eq!(v.type_name(), back.type_name(), "wire: {}", s);
            assert_eq!(v.to_string(), back.to_string(), "wire: {}", s);
            // And re-encoding is byte-identical (canonical form).
            let mut s2 = String::new();
            encode_value_into(&back, &mut s2).unwrap();
            assert_eq!(s, s2);
        }
    }

    /// A4 BUG 04 (seq 51): f64::MAX used to be written as its 309-digit
    /// decimal expansion, which serde_json rejects on re-parse ("number out
    /// of range") — save_world() wrote a save that load_world() and
    /// fork_from_bytes() refused. Every finite float the VM can hold must
    /// round-trip bit-exactly through the canonical wire text, and integral
    /// floats must keep their float marker (never decay to int).
    #[test]
    fn extreme_float_magnitudes_roundtrip_bit_exact() {
        let mut gc = GcHeap::new();
        for x in [
            f64::MAX,
            -f64::MAX,
            1.1e308,
            1e308,
            1e300,
            1.5e17,
            1e17,
            f64::MIN_POSITIVE,
            5e-324, // smallest subnormal
            3.0,
            -0.0,
            0.1,
        ] {
            let v = Value::from_float(x);
            let mut s = String::new();
            encode_value_into(&v, &mut s).unwrap();
            let j: serde_json::Value = serde_json::from_str(&s)
                .unwrap_or_else(|e| panic!("wire text for {:e} must re-parse ({}): {}", x, e, s));
            let back = decode_value(&mut gc, &j).unwrap();
            let y = back
                .as_float()
                .unwrap_or_else(|| panic!("float marker lost for {:e}: {}", x, s));
            assert_eq!(
                x.to_bits(),
                y.to_bits(),
                "round-trip must be bit-exact for {:e}: {}",
                x,
                s
            );
            // Re-encoding the decoded value is byte-identical (canonical).
            let mut s2 = String::new();
            encode_value_into(&back, &mut s2).unwrap();
            assert_eq!(s, s2);
        }
    }

    /// The fix for BUG 04: extremes take the shortest exponent form, while
    /// everyday floats keep their established text (digest stability).
    #[test]
    fn float_wire_text_shape() {
        for (x, expected) in [
            (f64::MAX, "1.7976931348623157e308"),
            (-f64::MAX, "-1.7976931348623157e308"),
            (3.0, "3.0"),
            (-0.0, "-0.0"),
            (0.1, "0.1"),
            (1234567890.5, "1234567890.5"),
        ] {
            let mut s = String::new();
            encode_value_into(&Value::from_float(x), &mut s).unwrap();
            assert_eq!(s, expected);
        }
    }

    #[test]
    fn tuple_map_keys_roundtrip_canonically() {
        let mut gc = GcHeap::new();
        let four = Value::from_int(&mut gc, 4);
        let two = Value::from_int(&mut gc, 2);
        let k1 = Value::tuple(&mut gc, vec![four, two]);
        let a = Value::from_string(&mut gc, "a".into());
        let inner = Value::tuple(&mut gc, vec![a, Value::from_bool(true)]);
        let one = Value::from_int(&mut gc, 1);
        let k2 = Value::tuple(&mut gc, vec![one, inner]);
        let mut m = MapStorage::new();
        m.insert(
            MapKey::from_value(&k1).unwrap(),
            Value::from_int(&mut gc, 6),
        );
        m.insert(
            MapKey::from_value(&k2).unwrap(),
            Value::from_int(&mut gc, 9),
        );
        m.insert(MapKey::Str("plain".into()), Value::from_int(&mut gc, 1));
        let v = Value::map(&mut gc, m);

        let mut s = String::new();
        encode_value_into(&v, &mut s).unwrap();
        let j: serde_json::Value = serde_json::from_str(&s).unwrap();
        let back = decode_value(&mut gc, &j).unwrap();
        let bm = back.as_map().unwrap();
        assert_eq!(bm.len(), 3);
        assert_eq!(
            bm[&MapKey::from_value(&k1).unwrap()].as_int(),
            Some(6),
            "wire: {}",
            s
        );
        assert_eq!(bm[&MapKey::from_value(&k2).unwrap()].as_int(), Some(9));
        // canonical: re-encoding is byte-identical
        let mut s2 = String::new();
        encode_value_into(&back, &mut s2).unwrap();
        assert_eq!(s, s2);
    }

    #[test]
    fn float_map_key_is_rejected() {
        let mut gc = GcHeap::new();
        let bad = Value::tuple(&mut gc, vec![Value::from_float(1.5)]);
        assert!(MapKey::from_value(&bad).is_err());
    }

    #[test]
    fn float_int_distinction_survives() {
        let mut gc = GcHeap::new();
        let mut s = String::new();
        encode_value_into(&Value::from_float(5.0), &mut s).unwrap();
        assert_eq!(s, "5.0");
        let j: serde_json::Value = serde_json::from_str(&s).unwrap();
        let back = decode_value(&mut gc, &j).unwrap();
        assert!(back.as_float().is_some());
        assert!(back.as_int().is_none() || back.as_float() == Some(5.0));
    }

    #[test]
    fn provenance_u32_fields_reject_overflow_instead_of_wrapping() {
        let overflows = [u32::MAX as u64 + 1, u64::MAX];
        for overflow in overflows {
            let write = serde_json::json!([
                [[0, overflow, null, "Health", "{}", 0, [0], null, null]],
                [],
                [],
                [],
                []
            ]);
            let error = decode_prov(&write).expect_err("write entity overflow");
            assert!(error.contains("write entity exceeds u32"), "{error}");

            let proposal_key = serde_json::json!([
                [],
                [],
                [[1, 0, [0]]],
                [[1, 1, "Damage", overflow, "{}", "Hit", 1]],
                []
            ]);
            let error = decode_prov(&proposal_key).expect_err("proposal key overflow");
            assert!(error.contains("proposal key exceeds u32"), "{error}");

            let proposal_line = serde_json::json!([
                [],
                [],
                [[1, 0, [0]]],
                [[1, 1, "Damage", 0, "{}", "Hit", overflow]],
                []
            ]);
            let error = decode_prov(&proposal_line).expect_err("source line overflow");
            assert!(
                error.contains("proposal source line exceeds u32"),
                "{error}"
            );

            let resolution = serde_json::json!([
                [],
                [],
                [[1, 0, [0]]],
                [],
                [[1, 1, "Damage", overflow, "ResolveDamage", []]]
            ]);
            let error = decode_prov(&resolution).expect_err("resolution key overflow");
            assert!(error.contains("resolution key exceeds u32"), "{error}");
        }
    }

    #[test]
    fn wire_entity_values_and_map_keys_reject_overflow() {
        let mut gc = GcHeap::new();
        for overflow in [u32::MAX as u64 + 1, u64::MAX] {
            let entity = serde_json::json!({"e": overflow});
            let error = decode_value(&mut gc, &entity).expect_err("entity overflow");
            assert!(error.contains("entity id exceeds u32"), "{error}");

            let map = serde_json::json!({"m": [[["e", overflow], 1]]});
            let error = decode_value(&mut gc, &map).expect_err("map key overflow");
            assert!(error.contains("entity map key exceeds u32"), "{error}");
        }
    }
}

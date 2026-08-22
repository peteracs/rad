// Writing values, map keys, rows, and the provenance closure directly into
// canonical JSON text — no intermediate tree.
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
        Cause::Transaction { name, parent } => {
            out.push_str("[3,");
            escape_json_into(out, name);
            out.push(',');
            encode_cause_into(parent, out);
            out.push(']');
        }
        Cause::HostCall {
            extension,
            generation,
            plugin_digest,
            export,
            input_digest,
            output_digest,
            parent,
        } => {
            out.push_str("[4,");
            escape_json_into(out, extension);
            out.push(',');
            escape_json_into(out, generation);
            out.push(',');
            escape_json_into(out, plugin_digest);
            out.push(',');
            escape_json_into(out, export);
            out.push(',');
            escape_json_into(out, input_digest);
            out.push(',');
            escape_json_into(out, output_digest);
            out.push(',');
            encode_cause_into(parent, out);
            out.push(']');
        }
    }
}

fn encode_opt_str_into(s: &Option<impl AsRef<str>>, out: &mut String) {
    match s {
        Some(s) => escape_json_into(out, s.as_ref()),
        None => out.push_str("null"),
    }
}

pub fn encode_prov_into(prov: &WireProvenance, out: &mut String) {
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
        escape_json_into(out, &w.value_string());
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
        out.push(',');
        match w.resolution_id {
            Some(id) => {
                let _ = write!(out, "{}", id);
            }
            None => out.push_str("null"),
        }
        out.push_str(",[");
        for (index, (field, value)) in w.fields().iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push('[');
            escape_json_into(out, field);
            out.push(',');
            escape_json_into(out, &value.to_string());
            out.push(']');
        }
        out.push(']');
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
    out.push_str("],[");
    for (index, assertion) in prov.relation_assertions.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "[{},{},", assertion.frame, assertion.assertion_id);
        escape_json_into(
            out,
            &crate::relation::runtime::fact_key_transport_hex(&assertion.fact_key),
        );
        out.push_str(",[");
        for (resolution_index, resolution_id) in assertion.resolution_ids.iter().enumerate() {
            if resolution_index > 0 {
                out.push(',');
            }
            let _ = write!(out, "{}", resolution_id);
        }
        out.push_str("],");
        encode_opt_str_into(&assertion.origin, out);
        out.push(']');
    }
    out.push_str("],[");
    let _ = write!(out, "{},", prov.truncation.evicted_records);
    escape_json_into(out, &prov.truncation.digest_hex());
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
        MapKey::Native(value) => {
            out.push_str("\"n\",[");
            escape_json_into(out, &value.type_name);
            out.push(',');
            escape_json_into(out, &value.repr.to_string());
            out.push(',');
            escape_json_into(out, value.flavor.as_str());
            let _ = write!(out, ",{}]", value.bits);
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

/// Append the canonical wire encoding of `v` to `out`.
pub(crate) fn encode_value_into(v: &Value, out: &mut String) -> Result<(), String> {
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
    if let Some(value) = v.as_native_scalar() {
        out.push_str("{\"n\":[");
        escape_json_into(out, &value.type_name);
        out.push(',');
        escape_json_into(out, &value.repr.to_string());
        out.push(',');
        escape_json_into(out, value.flavor.as_str());
        let _ = write!(out, ",{}]}}", value.bits);
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

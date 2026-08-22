// Reading the canonical JSON wire form back into values, map keys, and the
// provenance closure, rejecting anything the encoder could not have written.
fn decode_cause(j: &serde_json::Value) -> Result<Cause, String> {
    let arr = j.as_array().ok_or("prov: malformed cause")?;
    match arr.first().and_then(|v| v.as_u64()) {
        Some(0) => Ok(Cause::Main),
        Some(1) => Ok(Cause::System {
            name: arr
                .get(1)
                .and_then(|v| v.as_str())
                .ok_or("prov: malformed system cause")?
                .into(),
        }),
        Some(2) => Ok(Cause::Handler {
            event: arr
                .get(1)
                .and_then(|v| v.as_str())
                .ok_or("prov: malformed handler cause")?
                .into(),
            emit_id: arr
                .get(2)
                .and_then(|v| v.as_u64())
                .ok_or("prov: malformed handler cause")?,
        }),
        Some(3) => Ok(Cause::Transaction {
            name: arr
                .get(1)
                .and_then(|v| v.as_str())
                .ok_or("prov: malformed transaction cause")?
                .into(),
            parent: Box::new(decode_cause(
                arr.get(2)
                    .ok_or("prov: malformed transaction parent cause")?,
            )?),
        }),
        Some(4) if arr.len() == 8 => Ok(Cause::HostCall {
            extension: arr[1]
                .as_str()
                .ok_or("prov: malformed host extension")?
                .into(),
            generation: arr[2]
                .as_str()
                .ok_or("prov: malformed host generation")?
                .into(),
            plugin_digest: arr[3]
                .as_str()
                .ok_or("prov: malformed host plugin digest")?
                .into(),
            export: arr[4]
                .as_str()
                .ok_or("prov: malformed host export")?
                .into(),
            input_digest: arr[5]
                .as_str()
                .ok_or("prov: malformed host input digest")?
                .into(),
            output_digest: arr[6]
                .as_str()
                .ok_or("prov: malformed host output digest")?
                .into(),
            parent: Box::new(decode_cause(&arr[7])?),
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
        .filter(|a| a.len() == 7)
        .ok_or("prov: malformed section")?;
    let mut writes = Vec::new();
    for w in sections[0].as_array().ok_or("prov: malformed writes")? {
        let f = w
            .as_array()
            .filter(|a| a.len() == 10)
            .ok_or("prov: malformed write")?;
        writes.push(WriteRecord {
            frame: f[0].as_u64().ok_or("prov: malformed write")?,
            entity: if f[1].is_null() {
                None
            } else {
                Some(decode_u32(&f[1], "prov: write entity")?)
            },
            entity_name: decode_opt_str(&f[2]).map(Into::into),
            component: f[3].as_str().ok_or("prov: malformed write")?.into(),
            summary: crate::causality::WriteSummary::full(
                f[4].as_str().ok_or("prov: malformed write")?,
                f[9].as_array()
                    .ok_or("prov: malformed write fields")?
                    .iter()
                    .map(|entry| -> Result<_, String> {
                        let fields = entry
                            .as_array()
                            .filter(|fields| fields.len() == 2)
                            .ok_or_else(|| "prov: malformed write field".to_string())?;
                        Ok((
                            fields[0]
                                .as_str()
                                .ok_or_else(|| "prov: malformed write field".to_string())?
                                .into(),
                            crate::causality::CausalScalar::Text(
                                fields[1]
                                    .as_str()
                                    .ok_or_else(|| "prov: malformed write field".to_string())?
                                    .into(),
                            ),
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?
                    .into(),
            ),
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
    let mut relation_assertions = Vec::new();
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
    for assertion in sections[5]
        .as_array()
        .ok_or("prov: malformed relation assertions")?
    {
        let fields = assertion
            .as_array()
            .filter(|fields| fields.len() == 5)
            .ok_or("prov: malformed relation assertion")?;
        relation_assertions.push(RelationAssertionRecord {
            frame: fields[0]
                .as_u64()
                .ok_or("prov: malformed relation assertion")?,
            assertion_id: fields[1]
                .as_u64()
                .ok_or("prov: malformed relation assertion")?,
            fact_key: crate::relation::runtime::fact_key_from_transport_hex(
                fields[2]
                    .as_str()
                    .ok_or("prov: malformed relation assertion")?,
            )
            .map_err(|error| error.to_string())?,
            resolution_ids: fields[3]
                .as_array()
                .ok_or("prov: malformed relation assertion")?
                .iter()
                .map(|id| id.as_u64().ok_or("prov: malformed relation assertion"))
                .collect::<Result<Vec<_>, _>>()?,
            origin: decode_opt_str(&fields[4]),
        });
    }
    let truncation = sections[6]
        .as_array()
        .filter(|fields| fields.len() == 2)
        .ok_or("prov: malformed truncation marker")?;
    let evicted_records = truncation[0]
        .as_u64()
        .ok_or("prov: malformed truncation count")?;
    let digest = blake3::Hash::from_hex(
        truncation[1]
            .as_str()
            .ok_or("prov: malformed truncation digest")?,
    )
    .map_err(|_| "prov: malformed truncation digest")?;
    if (evicted_records == 0) != (digest.as_bytes() == &[0; 32]) {
        return Err("prov: inconsistent truncation marker".to_string());
    }
    Ok(WireProvenance {
        origin: String::new(),
        writes,
        emits,
        settlements,
        proposals,
        resolutions,
        relation_assertions,
        truncation: ProvenanceTruncation {
            evicted_records,
            digest: *digest.as_bytes(),
        },
    })
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
        "n" => decode_native_scalar(payload).map(MapKey::Native),
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

/// Decode one wire value into the given allocator (gc heap for transient
/// values, `PersistentStore` for values that live in snapshots).
pub(crate) fn decode_value(gc: &mut dyn Allocator, j: &serde_json::Value) -> Result<Value, String> {
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
                "n" => Ok(Value::from_native_scalar(gc, decode_native_scalar(body)?)),
                "q" => {
                    let parts = body
                        .as_array()
                        .filter(|parts| parts.len() == 2)
                        .ok_or_else(|| bad("state"))?;
                    let machine = parts[0]
                        .as_str()
                        .ok_or_else(|| bad("state machine"))?
                        .to_string();
                    let state = parts[1]
                        .as_str()
                        .ok_or_else(|| bad("state name"))?
                        .to_string();
                    Ok(Value::from_state(gc, machine, state))
                }
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

fn decode_native_scalar(
    body: &serde_json::Value,
) -> Result<crate::native_types::NativeScalarValue, String> {
    let parts = body
        .as_array()
        .filter(|parts| parts.len() == 4)
        .ok_or_else(|| "wire codec: malformed native scalar".to_string())?;
    let type_name = parts[0]
        .as_str()
        .ok_or_else(|| "wire codec: malformed native type name".to_string())?
        .to_string();
    let repr_name = parts[1]
        .as_str()
        .ok_or_else(|| "wire codec: malformed native representation".to_string())?;
    let repr = crate::native_types::NativeScalarKind::parse(repr_name)
        .ok_or_else(|| format!("wire codec: unknown native representation '{repr_name}'"))?;
    let flavor_name = parts[2]
        .as_str()
        .ok_or_else(|| "wire codec: malformed native flavor".to_string())?;
    let flavor = crate::native_types::NativeTypeFlavor::parse(flavor_name)
        .ok_or_else(|| format!("wire codec: unknown native flavor '{flavor_name}'"))?;
    let bits = parts[3]
        .as_u64()
        .ok_or_else(|| "wire codec: malformed native bits".to_string())?;
    Ok(crate::native_types::NativeScalarValue {
        type_name: type_name.into(),
        repr,
        flavor,
        bits,
    })
}

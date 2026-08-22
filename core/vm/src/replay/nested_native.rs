// Native calls made by parallel speculative workers need a deterministic
// sub-tape. The main trace cannot be consumed concurrently: thread scheduling
// is not semantic order. Each logical lane therefore records and replays its
// own ordered calls, while the parent stores all lanes in index order.

#[derive(Clone, Debug)]
pub(crate) struct NestedNativeCall {
    boundary: String,
    args_digest: String,
    result: Result<serde_json::Value, String>,
}

#[derive(Debug)]
pub(crate) enum NestedNativeTape {
    Recording(Vec<NestedNativeCall>),
    Replaying {
        calls: Vec<NestedNativeCall>,
        cursor: usize,
    },
}

impl NestedNativeTape {
    pub(crate) fn recording() -> Self {
        Self::Recording(Vec::new())
    }

    pub(crate) fn replaying(calls: Vec<NestedNativeCall>) -> Self {
        Self::Replaying { calls, cursor: 0 }
    }

    pub(crate) fn replay_result(
        &mut self,
        boundary: &str,
        args_digest: &str,
    ) -> Result<Option<Result<serde_json::Value, String>>, String> {
        let Self::Replaying { calls, cursor } = self else {
            return Ok(None);
        };
        let call = calls.get(*cursor).ok_or_else(|| {
            format!(
                "replay divergence in nested worker lane: {}(args {}) has no recorded native answer",
                boundary, args_digest
            )
        })?;
        if call.boundary != boundary || call.args_digest != args_digest {
            return Err(format!(
                "replay divergence in nested worker lane at native call #{}: recorded {}(args {}), replayed {}(args {})",
                *cursor, call.boundary, call.args_digest, boundary, args_digest
            ));
        }
        *cursor += 1;
        Ok(Some(call.result.clone()))
    }

    pub(crate) fn record_result(
        &mut self,
        boundary: String,
        args_digest: String,
        result: Result<serde_json::Value, String>,
    ) {
        if let Self::Recording(calls) = self {
            calls.push(NestedNativeCall {
                boundary,
                args_digest,
                result,
            });
        }
    }

    pub(crate) fn finish(self) -> Result<Vec<NestedNativeCall>, String> {
        match self {
            Self::Recording(calls) => Ok(calls),
            Self::Replaying { calls, cursor } if cursor == calls.len() => Ok(Vec::new()),
            Self::Replaying { calls, cursor } => Err(format!(
                "replay divergence in nested worker lane: {} recorded native call(s) were not consumed",
                calls.len() - cursor
            )),
        }
    }
}

pub(crate) fn encode_nested_native_lanes(
    lanes: &[Vec<NestedNativeCall>],
) -> serde_json::Value {
    let lanes = lanes
        .iter()
        .map(|calls| {
            calls
                .iter()
                .map(|call| {
                    let mut encoded = serde_json::json!({
                        "b": call.boundary,
                        "a": call.args_digest,
                    });
                    match &call.result {
                        Ok(value) => encoded["r"] = value.clone(),
                        Err(error) => encoded["e"] = serde_json::Value::String(error.clone()),
                    }
                    encoded
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    serde_json::json!({"version": 1, "lanes": lanes})
}

pub(crate) fn decode_nested_native_lanes(
    encoded: &serde_json::Value,
    expected_lanes: usize,
) -> Result<Vec<Vec<NestedNativeCall>>, String> {
    if encoded.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err("replay trace contains an unsupported nested-native tape version".into());
    }
    let lanes = encoded
        .get("lanes")
        .and_then(serde_json::Value::as_array)
        .ok_or("replay trace contains a malformed nested-native lane table")?;
    if lanes.len() != expected_lanes {
        return Err(format!(
            "replay divergence at nested worker boundary: recorded {} native lane(s), replayed {}",
            lanes.len(), expected_lanes
        ));
    }
    lanes
        .iter()
        .enumerate()
        .map(|(lane_index, lane)| {
            lane.as_array()
                .ok_or_else(|| {
                    format!("replay trace nested-native lane {lane_index} is not a list")
                })?
                .iter()
                .enumerate()
                .map(|(call_index, call)| {
                    let boundary = call
                        .get("b")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            format!(
                                "replay trace nested-native lane {lane_index} call {call_index} has no boundary"
                            )
                        })?
                        .to_string();
                    let args_digest = call
                        .get("a")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            format!(
                                "replay trace nested-native lane {lane_index} call {call_index} has no argument digest"
                            )
                        })?
                        .to_string();
                    let error = call.get("e");
                    let success = call.get("r");
                    if error.is_some() == success.is_some() {
                        return Err(format!(
                            "replay trace nested-native lane {lane_index} call {call_index} must contain exactly one of result or error"
                        ));
                    }
                    let result = match error {
                        Some(error) => Err(error
                            .as_str()
                            .ok_or_else(|| {
                                format!(
                                    "replay trace nested-native lane {lane_index} call {call_index} has a non-string error"
                                )
                            })?
                            .to_string()),
                        None => Ok(success.expect("exclusive success branch").clone()),
                    };
                    Ok(NestedNativeCall {
                        boundary,
                        args_digest,
                        result,
                    })
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod nested_native_tape_tests {
    #[test]
    fn nested_native_decoder_rejects_ambiguous_outcomes() {
        let encoded = serde_json::json!({
            "version": 1,
            "lanes": [[{
                "b": "native:v1:echo",
                "a": "arguments",
                "r": 7,
                "e": "also failed"
            }]]
        });
        let error = super::decode_nested_native_lanes(&encoded, 1)
            .expect_err("one call cannot be both a success and an error");
        assert!(error.contains("exactly one of result or error"));
    }
}

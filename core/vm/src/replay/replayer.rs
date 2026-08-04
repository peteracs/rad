

impl TraceReplayer {
    /// Parse a JSONL trace. Refuses tampered traces (embedded source vs
    /// `source_hash`) unless `force` is set.
    pub fn parse(jsonl: &str, force: bool) -> Result<Self, String> {
        let mut lines = jsonl.lines().filter(|l| !l.trim().is_empty());
        let header: serde_json::Value = lines
            .next()
            .ok_or("trace is empty")
            .and_then(|l| serde_json::from_str(l).map_err(|_| "trace header is not valid JSON"))
            .map_err(|e| e.to_string())?;
        if header["t"] != "header" {
            return Err("trace does not start with a header record".into());
        }
        let version = header["version"].as_u64().unwrap_or(0);
        if version != TRACE_VERSION {
            return Err(format!(
                "trace version {} is not supported (expected {})",
                version, TRACE_VERSION
            ));
        }
        let source = header["source"]
            .as_str()
            .ok_or("trace header has no embedded source")?
            .to_string();
        let recorded_hash = header["source_hash"].as_str().unwrap_or_default();
        if source_hash(&source) != recorded_hash && !force {
            return Err(
                "trace integrity check failed: embedded source does not match source_hash \
                 (the trace was modified). Use --force to replay it anyway."
                    .into(),
            );
        }
        let source_layout = match header.get("source_layout") {
            None => SourceLayout::default(),
            Some(value) => serde_json::from_value::<SourceLayout>(value.clone())
                .map_err(|error| format!("trace source layout is invalid: {error}"))?,
        };
        source_layout
            .validate(&source)
            .map_err(|error| format!("trace source layout is invalid: {error}"))?;
        if let Some(version) = header.get("source_layout_version") {
            if version.as_u64() != Some(SOURCE_LAYOUT_VERSION as u64) && !force {
                return Err(format!(
                    "unsupported trace source layout version {}; expected {}",
                    version, SOURCE_LAYOUT_VERSION
                ));
            }
        }
        if let Some(recorded_layout_hash) = header
            .get("source_layout_hash")
            .and_then(|value| value.as_str())
        {
            if source_layout.digest(&source)? != recorded_layout_hash && !force {
                return Err(
                    "trace integrity check failed: source layout does not match source_layout_hash. Use --force to replay it anyway."
                        .into(),
                );
            }
        } else if !source_layout.sections.is_empty() && !force {
            return Err("trace source layout is not protected by a source_layout_hash".into());
        }
        let features = match header.get("features") {
            None => Vec::new(),
            Some(value) => value
                .as_array()
                .ok_or("trace header features must be an array")?
                .iter()
                .map(|feature| {
                    feature
                        .as_str()
                        .map(str::to_string)
                        .ok_or("trace header feature names must be strings")
                })
                .collect::<Result<Vec<_>, _>>()?,
        };
        let features = canonical_features(&features);
        if let Some(recorded_feature_hash) = header.get("feature_hash").and_then(|v| v.as_str()) {
            if feature_hash(&features) != recorded_feature_hash && !force {
                return Err(
                    "trace integrity check failed: features do not match feature_hash. Use --force to replay it anyway."
                        .into(),
                );
            }
        } else if !features.is_empty() && !force {
            return Err("trace feature list is not protected by a feature_hash".into());
        }
        let seed = header["seed"].as_u64().ok_or("trace header has no seed")?;

        let mut records = Vec::new();
        let mut frame_starts = std::collections::BTreeMap::new();
        let mut end_world_digest = None;
        let mut end_error = None;
        let mut total_frames = 0u64;
        for (i, line) in lines.enumerate() {
            let j: serde_json::Value = serde_json::from_str(line)
                .map_err(|e| format!("trace line {} is not valid JSON: {}", i + 2, e))?;
            match j["t"].as_str() {
                Some("io") => {
                    let frame = j["f"].as_u64().ok_or("io record missing frame")?;
                    let rec = IoRecord {
                        frame,
                        seq: j["s"].as_u64().unwrap_or(0),
                        builtin: j["b"].as_str().unwrap_or_default().to_string(),
                        args_digest: j["a"].as_str().unwrap_or_default().to_string(),
                        result: match j.get("e") {
                            Some(e) => Err(e.as_str().unwrap_or_default().to_string()),
                            None => Ok(j["r"].clone()),
                        },
                    };
                    frame_starts.entry(frame).or_insert(records.len());
                    records.push(rec);
                }
                Some("frame") => total_frames += 1,
                Some("end") => {
                    end_world_digest = j["world"].as_str().map(|s| s.to_string());
                    if let Some(outcome) = j.get("outcome") {
                        end_error =
                            if outcome.get("ok").and_then(|value| value.as_bool()) == Some(true) {
                                Some(None)
                            } else if let Some(message) =
                                outcome.get("error").and_then(|value| value.as_str())
                            {
                                Some(Some(message.to_string()))
                            } else {
                                return Err("trace end record has malformed outcome".to_string());
                            };
                    }
                }
                other => {
                    return Err(format!(
                        "trace line {}: unknown record type {:?}",
                        i + 2,
                        other
                    ))
                }
            }
        }
        Ok(Self {
            source,
            source_layout,
            features,
            seed,
            records,
            frame_starts,
            cursor: 0,
            current_frame: 0,
            stop_at_frame: None,
            total_frames,
            end_world_digest,
            end_error,
            mode: ReplayMode::Strict,
            capture_timeline: false,
            timeline: Vec::new(),
        })
    }

    pub fn features(&self) -> &[String] {
        &self.features
    }

    pub fn source_layout(&self) -> &SourceLayout {
        &self.source_layout
    }

    /// Switch to retroactive mode: recorded io
    /// becomes an args-keyed oracle so *edited* source can be replayed
    /// against the original session's inputs.
    pub fn into_retro(mut self) -> Self {
        let mut oracle: HashMap<(String, String), std::collections::VecDeque<IoRecord>> =
            HashMap::new();
        for rec in &self.records {
            oracle
                .entry((rec.builtin.clone(), rec.args_digest.clone()))
                .or_default()
                .push_back(rec.clone());
        }
        self.mode = ReplayMode::Retro {
            oracle,
            last_served: HashMap::new(),
            reused: 0,
            virtualized: 0,
        };
        self
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn stop_at(&mut self, frame: u64) {
        self.stop_at_frame = Some(frame);
    }

    /// Frame boundaries recorded in this trace — the highest index
    /// `--to-frame` can honour.
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Range-check a `--to-frame` request. `0..=total_frames` are honest
    /// stop points (0 = stop before anything runs); anything beyond the last
    /// recorded frame boundary can never trigger the stop sentinel — the old
    /// behaviour silently replayed the whole trace and printed "Replay
    /// verified" for a request it did not honour (dogfood finding).
    pub fn validate_stop_frame(&self, frame: u64) -> Result<(), String> {
        if frame > self.total_frames {
            return Err(format!(
                "--to-frame {} is beyond the end of this trace: it records {} frame \
                 boundar{} (valid stop points: 0..={})",
                frame,
                self.total_frames,
                if self.total_frames == 1 { "y" } else { "ies" },
                self.total_frames
            ));
        }
        Ok(())
    }

    /// Enable per-frame keyframing for time-travel sessions.
    pub fn enable_timeline_capture(&mut self) {
        self.capture_timeline = true;
    }

    pub fn capturing_timeline(&self) -> bool {
        self.capture_timeline
    }

    pub fn push_timeline_snapshot(&mut self, snap: crate::world::WorldSnapshot) {
        self.timeline.push(std::sync::Arc::new(snap));
    }

    /// The captured timeline: `timeline()[k]` = world at start of frame `k`,
    /// last entry = world at program end.
    pub fn take_timeline(&mut self) -> Vec<std::sync::Arc<crate::world::WorldSnapshot>> {
        std::mem::take(&mut self.timeline)
    }

    pub fn io_record_count(&self) -> usize {
        self.records.len()
    }

    pub fn end_world_digest(&self) -> Option<&str> {
        self.end_world_digest.as_deref()
    }

    /// Serve the next io record. Strict mode halts loudly on any divergence
    /// between the recorded and replayed timelines; retro mode answers from
    /// the args-keyed oracle.
    pub fn next_io(&mut self, builtin: &str, args_digest: &str) -> Result<IoRecord, String> {
        match &mut self.mode {
            ReplayMode::Strict => {
                let rec = self.records.get(self.cursor).ok_or_else(|| {
                    format!(
                        "replay divergence at frame {}: the replayed run calls {}() but the \
                         recorded run performed no further io",
                        self.current_frame, builtin
                    )
                })?;
                if rec.builtin != builtin
                    || rec.args_digest != args_digest
                    || rec.frame != self.current_frame
                {
                    return Err(format!(
                        "replay divergence at frame {}, record #{}: recorded {}(args {}) in frame {}, \
                         replayed {}(args {})",
                        self.current_frame,
                        self.cursor,
                        rec.builtin,
                        rec.args_digest,
                        rec.frame,
                        builtin,
                        args_digest
                    ));
                }
                let rec = rec.clone();
                self.cursor += 1;
                Ok(rec)
            }
            ReplayMode::Retro {
                oracle,
                last_served,
                reused,
                virtualized,
            } => {
                let key = (builtin.to_string(), args_digest.to_string());
                if let Some(queue) = oracle.get_mut(&key) {
                    if let Some(rec) = queue.pop_front() {
                        last_served.insert(key, rec.clone());
                        self.cursor += 1;
                        return Ok(rec);
                    }
                }
                if let Some(rec) = last_served.get(&key) {
                    // Repeatable read: the key was recorded, just fewer
                    // times than the edited code asks for.
                    *reused += 1;
                    return Ok(rec.clone());
                }
                if is_virtualizable_write(builtin) {
                    // The edit changed what the program writes (payload or
                    // path). Writes consume nothing from the recorded world,
                    // so there is nothing to fabricate: suppress the side
                    // effect (replay never performs real io) and return the
                    // builtin's success value.
                    *virtualized += 1;
                    return Ok(IoRecord {
                        frame: self.current_frame,
                        seq: 0,
                        builtin: builtin.to_string(),
                        args_digest: args_digest.to_string(),
                        result: Ok(serde_json::json!({"t": "nil"})),
                    });
                }
                // A read the recorded world never answered is a genuine
                // hole. Say precisely which kind: "same builtin, different
                // arguments" sends the user to their edit; "never called"
                // sends them to the recording.
                let same_builtin = self.records.iter().filter(|r| r.builtin == builtin).count();
                if same_builtin > 0 {
                    Err(format!(
                        "retroactive replay hole at frame {}: the edited program calls \
                         {}(args {}) — the recorded session called {}() {} time(s) but never \
                         with these arguments, and replay cannot fabricate answers from a \
                         world it never saw",
                        self.current_frame, builtin, args_digest, builtin, same_builtin
                    ))
                } else {
                    Err(format!(
                        "retroactive replay hole at frame {}: the edited program calls {}() \
                         but the recorded session never called it — replay cannot fabricate \
                         answers from a world it never saw",
                        self.current_frame, builtin
                    ))
                }
            }
        }
    }

    /// Advance the frame counter at a main-timeline `flush_events` flip.
    /// Returns the stop sentinel when `--to-frame` is reached.
    pub fn advance_frame(&mut self) -> Option<String> {
        self.current_frame += 1;
        if self.stop_at_frame == Some(self.current_frame) {
            return Some(format!("{} {}", REPLAY_STOP_PREFIX, self.current_frame));
        }
        None
    }

    /// Reposition the io cursor to the first record of `frame` — used by
    /// keyframe seeking: restore a snapshot, seek the cursor, re-execute.
    pub fn seek_frame(&mut self, frame: u64) {
        self.current_frame = frame;
        self.cursor = self
            .frame_starts
            .range(frame..)
            .next()
            .map(|(_, &idx)| idx)
            .unwrap_or(self.records.len());
    }

    pub fn report(&self, world_digest: &str) -> ReplayReport {
        self.report_with_outcome(world_digest, None)
    }

    pub fn report_with_outcome(
        &self,
        world_digest: &str,
        replay_error: Option<&str>,
    ) -> ReplayReport {
        ReplayReport {
            frames_replayed: self.current_frame,
            io_replayed: self.cursor,
            leftover_io: self.records.len() - self.cursor,
            end_digest_match: self.end_world_digest.as_ref().map(|d| d == world_digest),
            end_outcome_match: self
                .end_error
                .as_ref()
                .map(|expected| expected.as_deref() == replay_error),
            reused_reads: match &self.mode {
                ReplayMode::Strict => 0,
                ReplayMode::Retro { reused, .. } => *reused,
            },
            virtual_writes: match &self.mode {
                ReplayMode::Strict => 0,
                ReplayMode::Retro { virtualized, .. } => *virtualized,
            },
        }
    }
}
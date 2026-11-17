// The options of `explore`: where a goal, a score and a world's identity are
// read from, and the knobs that change speed and memory but never a result.

/// How many worlds at the head of the frontier one round looks at, unless
/// `"batch"` says otherwise. It bounds the work thrown away when a goal turns
/// up early; it never changes a result.
const EXPLORE_BATCH: usize = 256;
const EXPLORE_MAX_BATCH: i64 = 65_536;

/// How many successors a worker may expand past the world it was given,
/// unless `"ahead"` says otherwise. Only a scored search uses it.
const EXPLORE_AHEAD: usize = 32;
const EXPLORE_MAX_AHEAD: i64 = 4_096;

/// Distinct worlds `explore` holds unless `"limit"` says otherwise.
const EXPLORE_DEFAULT_LIMIT: u64 = 1_000_000;

const EXPLORE_OPTIONS: &str = "\"goal\", \"prune\", \"score\", \"key\", \"collect\", \"limit\", \"depth\", \"batch\", \"ahead\"";

/// One `"Resource.field"` option.
struct ExploreField {
    option: &'static str,
    resource: String,
    field: String,
}

impl ExploreField {
    fn parse(option: &'static str, value: &Value) -> Result<Self, String> {
        let shape = || {
            format!(
                "explore() option \"{option}\" must be a string naming a resource field, like \"Board.solved\""
            )
        };
        let text = value.as_str().ok_or_else(shape)?;
        let (resource, field) = text.rsplit_once('.').ok_or_else(shape)?;
        if resource.is_empty() || field.is_empty() {
            return Err(shape());
        }
        Ok(Self {
            option,
            resource: resource.to_string(),
            field: field.to_string(),
        })
    }

    fn read(&self, world: &crate::world::WorldSnapshot) -> Result<Value, String> {
        let row = world.resource_ref(&self.resource).ok_or_else(|| {
            format!(
                "explore(): option \"{}\" reads resource '{}', which a reached world does not have",
                self.option, self.resource
            )
        })?;
        let index = row
            .layout
            .iter()
            .position(|name| name == &self.field)
            .ok_or_else(|| {
                format!(
                    "explore(): option \"{}\" reads '{}.{}', but resource '{}' has no field '{}'",
                    self.option, self.resource, self.field, self.resource, self.field
                )
            })?;
        Ok(row.values[index])
    }

    fn wrong_type(&self, wanted: &str, value: &Value) -> String {
        format!(
            "explore(): option \"{}\" needs '{}.{}' to be {}, got {}",
            self.option,
            self.resource,
            self.field,
            wanted,
            value.type_name()
        )
    }

    fn read_bool(&self, world: &crate::world::WorldSnapshot) -> Result<bool, String> {
        let value = self.read(world)?;
        value
            .as_bool()
            .ok_or_else(|| self.wrong_type("a bool", &value))
    }

    fn read_int(&self, world: &crate::world::WorldSnapshot) -> Result<i64, String> {
        let value = self.read(world)?;
        value
            .as_int()
            .ok_or_else(|| self.wrong_type("an int", &value))
    }

    /// A str or an int, in a form that can leave the worker that read it.
    fn read_label(&self, world: &crate::world::WorldSnapshot) -> Result<MoveLabel, String> {
        let value = self.read(world)?;
        MoveLabel::from_value(&value).ok_or_else(|| self.wrong_type("a str or an int", &value))
    }
}

struct ExploreOptions {
    goal: Option<ExploreField>,
    prune: Option<ExploreField>,
    score: Option<ExploreField>,
    key: Option<ExploreField>,
    /// Read from every world that is reached, and returned as a list.
    collect: Option<ExploreField>,
    limit: u64,
    depth: u32,
    batch: usize,
    ahead: usize,
}

impl ExploreOptions {
    fn parse(options: Option<&Value>) -> Result<Self, String> {
        let mut parsed = Self {
            goal: None,
            prune: None,
            score: None,
            key: None,
            collect: None,
            limit: EXPLORE_DEFAULT_LIMIT,
            depth: u32::MAX,
            batch: EXPLORE_BATCH,
            ahead: EXPLORE_AHEAD,
        };
        let Some(options) = options else {
            return Ok(parsed);
        };
        let entries = options.as_map().ok_or_else(|| {
            format!(
                "explore() third argument must be a map of options ({EXPLORE_OPTIONS}), got {}",
                options.type_name()
            )
        })?;
        for (name, value) in entries.iter() {
            let MapKey::Str(name) = name else {
                return Err(format!(
                    "explore() option names are strings ({EXPLORE_OPTIONS})"
                ));
            };
            match name.as_str() {
                "goal" => parsed.goal = Some(ExploreField::parse("goal", value)?),
                "prune" => parsed.prune = Some(ExploreField::parse("prune", value)?),
                "score" => parsed.score = Some(ExploreField::parse("score", value)?),
                "key" => parsed.key = Some(ExploreField::parse("key", value)?),
                "collect" => parsed.collect = Some(ExploreField::parse("collect", value)?),
                "limit" => {
                    let limit = value.as_int().filter(|limit| *limit > 0).ok_or_else(|| {
                        "explore() option \"limit\" must be a positive int: the number of distinct worlds to hold".to_string()
                    })?;
                    parsed.limit = limit as u64;
                }
                "depth" => {
                    let depth = value.as_int().filter(|depth| *depth >= 0).ok_or_else(|| {
                        "explore() option \"depth\" must be a non-negative int: the longest path to try".to_string()
                    })?;
                    parsed.depth = u32::try_from(depth).unwrap_or(u32::MAX);
                }
                "batch" => {
                    let batch = value
                        .as_int()
                        .filter(|batch| (1..=EXPLORE_MAX_BATCH).contains(batch))
                        .ok_or_else(|| {
                            format!(
                                "explore() option \"batch\" must be an int from 1 to {EXPLORE_MAX_BATCH}: how many worlds to expand ahead in parallel"
                            )
                        })?;
                    parsed.batch = batch as usize;
                }
                "ahead" => {
                    let ahead = value
                        .as_int()
                        .filter(|ahead| (0..=EXPLORE_MAX_AHEAD).contains(ahead))
                        .ok_or_else(|| {
                            format!(
                                "explore() option \"ahead\" must be an int from 0 to {EXPLORE_MAX_AHEAD}: how many successors a worker may expand past the world it was given"
                            )
                        })?;
                    parsed.ahead = ahead as usize;
                }
                other => {
                    return Err(format!(
                        "explore() has no option \"{other}\"; the options are {EXPLORE_OPTIONS}"
                    ));
                }
            }
        }
        Ok(parsed)
    }

    /// The name two worlds share exactly when `explore` treats them as one.
    ///
    /// With a `"key"` it is that field. Without one it is the content digest
    /// (the `world_digest` recipe) together with the events and timers still
    /// in flight, which are program state the digest leaves out. The
    /// operational fingerprint is deliberately not used: it covers allocator
    /// state, so two routes to one position would never meet.
    fn identity(
        &self,
        world: &std::sync::Arc<crate::world::WorldSnapshot>,
        transient: &HashSet<String>,
    ) -> Result<u128, String> {
        fn part(hasher: &mut blake3::Hasher, bytes: &[u8]) {
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        }
        let mut hasher = blake3::Hasher::new();
        match &self.key {
            Some(key) => {
                let value = key.read(world)?;
                if let Some(number) = value.as_int() {
                    hasher.update(b"rad.explore.key.int.v1");
                    hasher.update(&number.to_le_bytes());
                } else if let Some(text) = value.as_str() {
                    hasher.update(b"rad.explore.key.str.v1");
                    hasher.update(text.as_bytes());
                } else {
                    return Err(key.wrong_type("a str or an int", &value));
                }
            }
            None => {
                hasher.update(b"rad.explore.world.v1");
                part(&mut hasher, VM::fork_digest(world, transient)?.as_bytes());
                hasher.update(&(world.events.len() as u64).to_le_bytes());
                for (name, payload, _) in world.events.iter() {
                    part(&mut hasher, name.as_bytes());
                    part(&mut hasher, payload.to_string().as_bytes());
                }
                for (ticks, name, payload, _) in world.delayed.iter() {
                    hasher.update(&ticks.to_le_bytes());
                    part(&mut hasher, name.as_bytes());
                    part(&mut hasher, payload.to_string().as_bytes());
                }
            }
        }
        let digest = hasher.finalize();
        let mut prefix = [0_u8; 16];
        prefix.copy_from_slice(&digest.as_bytes()[..16]);
        Ok(u128::from_le_bytes(prefix))
    }
}

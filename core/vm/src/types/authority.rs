/// The authority-visible effects of one callable. Names are canonical
/// component/resource/event names; `"*"` denotes an operation over the whole
/// entity/world set (for example `entities()` or `despawn()`). Vectors are
/// sorted and deduplicated so diagnostics and JSON remain deterministic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AuthorityEffects {
    pub reads: Vec<String>,
    pub writes: Vec<String>,
    pub emits: Vec<String>,
    pub io: bool,
    #[serde(rename = "async")]
    pub async_effect: bool,
    /// A first-class function value was invoked without a statically bounded
    /// target. Restricted callers reject this instead of assuming purity.
    pub unknown: bool,
    /// Reachable world-wide iteration, including scans hidden behind helpers.
    pub full_scan: bool,
    /// Reachable heap allocation, including collection-producing builtins.
    pub allocates: bool,
    /// Query operations reachable in this effect set. These are closed,
    /// compiler-produced plan facts rather than source snippets.
    pub queries: Vec<AuthorityQueryOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityQueryComplexity {
    Constant,
    Logarithmic,
    OutputLinear,
    PopulationLinear,
    PopulationLogLinear,
}

impl fmt::Display for AuthorityQueryComplexity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Constant => "O(1)",
            Self::Logarithmic => "O(log n)",
            Self::OutputLinear => "O(k)",
            Self::PopulationLinear => "O(n)",
            Self::PopulationLogLinear => "O(n log n)",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct AuthorityQueryOperation {
    pub operation: String,
    pub source: String,
    pub complexity: AuthorityQueryComplexity,
    pub allocates: bool,
}

impl AuthorityEffects {
    pub fn is_empty(&self) -> bool {
        self.reads.is_empty()
            && self.writes.is_empty()
            && self.emits.is_empty()
            && !self.io
            && !self.async_effect
            && !self.unknown
            && !self.full_scan
            && !self.allocates
            && self.queries.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityCallableKind {
    Function,
    System,
    Transaction,
    PostCommit,
    Handler,
    Closure,
}

impl fmt::Display for AuthorityCallableKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Function => write!(f, "function"),
            Self::System => write!(f, "system"),
            Self::Transaction => write!(f, "transaction"),
            Self::PostCommit => write!(f, "post_commit"),
            Self::Handler => write!(f, "handler"),
            Self::Closure => write!(f, "closure"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CallableAuthority {
    /// Stable graph key. Imported declarations use their checker-mangled name
    /// so two modules cannot collide.
    pub name: String,
    /// Human spelling (`physics.integrate`, `on Damage#1`, or a closure site).
    pub display_name: String,
    pub kind: AuthorityCallableKind,
    pub direct: AuthorityEffects,
    /// Effects reachable before crossing an event or separately scheduled
    /// system boundary. System roots carry call-site-specialized callback
    /// effects; generic callable reports may conservatively union their known
    /// callback targets. Enforcement and parallel batching use the specialized
    /// system-root set.
    pub synchronous: AuthorityEffects,
    pub transitive: AuthorityEffects,
    /// Cost/lifecycle contracts declared on this callable. Keeping these on
    /// the checked graph lets operational tooling report the exact contract
    /// that enforcement consumed without rescanning source text.
    pub contracts: AuthorityContracts,
    /// Canonical graph keys in deterministic order.
    pub calls: Vec<String>,
    /// Subset of `calls` that executes at a separate event/schedule boundary.
    /// Reports follow these edges; a caller's synchronous sandbox does not.
    pub deferred_calls: Vec<String>,
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AuthorityContracts {
    pub frame: bool,
    pub tick: bool,
    pub render: bool,
    pub no_full_scan: bool,
    pub no_guest_allocation: bool,
    pub no_runtime_allocation: bool,
    pub no_host_allocation: bool,
    pub instruction_budget: Option<u64>,
    pub allow_full_scan_reason: Option<String>,
    pub no_nested_flush: bool,
    pub non_reentrant: bool,
    pub must_complete_before: Option<String>,
    pub exactly_once: bool,
}

impl From<&crate::ast::CallableContracts> for AuthorityContracts {
    fn from(contracts: &crate::ast::CallableContracts) -> Self {
        Self {
            frame: contracts.frame,
            tick: contracts.tick,
            render: contracts.render,
            no_full_scan: contracts.no_full_scan,
            no_guest_allocation: contracts.no_guest_allocation,
            no_runtime_allocation: contracts.no_runtime_allocation,
            no_host_allocation: contracts.no_host_allocation,
            instruction_budget: contracts.instruction_budget,
            allow_full_scan_reason: contracts.allow_full_scan_reason.clone(),
            no_nested_flush: contracts.no_nested_flush,
            non_reentrant: contracts.non_reentrant,
            must_complete_before: contracts.must_complete_before.clone(),
            exactly_once: contracts.exactly_once,
        }
    }
}

/// Cached authority graph plus reverse indexes. Query commands never rescan an
/// AST: a symbol lookup is logarithmic and readers/writers are direct index
/// lookups.
#[derive(Debug, Clone, Default, Serialize)]
pub struct AuthorityReport {
    pub callables: BTreeMap<String, CallableAuthority>,
    pub readers: BTreeMap<String, Vec<String>>,
    pub writers: BTreeMap<String, Vec<String>>,
}

impl AuthorityReport {
    pub fn resolve(&self, requested: &str) -> Result<&CallableAuthority, String> {
        if let Some(found) = self.callables.get(requested) {
            return Ok(found);
        }
        let mut matches = self.matching_callables(requested);
        matches.sort_by(|a, b| a.display_name.cmp(&b.display_name));
        let visible = matches
            .iter()
            .copied()
            .filter(|item| !item.name.starts_with("@specialized:"))
            .collect::<Vec<_>>();
        if let [only] = visible.as_slice() {
            if matches
                .iter()
                .all(|item| item.display_name == only.display_name)
            {
                return Ok(*only);
            }
        }
        match matches.as_slice() {
            [only] => Ok(*only),
            [] => Err(format!("unknown callable '{requested}'")),
            many => Err(format!(
                "callable '{requested}' is ambiguous: {}",
                many.iter()
                    .map(|item| item.display_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    pub fn readers_of(&self, authority: &str) -> Vec<String> {
        indexed_authority_query(&self.readers, authority)
    }

    pub fn writers_of(&self, authority: &str) -> Vec<String> {
        indexed_authority_query(&self.writers, authority)
    }

    pub fn path(&self, from: &str, to: &str) -> Result<Option<Vec<String>>, String> {
        let start = self.resolve(from)?;
        // System roots call site-specialized helpers. A bare helper can also
        // be the key of its conservative generic report, so an exact lookup
        // alone would hide the specialized nodes and incorrectly report no
        // path. Resolve the complete display-name family and retain an exact
        // internal key when one was supplied.
        let mut goals = self.matching_callables(to);
        if let Some(exact) = self.callables.get(to) {
            if goals.iter().all(|candidate| candidate.name != exact.name) {
                goals.push(exact);
            }
        }
        if goals.is_empty() {
            return Err(format!("unknown callable '{to}'"));
        }
        let goal_displays = goals
            .iter()
            .map(|item| item.display_name.as_str())
            .collect::<HashSet<_>>();
        if goal_displays.len() > 1 {
            let mut names = goal_displays.into_iter().collect::<Vec<_>>();
            names.sort();
            return Err(format!(
                "callable '{to}' is ambiguous: {}",
                names.join(", ")
            ));
        }
        let goal_names = goals
            .iter()
            .map(|item| item.name.as_str())
            .collect::<HashSet<_>>();
        if goal_names.contains(start.name.as_str()) {
            return Ok(Some(vec![start.display_name.clone()]));
        }
        let mut queue = VecDeque::from([start.name.clone()]);
        let mut previous = HashMap::<String, String>::new();
        previous.insert(start.name.clone(), String::new());
        while let Some(current) = queue.pop_front() {
            let Some(node) = self.callables.get(&current) else {
                continue;
            };
            for next in &node.calls {
                if previous.contains_key(next) {
                    continue;
                }
                previous.insert(next.clone(), current.clone());
                if goal_names.contains(next.as_str()) {
                    let mut keys = vec![next.clone()];
                    let mut cursor = next.clone();
                    while let Some(parent) = previous.get(&cursor) {
                        if parent.is_empty() {
                            break;
                        }
                        keys.push(parent.clone());
                        cursor = parent.clone();
                    }
                    keys.reverse();
                    return Ok(Some(
                        keys.into_iter()
                            .filter_map(|key| {
                                self.callables
                                    .get(&key)
                                    .map(|node| node.display_name.clone())
                            })
                            .collect(),
                    ));
                }
                queue.push_back(next.clone());
            }
        }
        Ok(None)
    }

    /// Shortest synchronous call path from `start` to direct cost evidence.
    /// This is the same graph primitive used by authority enforcement, so
    /// `rad cost-path` cannot invent a source-only interpretation.
    pub fn effect_path_matching(
        &self,
        start: &str,
        matches: impl Fn(&AuthorityEffects) -> bool,
    ) -> Option<Vec<String>> {
        let mut queue = VecDeque::from([start.to_string()]);
        let mut previous = HashMap::<String, String>::from([(start.to_string(), String::new())]);
        while let Some(current) = queue.pop_front() {
            let node = self.callables.get(&current)?;
            if matches(&node.direct) {
                let mut keys = vec![current.clone()];
                let mut cursor = current;
                while let Some(parent) = previous.get(&cursor) {
                    if parent.is_empty() {
                        break;
                    }
                    keys.push(parent.clone());
                    cursor = parent.clone();
                }
                keys.reverse();
                return Some(
                    keys.into_iter()
                        .filter_map(|key| {
                            self.callables
                                .get(&key)
                                .map(|item| item.display_name.clone())
                        })
                        .collect(),
                );
            }
            for next in &node.calls {
                if !previous.contains_key(next) {
                    previous.insert(next.clone(), current.clone());
                    queue.push_back(next.clone());
                }
            }
        }
        None
    }

    pub fn full_scan_path(&self, requested: &str) -> Result<Option<Vec<String>>, String> {
        let start = self.resolve(requested)?;
        Ok(self.effect_path_matching(&start.name, |effects| effects.full_scan))
    }

    pub fn allocation_path(&self, requested: &str) -> Result<Option<Vec<String>>, String> {
        let start = self.resolve(requested)?;
        Ok(self.effect_path_matching(&start.name, |effects| effects.allocates))
    }

    fn matching_callables(&self, requested: &str) -> Vec<&CallableAuthority> {
        let requested_member = requested.rsplit('.').next().unwrap_or(requested);
        self.callables
            .values()
            .filter(|item| {
                item.display_name == requested
                    || item.display_name.rsplit('.').next() == Some(requested_member)
                    || item
                        .display_name
                        .strip_prefix("on ")
                        .is_some_and(|name| name == requested)
                    || item.name.rsplit("__").next() == Some(requested_member)
            })
            .collect()
    }
}

fn indexed_authority_query(index: &BTreeMap<String, Vec<String>>, authority: &str) -> Vec<String> {
    let mut names = index.get(authority).cloned().unwrap_or_default();
    if authority != "*" {
        if let Some(wildcard) = index.get("*") {
            names.extend(wildcard.iter().cloned());
        }
    }
    names.sort();
    names.dedup();
    names
}

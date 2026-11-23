// `explore`, `offer`, `follow`: exhaustive search over forked worlds.
//
// The simulate family runs one future, or a sample of futures. `explore` runs
// every future. Each listed system is a move; a move system may `offer`
// several successor worlds in one run; a world that was already reached is
// dropped; and the result says either how a goal was reached (a path of
// labels that `follow` replays) or that no reachable world is a goal.
//
// Expansion runs in parallel on the worker-VM pool, but what a program can
// observe is defined sequentially: the world with the lowest score is expanded
// next, ties in arrival order. Expanding a world is a pure function of that
// world, so it may be done early and kept. Each round expands the worlds at
// the head of the frontier that have no expansion yet, then merges in frontier
// order for as long as the next world has one. A world a merge puts at the
// head simply has none yet, and the next round expands it. The answer
// therefore depends on neither the thread count nor how far ahead a round
// looks.
//
// A scored search often puts new worlds at the head (a successor cheaper than
// everything waiting). Rounds would then be one world long. So a worker that
// expands a world goes on to expand those of its successors that would be
// placed ahead of everything outside the round's window, lowest score first,
// up to `"ahead"` of them, and hands the results back attached. A successor
// that turns out to be a world already seen drops its attachment unused.

/// Expansions kept for worlds that are not at the head yet, as a multiple of
/// the batch. Past it a round expands only the worlds that share the lowest
/// score.
const EXPLORE_KEPT_BATCHES: usize = 4;

/// What a move is called on a path: the value given to `offer`, or the name
/// of a move system that never offers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MoveLabel {
    Int(i64),
    Str(std::sync::Arc<str>),
}

impl MoveLabel {
    fn from_value(value: &Value) -> Option<Self> {
        if let Some(number) = value.as_int() {
            return Some(Self::Int(number));
        }
        value.as_str().map(|text| Self::Str(text.into()))
    }

    fn to_value(&self, gc: &mut GcHeap) -> Value {
        match self {
            Self::Int(number) => Value::from_int(gc, *number),
            Self::Str(text) => Value::from_string(gc, text.to_string()),
        }
    }
}

impl std::fmt::Display for MoveLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Int(number) => write!(f, "{number}"),
            Self::Str(text) => write!(f, "\"{text}\""),
        }
    }
}

/// The world one move starts from, and the successors offered so far.
pub(crate) struct ExploreSession {
    base: std::sync::Arc<crate::world::WorldSnapshot>,
    offers: Vec<(MoveLabel, crate::world::WorldSnapshot)>,
}

/// One world a move leads to, with everything the search needs to place it.
struct ExploreSuccessor {
    label: MoveLabel,
    world: std::sync::Arc<crate::world::WorldSnapshot>,
    identity: u128,
    score: i64,
    goal: bool,
    /// What `"collect"` reads from this world.
    collected: Option<MoveLabel>,
    /// This world's own expansion, when a worker went on to compute it.
    ahead: Option<Box<ExploreExpansion>>,
}

struct ExploreExpansion {
    successors: Vec<ExploreSuccessor>,
    pruned: u64,
}

const EXPLORE_ROOT: u32 = u32::MAX;

struct ExploreNode {
    parent: u32,
    depth: u32,
    label: MoveLabel,
    score: i64,
    /// Kept while the world waits in the frontier, and for a goal.
    world: Option<std::sync::Arc<crate::world::WorldSnapshot>>,
    /// An expansion computed ahead of this world's turn.
    expansion: Option<Box<Result<ExploreExpansion, String>>>,
}

enum ExploreStop {
    Goal(u32),
    Limit,
}

/// What one round of parallel expansion shares with its workers.
struct ExploreRound<'a> {
    /// Identities reached so far. Read-only while workers run.
    seen: &'a HashSet<u128, rustc_hash::FxBuildHasher>,
    /// The score of the first frontier world outside the round's window.
    bound: i64,
    /// Successors a worker may expand past the world it was given.
    ahead: usize,
}

/// An expansion a worker computed past the world it was given: of successor
/// `slot` of the expansion at `parent`.
struct ExploreAhead {
    parent: usize,
    slot: usize,
    depth: u32,
    expansion: ExploreExpansion,
}

impl ExploreAhead {
    /// Queue the successors of `done[at]` that a worker may go on to expand.
    fn queue_successors(
        queue: &mut std::collections::BinaryHeap<std::cmp::Reverse<(i64, usize, usize)>>,
        done: &[ExploreAhead],
        at: usize,
        depth_limit: u32,
        bound: i64,
    ) {
        // A successor of a world at the depth bound is never expanded.
        if done[at].depth.saturating_add(1) >= depth_limit {
            return;
        }
        for (slot, successor) in done[at].expansion.successors.iter().enumerate() {
            if !successor.goal && successor.score < bound {
                queue.push(std::cmp::Reverse((successor.score, at, slot)));
            }
        }
    }
}

/// Run `work` on a worker-pool thread. The pooled worker VM lives in that
/// thread's local storage, never in the caller's.
#[cfg(not(target_arch = "wasm32"))]
fn explore_on_pool<R: Send>(work: impl FnOnce() -> R + Send) -> R {
    rayon::scope(|_| work())
}

#[cfg(target_arch = "wasm32")]
fn explore_on_pool<R>(work: impl FnOnce() -> R) -> R {
    work()
}

impl VM {
    fn explore_guard(&self, caller: &str) -> Result<(), String> {
        if self.in_simulation_fork > 0 || self.is_worker || self.explore_session.is_some() {
            return Err(format!(
                "{caller}() cannot run inside a simulation, a parallel system or another exploration"
            ));
        }
        if (self.recorder.is_some() || self.replayer.is_some())
            && !self.native_extension_manifests.is_empty()
        {
            return Err(format!(
                "{caller}() does not tape native calls made by move systems, so it cannot run under record or replay while an extension is loaded"
            ));
        }
        Ok(())
    }

    fn explore_schedule(&self, value: &Value, caller: &str) -> Result<Vec<String>, String> {
        let systems = Self::schedule_from_value(value, caller)?;
        for name in &systems {
            if !self.systems.contains_key(name) {
                return Err(format!("{caller}(): unknown system '{name}'"));
            }
        }
        Ok(systems)
    }

    fn explore_rewind(&mut self, world: &crate::world::WorldSnapshot) {
        self.restore_events_from(world);
        self.get_world_mut().restore(world.clone());
    }

    /// Prepare a pooled worker to run moves, run `work`, and leave the worker
    /// empty. Mirrors the rollout setup in `simulate_par`: the worker owns a
    /// private world, so writes apply directly.
    fn explore_as_worker<R>(&mut self, work: impl FnOnce(&mut VM) -> R) -> R {
        self.nested_native_tape = None;
        let was_worker = self.is_worker;
        self.is_worker = false;
        self.in_simulation_fork += 1;
        // Move systems are checked to use no randomness; a fixed seed keeps
        // anything that slips through from depending on the pool.
        self.set_random_seed(crate::sandbox::fork_seed(0, 0));

        let result = work(self);

        self.in_simulation_fork -= 1;
        self.is_worker = was_worker;
        self.explore_session = None;
        self.stack.clear();
        self.frames.clear();
        *self.get_world_mut() = crate::world::World::new();
        self.events_current.clear();
        self.events_next.clear();
        self.emit_ids_current.clear();
        self.emit_ids_next.clear();
        self.delayed_events.clear();
        result
    }

    /// Deliver the events an offered world still carries, as one tick does.
    fn explore_settle(
        &mut self,
        offered: crate::world::WorldSnapshot,
    ) -> Result<crate::world::WorldSnapshot, String> {
        if offered.events.is_empty() && offered.delayed.is_empty() {
            return Ok(offered);
        }
        self.restore_events_from(&offered);
        self.get_world_mut().restore(offered);
        self.bi_flush_events(vec![])?;
        Ok(self.snapshot_with_events())
    }

    /// Every world one move away from `base`, in move order.
    fn explore_moves(
        &mut self,
        base: &std::sync::Arc<crate::world::WorldSnapshot>,
        systems: &[String],
        caller: &str,
    ) -> Result<Vec<(MoveLabel, crate::world::WorldSnapshot)>, String> {
        let mut moves = Vec::new();
        for name in systems {
            self.explore_rewind(base);
            self.explore_session = Some(Box::new(ExploreSession {
                base: std::sync::Arc::clone(base),
                offers: Vec::new(),
            }));
            let ran = self.run_system_by_name(name);
            let offers = self
                .explore_session
                .take()
                .map(|session| session.offers)
                .unwrap_or_default();
            ran.map_err(|error| format!("{caller}(): move system '{name}': {error}"))?;
            if offers.is_empty() {
                // A system that never offers is one move: the world it leaves.
                self.bi_flush_events(vec![])?;
                moves.push((
                    MoveLabel::Str(name.as_str().into()),
                    self.snapshot_with_events(),
                ));
            } else {
                for (label, offered) in offers {
                    moves.push((label, self.explore_settle(offered)?));
                }
            }
        }
        let mut labels = HashSet::with_capacity(moves.len());
        for (label, _) in &moves {
            if !labels.insert(label) {
                return Err(format!(
                    "{caller}(): two moves from one world are both labelled {label}; a label must name one move, or a path could not be followed"
                ));
            }
        }
        Ok(moves)
    }

    fn explore_expand(
        &mut self,
        base: &std::sync::Arc<crate::world::WorldSnapshot>,
        systems: &[String],
        options: &ExploreOptions,
    ) -> Result<ExploreExpansion, String> {
        let moves = self.explore_moves(base, systems, "explore")?;
        let transient = std::sync::Arc::clone(&self.transient_resources);
        let mut successors = Vec::with_capacity(moves.len());
        let mut pruned = 0;
        for (label, world) in moves {
            if let Some(prune) = &options.prune {
                if prune.read_bool(&world)? {
                    pruned += 1;
                    continue;
                }
            }
            let goal = match &options.goal {
                Some(goal) => goal.read_bool(&world)?,
                None => false,
            };
            let score = match &options.score {
                Some(score) => score.read_int(&world)?,
                None => 0,
            };
            let collected = match &options.collect {
                Some(collect) => Some(collect.read_label(&world)?),
                None => None,
            };
            let world = std::sync::Arc::new(world);
            let identity = options.identity(&world, &transient)?;
            successors.push(ExploreSuccessor {
                label,
                world,
                identity,
                score,
                goal,
                collected,
                ahead: None,
            });
        }
        Ok(ExploreExpansion { successors, pruned })
    }

    /// Expand `base`, then up to `plan.ahead` of the worlds below it that
    /// would be placed ahead of everything outside the round's window: those
    /// scoring under `plan.bound`. Lowest score first, as the search itself
    /// would take them. Worlds the search has already reached are skipped; a
    /// successor that fails to expand is left for the search to find again.
    fn explore_expand_ahead(
        &mut self,
        base: &std::sync::Arc<crate::world::WorldSnapshot>,
        depth: u32,
        systems: &[String],
        options: &ExploreOptions,
        plan: &ExploreRound<'_>,
    ) -> Result<ExploreExpansion, String> {
        let root = self.explore_expand(base, systems, options)?;
        if plan.ahead == 0 {
            return Ok(root);
        }
        let mut done = vec![ExploreAhead {
            parent: usize::MAX,
            slot: 0,
            depth,
            expansion: root,
        }];
        let mut queue = std::collections::BinaryHeap::new();
        let mut tried: HashSet<u128> = HashSet::new();
        ExploreAhead::queue_successors(&mut queue, &done, 0, options.depth, plan.bound);
        let mut budget = plan.ahead;
        while budget > 0 {
            let Some(std::cmp::Reverse((_, at, slot))) = queue.pop() else {
                break;
            };
            let successor = &done[at].expansion.successors[slot];
            if plan.seen.contains(&successor.identity) || !tried.insert(successor.identity) {
                continue;
            }
            budget -= 1;
            let world = std::sync::Arc::clone(&successor.world);
            let depth = done[at].depth + 1;
            if let Ok(expansion) = self.explore_expand(&world, systems, options) {
                done.push(ExploreAhead {
                    parent: at,
                    slot,
                    depth,
                    expansion,
                });
                let last = done.len() - 1;
                ExploreAhead::queue_successors(&mut queue, &done, last, options.depth, plan.bound);
            }
        }
        // An expansion is always behind the one it grew from, so folding from
        // the back attaches every expansion before its parent moves.
        while done.len() > 1 {
            let Some(last) = done.pop() else { break };
            done[last.parent].expansion.successors[last.slot].ahead =
                Some(Box::new(last.expansion));
        }
        let root = done.pop().expect("the root expansion is always kept");
        Ok(root.expansion)
    }

    /// Expand `work` (worlds with their depths) on the pool. Results come back
    /// in the order of `work`.
    fn explore_expand_batch(
        shared: &crate::vm::VmSharedState,
        systems: &[String],
        options: &ExploreOptions,
        plan: &ExploreRound<'_>,
        work: &[(std::sync::Arc<crate::world::WorldSnapshot>, u32)],
    ) -> Vec<Result<ExploreExpansion, String>> {
        let run_chunk = |chunk: &[(std::sync::Arc<crate::world::WorldSnapshot>, u32)]| {
            super::exec::with_worker_vm(shared, |worker| {
                worker.explore_as_worker(|worker| {
                    let mut results = Vec::with_capacity(chunk.len());
                    for (base, depth) in chunk {
                        let result =
                            worker.explore_expand_ahead(base, *depth, systems, options, plan);
                        let failed = result.is_err();
                        results.push(result);
                        if failed {
                            break;
                        }
                    }
                    // A failure ends the search when the merge reaches it, and
                    // the merge follows this order, so nothing after it in the
                    // chunk is ever read.
                    while results.len() < chunk.len() {
                        results.push(Err(
                            "explore(): an earlier world of this batch failed".to_string()
                        ));
                    }
                    results
                })
            })
        };
        // wasm32 has no threads: one chunk, same results.
        #[cfg(target_arch = "wasm32")]
        let results: Vec<Result<ExploreExpansion, String>> =
            work.chunks(work.len().max(1)).flat_map(run_chunk).collect();
        #[cfg(not(target_arch = "wasm32"))]
        let results: Vec<Result<ExploreExpansion, String>> = {
            use rayon::prelude::*;
            let lanes = rayon::current_num_threads().max(1) * 4;
            let chunk_len = work.len().div_ceil(lanes).max(1);
            work.par_chunks(chunk_len)
                .flat_map_iter(run_chunk)
                .collect()
        };
        results
    }

    /// `explore(fork, [system::Move, ...], options?) -> map`
    fn bi_explore(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.explore_guard("explore")?;
        let root = std::sync::Arc::clone(
            args[0]
                .as_world_fork()
                .ok_or_else(|| "explore() first argument must be a world_fork".to_string())?,
        );
        let systems = self.explore_schedule(&args[1], "explore")?;
        let options = ExploreOptions::parse(args.get(2))?;
        let scored = options.score.is_some();

        // Reading every option from the start world reports a misspelt field
        // before any search runs.
        let root_goal = match &options.goal {
            Some(goal) => goal.read_bool(&root)?,
            None => false,
        };
        let root_score = match &options.score {
            Some(score) => score.read_int(&root)?,
            None => 0,
        };
        if let Some(prune) = &options.prune {
            prune.read_bool(&root)?;
        }
        let root_identity = options.identity(&root, &self.transient_resources)?;
        // One value per world reached, in the order they were reached.
        let mut collected: Vec<MoveLabel> = Vec::new();
        if let Some(collect) = &options.collect {
            collected.push(collect.read_label(&root)?);
        }

        let mut nodes = vec![ExploreNode {
            parent: EXPLORE_ROOT,
            depth: 0,
            label: MoveLabel::Int(0),
            score: root_score,
            world: Some(root),
            expansion: None,
        }];
        let mut seen: HashSet<u128, rustc_hash::FxBuildHasher> = HashSet::default();
        seen.insert(root_identity);
        // Ordered by (score, arrival). Without a score the score is the
        // depth, which makes this a breadth-first queue.
        let mut frontier: std::collections::BTreeSet<(i64, u32)> = Default::default();
        let mut levels: Vec<u64> = vec![1];
        let mut expanded: u64 = 0;
        let mut pruned: u64 = 0;
        let mut cut_by_depth = false;
        let mut stop = None;
        if root_goal {
            stop = Some(ExploreStop::Goal(0));
        } else if options.depth > 0 {
            frontier.insert((root_score, 0));
        } else {
            cut_by_depth = true;
        }

        let shared = self.shared_state();
        // Breadth-first successors never go ahead of what is waiting.
        let ahead = if scored { options.ahead } else { 0 };
        let kept_limit = options.batch.saturating_mul(EXPLORE_KEPT_BATCHES);
        // Frontier worlds that hold an expansion.
        let mut kept: usize = 0;
        'search: while stop.is_none() {
            let Some(&(lowest, _)) = frontier.first() else {
                break;
            };
            // This round's window is the first `batch` worlds of the frontier.
            // `bound` is the score of the first world outside it.
            let mut todo: Vec<u32> = Vec::new();
            let mut bound = i64::MAX;
            for (position, &(score, index)) in frontier.iter().enumerate() {
                if position == options.batch {
                    bound = score;
                    break;
                }
                if nodes[index as usize].expansion.is_some() {
                    continue;
                }
                if score > lowest && kept + todo.len() >= kept_limit {
                    bound = score;
                    break;
                }
                todo.push(index);
            }
            if !todo.is_empty() {
                let work: Vec<_> = todo
                    .iter()
                    .map(|index| {
                        let node = &nodes[*index as usize];
                        let world = node
                            .world
                            .as_ref()
                            .expect("a frontier world keeps its snapshot");
                        (std::sync::Arc::clone(world), node.depth)
                    })
                    .collect();
                let plan = ExploreRound {
                    seen: &seen,
                    bound,
                    ahead,
                };
                let results = Self::explore_expand_batch(&shared, &systems, &options, &plan, &work);
                kept += todo.len();
                for (index, result) in todo.into_iter().zip(results) {
                    nodes[index as usize].expansion = Some(Box::new(result));
                }
            }

            // Merge in frontier order for as long as the next world is ready.
            while let Some(&(_, index)) = frontier.first() {
                let Some(expansion) = nodes[index as usize].expansion.take() else {
                    break;
                };
                frontier.pop_first();
                kept -= 1;
                let expansion = (*expansion)?;
                let depth = nodes[index as usize].depth + 1;
                for successor in expansion.successors {
                    if seen.contains(&successor.identity) {
                        continue;
                    }
                    if nodes.len() as u64 >= options.limit {
                        stop = Some(ExploreStop::Limit);
                    } else {
                        seen.insert(successor.identity);
                        collected.extend(successor.collected);
                        let reached = nodes.len() as u32;
                        let score = if scored {
                            successor.score
                        } else {
                            i64::from(depth)
                        };
                        if levels.len() <= depth as usize {
                            levels.resize(depth as usize + 1, 0);
                        }
                        levels[depth as usize] += 1;
                        let waits = !successor.goal && depth < options.depth;
                        let ready = successor
                            .ahead
                            .filter(|_| waits)
                            .map(|expansion| Box::new(Ok(*expansion)));
                        kept += usize::from(ready.is_some());
                        nodes.push(ExploreNode {
                            parent: index,
                            depth,
                            label: successor.label,
                            score,
                            world: (waits || successor.goal).then_some(successor.world),
                            expansion: ready,
                        });
                        if successor.goal {
                            stop = Some(ExploreStop::Goal(reached));
                        } else if waits {
                            frontier.insert((score, reached));
                        } else {
                            cut_by_depth = true;
                        }
                    }
                    if stop.is_some() {
                        // This world was reached and not finished: it is
                        // still frontier.
                        frontier.insert((nodes[index as usize].score, index));
                        break 'search;
                    }
                }
                nodes[index as usize].world = None;
                expanded += 1;
                pruned += expansion.pruned;
            }
        }

        let deepest = levels.len() as i64 - 1;
        let (status, goal) = match stop {
            Some(ExploreStop::Goal(index)) => ("goal", Some(index)),
            Some(ExploreStop::Limit) => ("limit", None),
            None if cut_by_depth => ("depth", None),
            None => ("exhausted", None),
        };

        let mut labels = Vec::new();
        let mut goal_depth = deepest;
        let mut result = MapStorage::new();
        if let Some(goal) = goal {
            goal_depth = i64::from(nodes[goal as usize].depth);
            let mut at = goal;
            while nodes[at as usize].parent != EXPLORE_ROOT {
                labels.push(nodes[at as usize].label.to_value(&mut self.gc));
                at = nodes[at as usize].parent;
            }
            labels.reverse();
            let world = nodes[goal as usize]
                .world
                .take()
                .expect("a goal world keeps its snapshot");
            result.insert(
                MapKey::Str("world".to_string()),
                Value::world_fork(&mut self.gc, world),
            );
        }
        result.insert(
            MapKey::Str("status".to_string()),
            Value::from_string(&mut self.gc, status.to_string()),
        );
        result.insert(
            MapKey::Str("path".to_string()),
            Value::list(&mut self.gc, labels),
        );
        for (name, count) in [
            ("seen", nodes.len() as i64),
            ("expanded", expanded as i64),
            ("frontier", frontier.len() as i64),
            ("pruned", pruned as i64),
            ("depth", goal_depth),
        ] {
            result.insert(
                MapKey::Str(name.to_string()),
                Value::from_int(&mut self.gc, count),
            );
        }
        let levels: Vec<Value> = levels
            .iter()
            .map(|count| Value::from_int(&mut self.gc, *count as i64))
            .collect();
        result.insert(
            MapKey::Str("levels".to_string()),
            Value::list(&mut self.gc, levels),
        );
        if options.collect.is_some() {
            let collected: Vec<Value> = collected
                .iter()
                .map(|value| value.to_value(&mut self.gc))
                .collect();
            result.insert(
                MapKey::Str("collected".to_string()),
                Value::list(&mut self.gc, collected),
            );
        }
        Ok(Value::map(&mut self.gc, result))
    }

    /// `offer(label)`: inside a move system, record the world as it is now as
    /// one successor and rewind to the world the move started from.
    fn bi_offer(&mut self, args: Vec<Value>) -> Result<Value, String> {
        let label = MoveLabel::from_value(&args[0]).ok_or_else(|| {
            format!(
                "offer() label must be a str or an int, got {}",
                args[0].type_name()
            )
        })?;
        let Some(session) = self.explore_session.as_ref() else {
            return Err(
                "offer() only means something in a move system that explore() or follow() is running; it was called outside one (or from an event handler)"
                    .to_string(),
            );
        };
        // A `mut` parameter is a private copy written back when the system
        // returns. That is after the rewind, so the offered world would miss
        // the write and the rewound world would get it.
        if self
            .frames
            .iter()
            .filter_map(|frame| frame.system_writeback.as_ref())
            .any(|writeback| {
                !writeback.mutable_params.is_empty() || !writeback.mutable_resources.is_empty()
            })
        {
            return Err(
                "offer() cannot be used in a system with `mut` parameters: their writes land when the system returns, after the world was offered. Write with set()/set_resource() instead"
                    .to_string(),
            );
        }
        let base = std::sync::Arc::clone(&session.base);
        let offered = self.snapshot_with_events();
        if let Some(session) = self.explore_session.as_mut() {
            session.offers.push((label, offered));
        }
        self.explore_rewind(&base);
        Ok(Value::NIL)
    }

    /// `follow(fork, [system::Move, ...], path) -> world_fork`: run the moves
    /// a path names, one after another. Nothing from the search that found the
    /// path is reused, so a path that follows is a checked certificate.
    fn bi_follow(&mut self, args: Vec<Value>) -> Result<Value, String> {
        self.explore_guard("follow")?;
        let start = std::sync::Arc::clone(
            args[0]
                .as_world_fork()
                .ok_or_else(|| "follow() first argument must be a world_fork".to_string())?,
        );
        let systems = self.explore_schedule(&args[1], "follow")?;
        let path = args[2]
            .as_list()
            .ok_or_else(|| "follow() third argument must be a list of move labels".to_string())?
            .iter()
            .map(|label| {
                MoveLabel::from_value(label).ok_or_else(|| {
                    format!(
                        "follow() path labels are str or int, got {}",
                        label.type_name()
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let shared = self.shared_state();
        let end = explore_on_pool(|| {
            super::exec::with_worker_vm(&shared, |worker| {
                worker.explore_as_worker(|worker| {
                    let mut at = start;
                    for (step, wanted) in path.iter().enumerate() {
                        let moves = worker.explore_moves(&at, &systems, "follow")?;
                        let offered = moves.len();
                        let Some((_, next)) =
                            moves.into_iter().find(|(label, _)| label == wanted)
                        else {
                            return Err(format!(
                                "follow(): step {} of the path is {wanted}, but none of the {offered} moves from that world has this label",
                                step + 1
                            ));
                        };
                        at = std::sync::Arc::new(next);
                    }
                    Ok(at)
                })
            })
        })?;
        Ok(Value::world_fork(&mut self.gc, end))
    }
}

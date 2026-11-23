// Inferring the effect class of unannotated functions.
//
// A body is classified by the conservative walkers (`block_is_conservatively_pure`,
// `block_is_conservatively_readonly`), which consult the signatures of the functions it
// calls. Registration classifies each fn once, in declaration order; `refine_fn_effects`
// then settles the whole table as a greatest fixpoint, so neither declaration order nor
// recursion decides the answer (GH 0018).

/// The effect class inferred for a function body, strongest guarantee first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum InferredEffect {
    Pure,
    Readonly,
    Unrestricted,
}

impl InferredEffect {
    fn of_signature(is_pure: bool, effects: &EffectSet) -> Self {
        if is_pure || effects.is_pure() {
            Self::Pure
        } else if *effects == EffectSet::single(Effect::ReadECS) {
            Self::Readonly
        } else {
            Self::Unrestricted
        }
    }

    fn effect_set(self) -> EffectSet {
        match self {
            Self::Pure => EffectSet::pure(),
            Self::Readonly => EffectSet::single(Effect::ReadECS),
            Self::Unrestricted => EffectSet::unrestricted(),
        }
    }
}

/// An unannotated fn whose class the fixpoint may still narrow.
struct InferenceCandidate<'p> {
    name: String,
    body: &'p Block,
    registered_pure: bool,
    registered_effects: EffectSet,
}

impl Checker {
    /// The strongest class the conservative walkers vouch for, under the current table.
    fn infer_body_effect(&self, body: &Block) -> InferredEffect {
        [InferredEffect::Pure, InferredEffect::Readonly]
            .into_iter()
            .find(|&class| self.body_meets(body, class))
            .unwrap_or(InferredEffect::Unrestricted)
    }

    fn body_meets(&self, body: &Block, class: InferredEffect) -> bool {
        match class {
            InferredEffect::Pure => self.block_is_conservatively_pure(body),
            InferredEffect::Readonly => self.block_is_conservatively_readonly(body),
            InferredEffect::Unrestricted => true,
        }
    }

    /// Every identifier a body mentions. A superset of the functions whose signatures
    /// the walkers read for it, which is all the fixpoint needs for its caller edges.
    fn collect_named_refs(block: &Block, out: &mut std::collections::HashSet<String>) {
        use crate::visitor::AstVisitor;
        struct NameCollector<'a> {
            out: &'a mut std::collections::HashSet<String>,
        }
        impl AstVisitor for NameCollector<'_> {
            fn visit_expr(&mut self, expr: &Expr) {
                if let Expr::Ident(name, _) = expr {
                    self.out.insert(name.clone());
                }
                crate::visitor::walk_expr(self, expr);
            }
        }
        NameCollector { out }.visit_block(block);
    }

    fn set_inferred_signature(&mut self, name: &str, is_pure: bool, effects: EffectSet) {
        if let Some(sig) = self.functions.get_mut(name) {
            sig.is_pure = is_pure;
            sig.effects = effects;
        }
    }

    /// Settle the effect class of every unannotated fn with the complete function table.
    ///
    /// Greatest fixpoint, one class at a time (pure, then readonly): every candidate
    /// registered weaker than the class is assumed to meet it, and a candidate whose body
    /// does not meet it under the current assumptions is put back to its registered
    /// signature. Only its callers can change because of that, so they alone are
    /// rechecked: the work is proportional to the call edges, not to passes times fns.
    /// Starting from "unrestricted" instead could never prove a recursive fn pure,
    /// because its own call would still read as unrestricted.
    pub(super) fn refine_fn_effects(&mut self, program: &Program) {
        let candidates = self.effect_inference_candidates(program);
        let index: HashMap<&str, usize> = candidates
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.as_str(), i))
            .collect();
        let mut callers: Vec<Vec<usize>> = vec![Vec::new(); candidates.len()];
        for (caller, c) in candidates.iter().enumerate() {
            let mut refs = std::collections::HashSet::new();
            Self::collect_named_refs(c.body, &mut refs);
            for callee in refs.iter().filter_map(|r| index.get(r.as_str())) {
                callers[*callee].push(caller);
            }
        }

        let mut settled = vec![false; candidates.len()];
        for class in [InferredEffect::Pure, InferredEffect::Readonly] {
            let assumed = self.assume_and_demote(&candidates, &callers, &settled, class);
            for (i, c) in candidates.iter().enumerate() {
                if assumed[i] {
                    settled[i] = true;
                    if class == InferredEffect::Pure {
                        self.purity_breach_reasons.remove(&c.name);
                    }
                }
            }
        }
    }

    fn effect_inference_candidates<'p>(&self, program: &'p Program) -> Vec<InferenceCandidate<'p>> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for decl in &program.declarations {
            let Decl::Fn(f) = decl else { continue };
            // explicit annotations are contracts, not inferences
            if f.is_pure || !f.effects.is_empty() || f.is_async {
                continue;
            }
            let Some(sig) = self.functions.get(&f.name) else {
                continue;
            };
            if InferredEffect::of_signature(sig.is_pure, &sig.effects) == InferredEffect::Pure
                || !seen.insert(f.name.clone())
            {
                continue;
            }
            out.push(InferenceCandidate {
                name: f.name.clone(),
                body: &f.body,
                registered_pure: sig.is_pure,
                registered_effects: sig.effects.clone(),
            });
        }
        out
    }

    /// Assume `class` for every unsettled candidate registered weaker than it, then demote
    /// along caller edges until stable. Returns which candidates keep the assumption.
    fn assume_and_demote(
        &mut self,
        candidates: &[InferenceCandidate<'_>],
        callers: &[Vec<usize>],
        settled: &[bool],
        class: InferredEffect,
    ) -> Vec<bool> {
        let mut assumed: Vec<bool> = candidates
            .iter()
            .enumerate()
            .map(|(i, c)| {
                !settled[i]
                    && InferredEffect::of_signature(c.registered_pure, &c.registered_effects)
                        > class
            })
            .collect();
        for (i, c) in candidates.iter().enumerate() {
            if assumed[i] {
                self.set_inferred_signature(&c.name, class == InferredEffect::Pure, class.effect_set());
            }
        }
        let mut work: Vec<usize> = (0..candidates.len()).filter(|&i| assumed[i]).collect();
        while let Some(i) = work.pop() {
            if !assumed[i] || self.body_meets(candidates[i].body, class) {
                continue;
            }
            assumed[i] = false;
            let c = &candidates[i];
            self.set_inferred_signature(&c.name, c.registered_pure, c.registered_effects.clone());
            work.extend(callers[i].iter().copied().filter(|&j| assumed[j]));
        }
        assumed
    }
}

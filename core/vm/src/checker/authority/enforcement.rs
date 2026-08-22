use super::graph::{effect_path, effect_path_matching, unknown_effect_path};
use super::*;

impl Checker {
    pub(super) fn enforce_system_authority(&mut self, node: &NodeDraft, report: &AuthorityReport) {
        let Some(inferred) = report.callables.get(&node.seed.name) else {
            return;
        };
        self.enforce_cost_contracts(node, report, inferred);
        let mut allowed_reads = node
            .seed
            .system_params
            .values()
            .map(|(name, _)| name.as_str())
            .collect::<HashSet<_>>();
        allowed_reads.extend(node.seed.authority_reads.iter().map(String::as_str));
        let mut allowed_writes = node
            .seed
            .system_params
            .values()
            .filter(|(_, mutable)| *mutable)
            .map(|(name, _)| name.as_str())
            .collect::<HashSet<_>>();
        allowed_writes.extend(node.seed.authority_writes.iter().map(String::as_str));
        let allowed_emits = node
            .seed
            .authority_emits
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let reads_all = allowed_reads.contains(WHOLE_WORLD);
        let writes_all = allowed_writes.contains(WHOLE_WORLD);
        let emits_all = allowed_emits.contains(WHOLE_WORLD);
        let denied_reads = inferred
            .transitive
            .reads
            .iter()
            .filter(|name| !reads_all && !allowed_reads.contains(name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let denied_writes = inferred
            .transitive
            .writes
            .iter()
            .filter(|name| {
                !writes_all
                    && !allowed_writes.contains(name.as_str())
                    && !node.parameter_assignments.contains(name.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        let denied_emits = inferred
            .transitive
            .emits
            .iter()
            .filter(|name| !emits_all && !allowed_emits.contains(name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let denied_io = inferred.transitive.io && !node.seed.authority_io;
        let denied_async = inferred.transitive.async_effect && !node.seed.authority_async;
        if denied_reads.is_empty()
            && denied_writes.is_empty()
            && denied_emits.is_empty()
            && !denied_io
            && !denied_async
            && !inferred.transitive.unknown
        {
            return;
        }
        let mut violations = Vec::new();
        if !denied_reads.is_empty() {
            violations.push(format!("reads [{}]", denied_reads.join(", ")));
        }
        if !denied_writes.is_empty() {
            violations.push(format!("writes [{}]", denied_writes.join(", ")));
        }
        if !denied_emits.is_empty() {
            violations.push(format!("emits [{}]", denied_emits.join(", ")));
        }
        if denied_io {
            violations.push("performs IO".to_string());
        }
        if denied_async {
            violations.push("reaches async execution".to_string());
        }
        if inferred.transitive.unknown {
            violations.push("invokes an unbounded function value".to_string());
        }
        let evidence = denied_writes
            .first()
            .map(|name| (name.as_str(), true))
            .or_else(|| denied_reads.first().map(|name| (name.as_str(), false)))
            .and_then(|(name, write)| effect_path(report, &node.seed.name, name, write))
            .or_else(|| {
                denied_emits.first().and_then(|name| {
                    effect_path_matching(report, &node.seed.name, |effects| {
                        effects.emits.iter().any(|event| event == name)
                    })
                })
            })
            .or_else(|| {
                denied_io
                    .then(|| effect_path_matching(report, &node.seed.name, |effects| effects.io))?
            })
            .or_else(|| {
                denied_async.then(|| {
                    effect_path_matching(report, &node.seed.name, |effects| effects.async_effect)
                })?
            })
            .or_else(|| {
                inferred
                    .transitive
                    .unknown
                    .then(|| unknown_effect_path(report, &node.seed.name))
                    .flatten()
            })
            .expect("every transitive authority violation must have a direct call path");
        let mut grants = Vec::new();
        grants.extend(denied_reads.iter().map(|name| format!("reads {name}")));
        grants.extend(denied_writes.iter().map(|name| format!("writes {name}")));
        grants.extend(denied_emits.iter().map(|name| {
            if name.starts_with('$') {
                format!("emits \"{name}\"")
            } else {
                format!("emits {name}")
            }
        }));
        if denied_io {
            grants.push("io true".to_string());
        }
        if denied_async {
            grants.push("async true".to_string());
        }
        self.authority_error(
            &node.seed.span,
            format!(
                "System '{}' exceeds its declared authority: {}",
                node.seed.display_name,
                violations.join("; ")
            ),
            Some(format!(
                "authority path: {}. Add `{}` to the system signature, or move the effect behind a separately scheduled authority boundary",
                evidence.join(" -> "),
                grants.join("`, `")
            )),
        );
    }

    fn enforce_cost_contracts(
        &mut self,
        node: &NodeDraft,
        report: &AuthorityReport,
        inferred: &CallableAuthority,
    ) {
        let contracts = &node.seed.contracts;
        let scan_restricted =
            contracts.frame || contracts.tick || contracts.render || contracts.no_full_scan;
        if scan_restricted
            && contracts.allow_full_scan_reason.is_none()
            && inferred.transitive.full_scan
        {
            let path = effect_path_matching(report, &node.seed.name, |effects| effects.full_scan)
                .expect("transitive full scan has a direct evidence path");
            self.authority_error(
                &node.seed.span,
                format!(
                    "System '{}' violates its no-full-scan contract",
                    node.seed.display_name
                ),
                Some(format!(
                    "query-cost path: {}. Use an indexed lookup or materialized view; exceptional scans require @allow_full_scan(reason: \"...\")",
                    path.join(" -> ")
                )),
            );
        }
        if contracts.no_guest_allocation && inferred.transitive.allocates {
            let path = effect_path_matching(report, &node.seed.name, |effects| effects.allocates)
                .expect("transitive allocation has a direct evidence path");
            self.authority_error(
                &node.seed.span,
                format!(
                    "System '{}' violates its no-guest-allocation contract",
                    node.seed.display_name
                ),
                Some(format!("allocation path: {}", path.join(" -> "))),
            );
        }
    }

    pub(super) fn enforce_transaction_authority(
        &mut self,
        node: &NodeDraft,
        report: &AuthorityReport,
    ) {
        let Some(inferred) = report.callables.get(&node.seed.name) else {
            return;
        };
        let allowed = node
            .seed
            .transaction_changes_only
            .as_ref()
            .expect("transaction authority roots carry changes_only");
        let writes_all = allowed.contains(WHOLE_WORLD);
        let denied_writes = inferred
            .transitive
            .writes
            .iter()
            .filter(|name| !writes_all && !allowed.contains(name.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if denied_writes.is_empty()
            && inferred.transitive.emits.is_empty()
            && !inferred.transitive.io
            && !inferred.transitive.async_effect
            && !inferred.transitive.unknown
        {
            return;
        }

        let mut violations = Vec::new();
        if !denied_writes.is_empty() {
            violations.push(format!(
                "writes outside changes_only [{}]",
                denied_writes.join(", ")
            ));
        }
        if !inferred.transitive.emits.is_empty() {
            violations.push(format!(
                "emits before commit [{}]",
                inferred.transitive.emits.join(", ")
            ));
        }
        if inferred.transitive.io {
            violations.push("performs IO before commit".to_string());
        }
        if inferred.transitive.async_effect {
            violations.push("reaches async execution before commit".to_string());
        }
        if inferred.transitive.unknown {
            violations.push("invokes an unbounded function value".to_string());
        }

        let evidence = denied_writes
            .first()
            .and_then(|name| effect_path(report, &node.seed.name, name, true))
            .or_else(|| {
                (!inferred.transitive.emits.is_empty()).then(|| {
                    effect_path_matching(report, &node.seed.name, |effects| {
                        !effects.emits.is_empty()
                    })
                })?
            })
            .or_else(|| {
                inferred
                    .transitive
                    .io
                    .then(|| effect_path_matching(report, &node.seed.name, |effects| effects.io))?
            })
            .or_else(|| {
                inferred.transitive.async_effect.then(|| {
                    effect_path_matching(report, &node.seed.name, |effects| effects.async_effect)
                })?
            })
            .or_else(|| {
                inferred
                    .transitive
                    .unknown
                    .then(|| unknown_effect_path(report, &node.seed.name))
                    .flatten()
            })
            .expect("every transaction authority violation has a direct call path");
        self.authority_error(
            &node.seed.span,
            format!(
                "{} violates its atomic effect boundary: {}",
                node.seed.display_name,
                violations.join("; ")
            ),
            Some(format!(
                "authority path: {}. Add state authority to `changes_only`, or move external effects into `post_commit`",
                evidence.join(" -> ")
            )),
        );
    }

    pub(super) fn enforce_post_commit_authority(
        &mut self,
        node: &NodeDraft,
        report: &AuthorityReport,
    ) {
        let Some(inferred) = report.callables.get(&node.seed.name) else {
            return;
        };
        let transition = inferred
            .transitive
            .emits
            .iter()
            .any(|event| event == STATE_TRANSITION);
        if inferred.transitive.writes.is_empty() && !transition && !inferred.transitive.unknown {
            return;
        }
        let evidence = inferred
            .transitive
            .writes
            .first()
            .and_then(|name| effect_path(report, &node.seed.name, name, true))
            .or_else(|| {
                transition.then(|| {
                    effect_path_matching(report, &node.seed.name, |effects| {
                        effects.emits.iter().any(|event| event == STATE_TRANSITION)
                    })
                })?
            })
            .or_else(|| {
                inferred
                    .transitive
                    .unknown
                    .then(|| unknown_effect_path(report, &node.seed.name))
                    .flatten()
            })
            .expect("every post_commit authority violation has a direct path");
        let mut violations = Vec::new();
        if !inferred.transitive.writes.is_empty() {
            violations.push(format!(
                "writes authoritative state [{}]",
                inferred.transitive.writes.join(", ")
            ));
        }
        if transition {
            violations.push("performs a state transition".to_string());
        }
        if inferred.transitive.unknown {
            violations.push("invokes an unbounded function value".to_string());
        }
        self.authority_error(
            &node.seed.span,
            format!(
                "{} violates its external-effect boundary: {}",
                node.seed.display_name,
                violations.join("; ")
            ),
            Some(format!(
                "authority path: {}. Keep authoritative state changes inside the transaction body",
                evidence.join(" -> ")
            )),
        );
    }
}

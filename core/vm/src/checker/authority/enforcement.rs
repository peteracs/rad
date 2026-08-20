use super::graph::{effect_path, effect_path_matching, unknown_effect_path};
use super::*;

impl Checker {
    pub(super) fn enforce_system_authority(&mut self, node: &NodeDraft, report: &AuthorityReport) {
        let Some(inferred) = report.callables.get(&node.seed.name) else {
            return;
        };
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
}

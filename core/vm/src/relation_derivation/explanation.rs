use super::{DerivedRelationState, ProofAlternative, SupportRef};
use crate::causality::CausalityLedger;
use crate::relation_runtime::{AuthoritativeRelationState, FactKey};
use std::fmt::Write as _;

const MAX_EXPLANATION_BYTES: usize = 64 * 1024;
const MAX_SHOWN_ALTERNATIVES: usize = 8;
const TRUNCATION_SUFFIX: &str = "\n… (explanation byte limit reached)";
const CONTENT_BYTES: usize = MAX_EXPLANATION_BYTES - TRUNCATION_SUFFIX.len();

/// Render one exact fact proof without losing the authoritative assertion
/// lifetime at the bottom of the tree. Sandboxed callers are rejected before
/// reaching this function; capability filtering is therefore not delegated
/// to a display layer that could accidentally reveal hidden branches.
pub(crate) fn explain_fact(
    fact: &FactKey,
    authoritative: &AuthoritativeRelationState,
    derived: &DerivedRelationState,
    ledger: &CausalityLedger,
) -> String {
    let mut renderer = ExplanationRenderer {
        authoritative,
        derived,
        ledger,
        output: String::new(),
        truncated: false,
    };
    renderer.fact(fact, 0, None);
    if renderer.truncated {
        renderer.output.push_str(TRUNCATION_SUFFIX);
    }
    renderer.output
}

struct ExplanationRenderer<'a> {
    authoritative: &'a AuthoritativeRelationState,
    derived: &'a DerivedRelationState,
    ledger: &'a CausalityLedger,
    output: String,
    truncated: bool,
}

impl ExplanationRenderer<'_> {
    fn push(&mut self, value: &str) {
        let mut writer = CappedWriter {
            output: &mut self.output,
            truncated: &mut self.truncated,
        };
        let _ = writer.write_str(value);
    }

    fn line(&mut self, depth: usize, value: impl std::fmt::Display) {
        if !self.output.is_empty() {
            self.push("\n");
        }
        for _ in 0..depth {
            self.push("  ");
        }
        let mut writer = CappedWriter {
            output: &mut self.output,
            truncated: &mut self.truncated,
        };
        let _ = write!(writer, "{value}");
    }

    fn fact(&mut self, fact: &FactKey, depth: usize, proof_id: Option<&str>) {
        if let Some(assertion) = self.authoritative.assertions().get(fact) {
            self.line(
                depth,
                format_args!(
                    "{} {:?} [authoritative assertion #{}]",
                    fact.relation, fact.tuple, assertion.assertion_id
                ),
            );
            let ancestry = self
                .ledger
                .explain_relation_assertion(fact, assertion.assertion_id);
            for line in ancestry.lines() {
                self.line(depth + 1, line);
            }
            return;
        }

        let Some(proofs) = self.derived.proofs(fact) else {
            self.line(
                depth,
                format_args!("{} {:?}: fact absent", fact.relation, fact.tuple),
            );
            return;
        };
        self.line(
            depth,
            format_args!("{} {:?} [derived]", fact.relation, fact.tuple),
        );
        if let Some(proof_id) = proof_id {
            let proof = proofs.iter().find(|proof| proof.identity() == proof_id);
            match proof {
                Some(proof) => self.proof(proof, depth + 1),
                None => self.line(depth + 1, format_args!("proof {proof_id}: unavailable")),
            }
            return;
        }

        for (index, proof) in proofs.iter().take(MAX_SHOWN_ALTERNATIVES).enumerate() {
            self.line(depth + 1, format_args!("alternative {}:", index + 1));
            self.proof(proof, depth + 2);
        }
        if proofs.len() > MAX_SHOWN_ALTERNATIVES {
            self.line(
                depth + 1,
                format_args!(
                    "{} additional proof alternatives omitted",
                    proofs.len() - MAX_SHOWN_ALTERNATIVES
                ),
            );
        }
    }

    fn proof(&mut self, proof: &ProofAlternative, depth: usize) {
        self.line(
            depth,
            format_args!("← rule `{}` (proof {})", proof.rule, proof.identity()),
        );
        if let Some(group) = &proof.aggregate_group {
            self.line(depth + 1, format_args!("aggregate group: {group:?}"));
        }
        if !proof.required_capabilities.is_empty() {
            self.line(
                depth + 1,
                format_args!("required capabilities: {:?}", proof.required_capabilities),
            );
        }
        for support in &proof.supports {
            match support {
                SupportRef::Authoritative {
                    key, assertion_id, ..
                } => {
                    self.line(depth + 1, "← authoritative support");
                    if self
                        .authoritative
                        .assertions()
                        .get(key)
                        .is_some_and(|assertion| assertion.assertion_id == *assertion_id)
                    {
                        self.fact(key, depth + 2, None);
                    } else {
                        self.line(
                            depth + 2,
                            format_args!(
                                "{} {:?} assertion #{} unavailable",
                                key.relation, key.tuple, assertion_id
                            ),
                        );
                    }
                }
                SupportRef::Derived { key, proof_id, .. } => {
                    self.line(depth + 1, "← derived support");
                    self.fact(key, depth + 2, Some(proof_id));
                }
            }
        }
    }
}

struct CappedWriter<'a> {
    output: &'a mut String,
    truncated: &'a mut bool,
}

impl std::fmt::Write for CappedWriter<'_> {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        if *self.truncated {
            return Ok(());
        }
        let remaining = CONTENT_BYTES.saturating_sub(self.output.len());
        if value.len() <= remaining {
            self.output.push_str(value);
            return Ok(());
        }
        let mut end = remaining;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        self.output.push_str(&value[..end]);
        *self.truncated = true;
        Ok(())
    }
}

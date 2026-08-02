use std::collections::BTreeMap;

use super::layout_analysis::{LayoutAnalysis, StorageLayout};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MaterializationPlan {
    pub aos_boundaries: BTreeMap<String, Vec<String>>,
}

impl MaterializationPlan {
    #[allow(dead_code)]
    pub(crate) fn from_layout_analysis(layout_analysis: &LayoutAnalysis) -> Self {
        let mut plan = MaterializationPlan::default();
        for (type_name, decision) in &layout_analysis.layouts {
            if matches!(decision.storage, StorageLayout::SoA) && !decision.aos_sites.is_empty() {
                plan.aos_boundaries
                    .insert(type_name.clone(), decision.aos_sites.clone());
            }
        }
        plan
    }

    #[allow(dead_code)]
    pub(crate) fn needs_materialization(&self, type_name: &str) -> bool {
        self.aos_boundaries.contains_key(type_name)
    }

    #[allow(dead_code)]
    pub(crate) fn boundary_reasons(&self, type_name: &str) -> Option<&[String]> {
        self.aos_boundaries
            .get(type_name)
            .map(|reasons| reasons.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::layout_analysis::{LayoutAnalysis, LayoutDecision, StorageLayout};
    use std::collections::BTreeMap;

    #[test]
    fn captures_soa_types_with_aos_boundaries() {
        let mut layouts = BTreeMap::new();
        layouts.insert(
            "Position".to_string(),
            LayoutDecision {
                storage: StorageLayout::SoA,
                soa_sites: vec!["system `move`".to_string()],
                aos_sites: vec!["fn `write_position`".to_string()],
            },
        );
        layouts.insert(
            "Velocity".to_string(),
            LayoutDecision {
                storage: StorageLayout::AoS,
                soa_sites: vec![],
                aos_sites: vec!["fn `serialize`".to_string()],
            },
        );
        let analysis = LayoutAnalysis { layouts };
        let plan = MaterializationPlan::from_layout_analysis(&analysis);
        assert!(plan.needs_materialization("Position"));
        assert!(!plan.needs_materialization("Velocity"));
        assert!(plan
            .boundary_reasons("Position")
            .unwrap()
            .iter()
            .any(|site| site.contains("write_position")));
    }
}

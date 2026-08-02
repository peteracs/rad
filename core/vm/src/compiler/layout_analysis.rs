use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use crate::checker::FunctionSig;
use crate::types::{CheckerOutput, Effect, Ty};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StorageLayout {
    AoS,
    SoA,
}

impl fmt::Display for StorageLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageLayout::AoS => write!(f, "AoS"),
            StorageLayout::SoA => write!(f, "SoA"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct LayoutVotes {
    soa_sites: Vec<String>,
    aos_sites: Vec<String>,
}

impl LayoutVotes {
    fn push_soa(&mut self, site: String) {
        push_unique(&mut self.soa_sites, site);
    }

    fn push_aos(&mut self, site: String) {
        push_unique(&mut self.aos_sites, site);
    }

    fn is_empty(&self) -> bool {
        self.soa_sites.is_empty() && self.aos_sites.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LayoutDecision {
    pub storage: StorageLayout,
    pub soa_sites: Vec<String>,
    pub aos_sites: Vec<String>,
}

impl LayoutDecision {
    #[allow(dead_code)]
    pub(crate) fn requires_materialization(&self) -> bool {
        matches!(self.storage, StorageLayout::SoA) && !self.aos_sites.is_empty()
    }
}

impl Default for LayoutDecision {
    fn default() -> Self {
        Self {
            storage: StorageLayout::SoA,
            soa_sites: Vec::new(),
            aos_sites: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LayoutAnalysis {
    pub layouts: BTreeMap<String, LayoutDecision>,
}

impl LayoutAnalysis {
    pub(crate) fn analyze<F>(checker_output: &CheckerOutput, mut canonicalize: F) -> Self
    where
        F: FnMut(&str) -> String,
    {
        let mut known_types = HashSet::new();
        for name in checker_output
            .components
            .keys()
            .chain(checker_output.resources.keys())
            .chain(checker_output.structs.keys())
        {
            known_types.insert(canonicalize(name));
        }

        let mut votes: BTreeMap<String, LayoutVotes> = BTreeMap::new();
        for name in &known_types {
            votes.entry(name.clone()).or_default();
        }

        let containment = build_containment_graph(checker_output, &known_types, &mut canonicalize);

        for (name, sig) in &checker_output.functions {
            if sig.is_pure || sig.effects.is_pure() {
                continue;
            }

            let referenced = collect_ty_refs_from_sig(sig, &known_types, &mut canonicalize);
            if referenced.is_empty() {
                continue;
            }

            let site = format!("fn `{}`", name);
            if sig.effects.allows(Effect::IO) {
                for ty_name in &referenced {
                    votes
                        .entry(ty_name.clone())
                        .or_default()
                        .push_aos(site.clone());
                }
            }
            if sig.effects.allows(Effect::ECS) || sig.effects.allows(Effect::ReadECS) {
                for ty_name in &referenced {
                    votes
                        .entry(ty_name.clone())
                        .or_default()
                        .push_soa(site.clone());
                }
            }
        }

        for (name, sys) in &checker_output.systems {
            let site = format!("system `{}`", name);
            for param in &sys.params {
                if param.is_resource {
                    continue;
                }
                let resolved = canonicalize(&param.component_type);
                if known_types.contains(&resolved) {
                    votes
                        .entry(resolved)
                        .or_default()
                        .push_soa(format!("{} param `{}`", site, param.name));
                }
            }
        }

        let mut changed = true;
        while changed {
            changed = false;
            for (child, parents) in &containment {
                let Some(child_votes) = votes.get(child).cloned() else {
                    continue;
                };
                if child_votes.is_empty() {
                    continue;
                }

                for (parent, field_name) in parents {
                    let parent_votes = votes.entry(parent.clone()).or_default();
                    let before = parent_votes.clone();
                    if !child_votes.soa_sites.is_empty() {
                        for reason in &child_votes.soa_sites {
                            parent_votes.push_soa(format!(
                                "contains field `{}` via {}",
                                field_name, reason
                            ));
                        }
                    }
                    if !child_votes.aos_sites.is_empty() {
                        for reason in &child_votes.aos_sites {
                            parent_votes.push_aos(format!(
                                "contains field `{}` via {}",
                                field_name, reason
                            ));
                        }
                    }
                    if *parent_votes != before {
                        changed = true;
                    }
                }
            }
        }

        let mut layouts = BTreeMap::new();
        for name in known_types {
            let votes = votes.get(&name).cloned().unwrap_or_default();
            layouts.insert(name, finalize_decision(votes));
        }

        Self { layouts }
    }

    #[allow(dead_code)]
    pub(crate) fn decision(&self, name: &str) -> Option<&LayoutDecision> {
        self.layouts.get(name)
    }

    #[allow(dead_code)]
    pub(crate) fn materialized_types(&self) -> Vec<&str> {
        self.layouts
            .iter()
            .filter_map(|(name, decision)| {
                if decision.requires_materialization() {
                    Some(name.as_str())
                } else {
                    None
                }
            })
            .collect()
    }
}

fn finalize_decision(votes: LayoutVotes) -> LayoutDecision {
    let storage = if votes.soa_sites.is_empty() && !votes.aos_sites.is_empty() {
        StorageLayout::AoS
    } else {
        StorageLayout::SoA
    };
    LayoutDecision {
        storage,
        soa_sites: votes.soa_sites,
        aos_sites: votes.aos_sites,
    }
}

fn build_containment_graph<F>(
    checker_output: &CheckerOutput,
    known_types: &HashSet<String>,
    canonicalize: &mut F,
) -> HashMap<String, Vec<(String, String)>>
where
    F: FnMut(&str) -> String,
{
    let mut graph: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for (parent_name, fields) in checker_output
        .components
        .iter()
        .map(|(name, decl)| (name, &decl.fields))
        .chain(
            checker_output
                .resources
                .iter()
                .map(|(name, decl)| (name, &decl.fields)),
        )
        .chain(
            checker_output
                .structs
                .iter()
                .map(|(name, decl)| (name, &decl.fields)),
        )
    {
        let parent = canonicalize(parent_name);
        for (field_name, ty) in fields {
            for child in collect_ty_refs(ty, known_types, canonicalize) {
                graph
                    .entry(child)
                    .or_default()
                    .push((parent.clone(), field_name.clone()));
            }
        }
    }
    graph
}

fn collect_ty_refs_from_sig<F>(
    sig: &FunctionSig,
    known_types: &HashSet<String>,
    canonicalize: &mut F,
) -> HashSet<String>
where
    F: FnMut(&str) -> String,
{
    let mut refs = HashSet::new();
    for ty in &sig.params {
        refs.extend(collect_ty_refs(ty, known_types, canonicalize));
    }
    refs.extend(collect_ty_refs(&sig.ret, known_types, canonicalize));
    refs
}

fn collect_ty_refs<F>(
    ty: &Ty,
    known_types: &HashSet<String>,
    canonicalize: &mut F,
) -> HashSet<String>
where
    F: FnMut(&str) -> String,
{
    let mut refs = HashSet::new();
    match ty {
        Ty::Component(name) | Ty::Struct(name) | Ty::SumType(name) | Ty::Event(name) => {
            let resolved = canonicalize(name);
            if known_types.contains(&resolved) {
                refs.insert(resolved);
            }
        }
        Ty::App(name, args) => {
            let resolved = canonicalize(name);
            if known_types.contains(&resolved) {
                refs.insert(resolved);
            }
            for arg in args {
                refs.extend(collect_ty_refs(arg, known_types, canonicalize));
            }
        }
        Ty::List(inner) | Ty::Task(inner) => {
            refs.extend(collect_ty_refs(inner, known_types, canonicalize));
        }
        Ty::Tuple(items) | Ty::Union(items) => {
            for item in items {
                refs.extend(collect_ty_refs(item, known_types, canonicalize));
            }
        }
        Ty::Map(key, value) => {
            refs.extend(collect_ty_refs(key, known_types, canonicalize));
            refs.extend(collect_ty_refs(value, known_types, canonicalize));
        }
        Ty::Fn { params, ret, .. } => {
            for param in params {
                refs.extend(collect_ty_refs(param, known_types, canonicalize));
            }
            refs.extend(collect_ty_refs(ret, known_types, canonicalize));
        }
        _ => {}
    }
    refs
}

fn push_unique(vec: &mut Vec<String>, value: String) {
    if !vec.iter().any(|existing| existing == &value) {
        vec.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::FunctionSig;
    use crate::types::{
        CheckerOutput, ComponentType, Effect, EffectSet, ResourceType, StructType, SystemParam,
        SystemType, Ty,
    };

    fn field(name: &str, ty: Ty) -> (String, Ty) {
        (name.to_string(), ty)
    }

    fn checker_output_with(layout_fields: Vec<(String, Ty)>) -> CheckerOutput {
        let mut output = CheckerOutput::default();
        output.components.insert(
            "Position".to_string(),
            ComponentType {
                name: "Position".to_string(),
                is_pub: true,
                file_id: None,
                fields: layout_fields,
                indexed_fields: Default::default(),
            },
        );
        output.structs.insert(
            "Payload".to_string(),
            StructType {
                name: "Payload".to_string(),
                is_pub: true,
                file_id: None,
                fields: vec![field("pos", Ty::Component("Position".to_string()))],
            },
        );
        output.resources.insert(
            "WorldState".to_string(),
            ResourceType {
                name: "WorldState".to_string(),
                is_pub: true,
                file_id: None,
                fields: vec![],
            },
        );
        output.functions.insert(
            "write_position".to_string(),
            FunctionSig {
                type_params: vec![],
                params: vec![Ty::Component("Position".to_string())],
                ret: Ty::Nil,
                is_pure: false,
                effects: EffectSet::from_vec(&[Effect::IO]),
            },
        );
        output.functions.insert(
            "read_position".to_string(),
            FunctionSig {
                type_params: vec![],
                params: vec![Ty::Component("Position".to_string())],
                ret: Ty::Nil,
                is_pure: false,
                effects: EffectSet::from_vec(&[Effect::ReadECS]),
            },
        );
        output.systems.insert(
            "move".to_string(),
            SystemType {
                name: "move".to_string(),
                is_pub: true,
                file_id: None,
                params: vec![SystemParam {
                    name: "pos".to_string(),
                    component_type: "Position".to_string(),
                    is_mut: true,
                    is_resource: false,
                }],
                simulation_breach: None,
                simulation_breach_par: None,
            },
        );
        output
    }

    #[test]
    fn prefers_soa_for_ecs_and_marks_io_boundary() {
        let output = checker_output_with(vec![]);
        let analysis = LayoutAnalysis::analyze(&output, |name| name.to_string());
        let decision = analysis.decision("Position").expect("layout decision");
        assert_eq!(decision.storage, StorageLayout::SoA);
        assert!(decision.requires_materialization());
        assert!(decision
            .soa_sites
            .iter()
            .any(|site| site.contains("system `move`")));
        assert!(decision
            .aos_sites
            .iter()
            .any(|site| site.contains("fn `write_position`")));
    }

    #[test]
    fn propagates_containment_constraints_to_parents() {
        let mut output = CheckerOutput::default();
        output.components.insert(
            "Child".to_string(),
            ComponentType {
                name: "Child".to_string(),
                is_pub: true,
                file_id: None,
                fields: vec![],
                indexed_fields: Default::default(),
            },
        );
        output.structs.insert(
            "Parent".to_string(),
            StructType {
                name: "Parent".to_string(),
                is_pub: true,
                file_id: None,
                fields: vec![field("child", Ty::Struct("Child".to_string()))],
            },
        );
        output.functions.insert(
            "io_use".to_string(),
            FunctionSig {
                type_params: vec![],
                params: vec![Ty::Struct("Child".to_string())],
                ret: Ty::Nil,
                is_pure: false,
                effects: EffectSet::from_vec(&[Effect::IO]),
            },
        );

        let analysis = LayoutAnalysis::analyze(&output, |name| name.to_string());
        let child = analysis.decision("Child").expect("child decision");
        let parent = analysis.decision("Parent").expect("parent decision");

        assert_eq!(child.storage, StorageLayout::AoS);
        assert_eq!(parent.storage, StorageLayout::AoS);
        assert!(parent
            .aos_sites
            .iter()
            .any(|site| site.contains("contains field `child`")));
    }
}

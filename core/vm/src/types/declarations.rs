#[derive(Debug, Clone)]
pub struct ComponentType {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub fields: Vec<(String, Ty)>,
    pub indexed_fields: HashSet<String>,
}

impl ComponentType {
    /// A component-shaped view of another declaration's fields.
    ///
    /// Resources and structs are stored in their own registries but the
    /// compiler looks up field layout through one component table. Both
    /// conversions were written out separately at that call site, so a new
    /// field on `ComponentType` had to be remembered twice.
    pub(crate) fn from_declared_fields(
        name: String,
        fields: Vec<(String, Ty)>,
        is_pub: bool,
        file_id: Option<crate::ast::FileId>,
    ) -> Self {
        Self {
            name,
            is_pub,
            file_id,
            fields,
            // Only components declare `indexed` fields; a resource or struct
            // borrowed into this shape has none.
            indexed_fields: HashSet::new(),
        }
    }

    pub fn field_type(&self, name: &str) -> Option<&Ty> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }
}

#[derive(Debug, Clone)]
pub struct ResourceType {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub fields: Vec<(String, Ty)>,
}

impl ResourceType {
    pub fn field_type(&self, name: &str) -> Option<&Ty> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }
}

#[derive(Debug, Clone)]
pub struct StructType {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub fields: Vec<(String, Ty)>,
}

impl StructType {
    pub fn field_type(&self, name: &str) -> Option<&Ty> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }
}

#[derive(Debug, Clone)]
pub struct StateMachineType {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub states: Vec<String>,
    pub transitions: HashMap<String, Vec<(String, String)>>,
}

impl StateMachineType {
    pub fn has_state(&self, name: &str) -> bool {
        self.states.iter().any(|s| s == name)
    }
}

#[derive(Debug, Clone)]
pub struct SystemType {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub params: Vec<SystemParam>,
    /// Authority-only entries bound access without changing the ECS query.
    pub authority_reads: Vec<String>,
    pub authority_writes: Vec<String>,
    pub authority_emits: Vec<String>,
    pub authority_io: bool,
    pub authority_async: bool,
    /// Why this system may not run under `simulate()` (IO, events, async…),
    /// if anything. `simulate()` is strict: even `rand_*` is banned because
    /// plain forks carry no explicit seed.
    pub simulation_breach: Option<String>,
    /// The lenient variant consulted by `simulate_par()`: identical, except
    /// `rand_*` builtins are permitted — every parallel fork is seeded
    /// explicitly (`fork_seed(seed, k)`), so guest randomness is
    /// deterministic and is precisely how opponent-model jitter is meant
    /// to be expressed.
    pub simulation_breach_par: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SystemParam {
    pub name: String,
    pub component_type: String,
    pub is_mut: bool,
    pub is_resource: bool,
}

#[derive(Debug, Clone)]
pub struct SumTypeDef {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub type_params: Vec<String>,
    pub variants: Vec<VariantType>,
}

#[derive(Debug, Clone)]
pub struct VariantType {
    pub name: String,
    pub fields: Vec<(String, Ty)>,
}

#[derive(Debug, Clone)]
pub struct EventType {
    pub name: String,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub fields: Vec<(String, Ty)>,
}

impl EventType {
    pub fn field_type(&self, name: &str) -> Option<&Ty> {
        self.fields.iter().find(|(n, _)| n == name).map(|(_, t)| t)
    }
}

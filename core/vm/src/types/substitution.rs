#[derive(Debug, Clone)]
pub struct TypeScheme {
    pub type_params: Vec<String>,
    pub is_pub: bool,
    pub file_id: Option<crate::ast::FileId>,
    pub ty: Ty,
}

#[derive(Debug, Clone)]
pub struct Substitution {
    map: HashMap<u32, Ty>,
}

impl Substitution {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn bind(&mut self, id: u32, ty: Ty) {
        self.map.insert(id, ty);
    }

    pub fn lookup(&self, id: u32) -> Option<&Ty> {
        self.map.get(&id)
    }

    pub fn resolve(&self, ty: &Ty) -> Ty {
        match ty {
            Ty::Var(id) => {
                if let Some(bound) = self.map.get(id) {
                    self.resolve(bound)
                } else {
                    ty.clone()
                }
            }
            Ty::List(inner) => Ty::List(Box::new(self.resolve(inner))),
            Ty::Map(key, val) => Ty::Map(Box::new(self.resolve(key)), Box::new(self.resolve(val))),
            Ty::Fn {
                params,
                ret,
                purity,
            } => Ty::Fn {
                params: params.iter().map(|p| self.resolve(p)).collect(),
                ret: Box::new(self.resolve(ret)),
                purity: *purity,
            },
            Ty::Task(inner) => Ty::Task(Box::new(self.resolve(inner))),
            Ty::App(name, args) => {
                Ty::App(name.clone(), args.iter().map(|a| self.resolve(a)).collect())
            }
            _ => ty.clone(),
        }
    }

    pub fn unify(&mut self, a: &Ty, b: &Ty) -> Result<(), String> {
        let a = self.resolve(a);
        let b = self.resolve(b);

        if a == b {
            return Ok(());
        }

        match (&a, &b) {
            (Ty::Any, _) | (_, Ty::Any) => Ok(()),

            (Ty::Var(id), _) => {
                if b.contains_var(*id) {
                    return Err(format!("Infinite type: ?T{} occurs in {}", id, b));
                }
                self.bind(*id, b);
                Ok(())
            }
            (_, Ty::Var(id)) => {
                if a.contains_var(*id) {
                    return Err(format!("Infinite type: ?T{} occurs in {}", id, a));
                }
                self.bind(*id, a);
                Ok(())
            }

            (Ty::Float, Ty::Int) | (Ty::Int, Ty::Float) => Ok(()),

            (Ty::List(a_inner), Ty::List(b_inner)) => self.unify(a_inner, b_inner),
            (Ty::Map(a_key, a_val), Ty::Map(b_key, b_val)) => {
                self.unify(a_key, b_key)?;
                self.unify(a_val, b_val)
            }
            (Ty::Task(a_inner), Ty::Task(b_inner)) => self.unify(a_inner, b_inner),

            (
                Ty::Fn {
                    params: ap,
                    ret: ar,
                    purity: a_purity,
                },
                Ty::Fn {
                    params: bp,
                    ret: br,
                    purity: b_purity,
                },
            ) => {
                if b_purity > a_purity {
                    return Err(format!(
                        "Cannot unify {} function with {} function",
                        a_purity.word(),
                        b_purity.word()
                    ));
                }
                if ap.len() != bp.len() {
                    return Err(format!(
                        "Function arity mismatch: {} vs {} parameters",
                        ap.len(),
                        bp.len()
                    ));
                }
                for (pa, pb) in ap.iter().zip(bp.iter()) {
                    self.unify(pa, pb)?;
                }
                self.unify(ar, br)
            }

            (Ty::SumType(a_name), Ty::SumType(b_name)) if a_name == b_name => Ok(()),
            (Ty::Component(a_name), Ty::Component(b_name)) if a_name == b_name => Ok(()),
            (Ty::Struct(a_name), Ty::Struct(b_name)) if a_name == b_name => Ok(()),
            (Ty::State(a_name), Ty::State(b_name)) if a_name == b_name => Ok(()),

            (Ty::App(a_name, a_args), Ty::App(b_name, b_args)) => {
                if a_name != b_name {
                    return Err(format!(
                        "Type constructor mismatch: {} vs {}",
                        a_name, b_name
                    ));
                }
                if a_args.len() != b_args.len() {
                    return Err(format!(
                        "Type argument count mismatch for {}: {} vs {}",
                        a_name,
                        a_args.len(),
                        b_args.len()
                    ));
                }
                for (aa, ba) in a_args.iter().zip(b_args.iter()) {
                    self.unify(aa, ba)?;
                }
                Ok(())
            }

            _ => Err(format!("Cannot unify {} with {}", a, b)),
        }
    }
}

impl Default for Substitution {
    fn default() -> Self {
        Self::new()
    }
}

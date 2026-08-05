use crate::relation_frontend::{
    FrontendArtifacts, FrontendManifestDigest, RelationKind, RelationSchema, SealedRulePlan,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use super::{RelationRuntimeError, RelationRuntimeResult};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeRelationSchema {
    schema: RelationSchema,
    digest: [u8; 32],
}

impl RuntimeRelationSchema {
    pub fn schema(&self) -> &RelationSchema {
        &self.schema
    }

    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelationRuntimeManifest {
    frontend_digest: FrontendManifestDigest,
    schemas: Arc<[RuntimeRelationSchema]>,
    rules: Arc<[Arc<SealedRulePlan>]>,
    canonical_bytes: Arc<[u8]>,
    digest: [u8; 32],
}

impl RelationRuntimeManifest {
    pub fn from_frontend(artifacts: &FrontendArtifacts) -> RelationRuntimeResult<Self> {
        if !artifacts.verify_manifest_digest() {
            return Err(RelationRuntimeError::new(
                "relation.frontend_manifest_mismatch",
                "front-end artifacts no longer match their sealed manifest digest",
            ));
        }
        let mut schemas = artifacts
            .relations
            .schemas()
            .iter()
            .cloned()
            .map(|schema| {
                let bytes = schema_bytes(&schema);
                RuntimeRelationSchema {
                    schema,
                    digest: Sha256::digest(bytes).into(),
                }
            })
            .collect::<Vec<_>>();
        schemas.sort_by(|left, right| left.schema.identity.cmp(&right.schema.identity));
        for pair in schemas.windows(2) {
            if pair[0].schema.identity == pair[1].schema.identity {
                return Err(RelationRuntimeError::new(
                    "relation.manifest_duplicate_schema",
                    pair[0].schema.identity.clone(),
                ));
            }
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"rad.relation-runtime-manifest.v1");
        out.extend_from_slice(&artifacts.manifest_digest.as_bytes());
        put_u64(&mut out, schemas.len() as u64);
        for schema in &schemas {
            put_bytes(&mut out, &schema_bytes(&schema.schema));
            out.extend_from_slice(&schema.digest);
        }
        let digest = Sha256::digest(&out).into();
        Ok(Self {
            frontend_digest: artifacts.manifest_digest,
            schemas: schemas.into(),
            rules: Arc::clone(&artifacts.rules),
            canonical_bytes: out.into(),
            digest,
        })
    }

    pub fn frontend_digest(&self) -> FrontendManifestDigest {
        self.frontend_digest
    }

    pub fn schemas(&self) -> &[RuntimeRelationSchema] {
        &self.schemas
    }

    pub fn schema(&self, identity: &str) -> Option<&RuntimeRelationSchema> {
        self.schemas
            .binary_search_by(|schema| schema.schema.identity.as_str().cmp(identity))
            .ok()
            .map(|index| &self.schemas[index])
    }

    pub fn rules(&self) -> &[Arc<SealedRulePlan>] {
        &self.rules
    }

    pub fn authoritative_schema(&self, identity: &str) -> Option<&RuntimeRelationSchema> {
        self.schema(identity)
            .filter(|schema| schema.schema.kind == RelationKind::Authoritative)
    }

    pub fn canonical_bytes(&self) -> &[u8] {
        &self.canonical_bytes
    }

    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

fn schema_bytes(schema: &RelationSchema) -> Vec<u8> {
    let mut out = Vec::new();
    put_text(&mut out, &schema.identity);
    put_text(&mut out, &schema.owner);
    out.push(match schema.kind {
        RelationKind::Authoritative => 0,
        RelationKind::Derived => 1,
    });
    out.push(u8::from(schema.symmetric));
    put_u64(&mut out, schema.columns.len() as u64);
    for column in &schema.columns {
        put_text(&mut out, &column.name);
        out.push(column.value_type.tag());
        out.push(match column.on_delete {
            None => 0,
            Some(crate::relation_frontend::OnDelete::Restrict) => 1,
            Some(crate::relation_frontend::OnDelete::Cascade) => 2,
        });
    }
    put_u64(&mut out, schema.unique.len() as u64);
    for unique in &schema.unique {
        put_text(&mut out, &unique.name);
        put_u64(&mut out, unique.columns.len() as u64);
        for column in &unique.columns {
            put_text(&mut out, column);
        }
    }
    out
}

fn put_text(out: &mut Vec<u8>, value: &str) {
    put_bytes(out, value.as_bytes());
}

fn put_bytes(out: &mut Vec<u8>, value: &[u8]) {
    put_u64(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

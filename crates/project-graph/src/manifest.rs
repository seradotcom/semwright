//! Portable manifests are untrusted declarations, never imported authority.
use crate::*;
use composition::{canonical_bytes, strict_decode};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PortableAsset {
    pub source_id: LogicalAssetId,
    pub label: String,
    pub resource_type: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PortableEdge {
    pub from: LogicalAssetId,
    pub to: LogicalAssetId,
    pub relation: Relation,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PortableManifest {
    pub version: u32,
    pub source_project: ProjectId,
    pub source_snapshot: u64,
    pub assets: Vec<PortableAsset>,
    pub declarations: Vec<PortableEdge>,
    pub coverage_complete: bool,
}
impl PortableManifest {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let m: Self = strict_decode(bytes)?;
        m.validate()?;
        Ok(m)
    }
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.version == SCHEMA_VERSION
                && self.assets.len() <= 256
                && self.declarations.len() <= 1024,
            "portable manifest version/budget",
        )?;
        let mut ids = BTreeSet::new();
        for a in &self.assets {
            name(&a.label)?;
            name(&a.resource_type)?;
            ensure(ids.insert(&a.source_id), "duplicate imported identity")?;
        }
        for e in &self.declarations {
            ensure(
                ids.contains(&e.from) && ids.contains(&e.to),
                "manifest edge leaves declared page",
            )?;
            ensure(
                matches!(
                    e.relation,
                    Relation::Contains
                        | Relation::References
                        | Relation::DerivedFrom
                        | Relation::Realizes
                        | Relation::PublishedAs
                ),
                "manifest cannot certify execution or verification",
            )?;
        }
        canonical_bytes(self)?;
        Ok(())
    }
}
impl ProjectGraph {
    pub fn export_manifest(
        &self,
        access: &ProjectAccess,
        ids: &[LogicalAssetId],
    ) -> Result<PortableManifest> {
        self.access(access, false)?;
        ensure(ids.len() <= 256, "manifest asset page limit")?;
        let selected: BTreeSet<_> = ids.iter().cloned().collect();
        ensure(selected.len() == ids.len(), "duplicate export identity")?;
        let mut assets = Vec::new();
        for id in ids {
            let a = &self.visible(access, id)?.asset;
            assets.push(PortableAsset {
                source_id: id.clone(),
                label: a.label.clone(),
                resource_type: a.resource_type.clone(),
            });
        }
        let mut declarations = Vec::new();
        for edge in self.edges.values() {
            if let (Vertex::Asset(from), Vertex::Asset(to)) = (&edge.from, &edge.to)
                && selected.contains(from)
                && selected.contains(to)
                && matches!(
                    edge.relation,
                    Relation::Contains
                        | Relation::References
                        | Relation::DerivedFrom
                        | Relation::Realizes
                        | Relation::PublishedAs
                )
            {
                ensure(declarations.len() < 1024, "manifest edge page limit")?;
                declarations.push(PortableEdge {
                    from: from.clone(),
                    to: to.clone(),
                    relation: edge.relation,
                });
            }
        }
        let manifest = PortableManifest {
            version: SCHEMA_VERSION,
            source_project: self.project.clone(),
            source_snapshot: self.sequence,
            assets,
            declarations,
            coverage_complete: false,
        };
        manifest.validate()?;
        Ok(manifest)
    }
    /// Always allocate local identities. No locator, owner, ref or acquired trust is imported.
    pub fn import_manifest(
        &mut self,
        access: &ProjectAccess,
        manifest: &PortableManifest,
    ) -> Result<BTreeMap<LogicalAssetId, LogicalAssetId>> {
        self.access(access, true)?;
        manifest.validate()?;
        let mut staged = self.clone();
        let mut remap = BTreeMap::new();
        for source in &manifest.assets {
            let id = LogicalAssetId::new();
            staged.register(
                access,
                Asset {
                    id: id.clone(),
                    resource_type: source.resource_type.clone(),
                    label: source.label.clone(),
                    locator: None,
                },
            )?;
            remap.insert(source.source_id.clone(), id);
        }
        for edge in &manifest.declarations {
            staged.declare(
                access,
                Edge {
                    from: Vertex::Asset(remap.get(&edge.from).ok_or(GraphError::Corrupt)?.clone()),
                    to: Vertex::Asset(remap.get(&edge.to).ok_or(GraphError::Corrupt)?.clone()),
                    relation: edge.relation,
                    evidence: EdgeEvidence::Declared {
                        declaration: ReceiptId::new(),
                    },
                },
            )?;
        }
        *self = staged;
        Ok(remap)
    }
}

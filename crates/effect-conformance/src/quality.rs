use crate::EffectEvaluation;
use schemars::JsonSchema;
use semwright_semantic_composition::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QualityDimension {
    Actuation, Observation, Roundtrip, Persistence, EffectBoundedScope,
    EnumerationCompleteness, Recovery, Conformance, NativeEvidence,
}
/// Orthogonal evidence routes, NOT security or certification levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRoute { Contractual, NativeAdapter, NativeBroker }
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkflowIdentity {
    pub driver: String,
    pub driver_version: String,
    pub runtime: String,
    pub os: String,
    pub workflow: String,
    pub fixture: String,
    pub source_sha: String,
    pub route: EvidenceRoute,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct DimensionEvidence {
    pub dimension: QualityDimension,
    pub verdict: Verdict,
    pub rules: BTreeSet<String>,
    pub reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct WorkflowQuality {
    pub identity: WorkflowIdentity,
    pub dimensions: Vec<DimensionEvidence>,
    pub negative_cases: BTreeSet<String>,
    pub limitations: Vec<String>,
}
/// Generate a matrix from evaluated evidence, not from descriptor counts. Missing
/// dimensions stay UNKNOWN; native-adapter evidence never implies Broker E2E.
pub fn workflow_quality(identity: WorkflowIdentity, mappings: &BTreeMap<QualityDimension, BTreeSet<String>>, evaluation: &EffectEvaluation, negative_cases: BTreeSet<String>) -> Result<WorkflowQuality> {
    for text in [&identity.driver,&identity.driver_version,&identity.runtime,&identity.os,&identity.workflow,&identity.fixture] { bounded_id(text)?; }
    ensure(identity.source_sha.len()==40 && identity.source_sha.bytes().all(|b| b.is_ascii_hexdigit()), "quality source SHA")?;
    let mut dimensions = Vec::new();
    for dimension in [QualityDimension::Actuation,QualityDimension::Observation,QualityDimension::Roundtrip,
        QualityDimension::Persistence,QualityDimension::EffectBoundedScope,QualityDimension::EnumerationCompleteness,
        QualityDimension::Recovery,QualityDimension::Conformance,QualityDimension::NativeEvidence] {
        let rules = mappings.get(&dimension).cloned().unwrap_or_default();
        let mut reason = None;
        let verdict = if rules.is_empty() {
            reason = Some("no executed evidence mapping for this workflow dimension".into()); Verdict::Unknown
        } else if dimension == QualityDimension::NativeEvidence && identity.route == EvidenceRoute::Contractual {
            reason = Some("contractual consumer is not native acceptance".into()); Verdict::Unknown
        } else if dimension == QualityDimension::Conformance && negative_cases.is_empty() {
            reason = Some("no executed negative case receipts".into()); Verdict::Unknown
        } else {
            // Delegate every aggregation to A, retaining all version/base/source rules.
            let mut selected = evaluation.report.clone();
            selected.validation.required_rules = rules.clone();
            selected.validation.checks.retain(|check| rules.contains(&check.rule));
            selected.verdict()?
        };
        dimensions.push(DimensionEvidence { dimension, verdict, rules, reason });
    }
    Ok(WorkflowQuality { identity, dimensions, negative_cases, limitations: vec![
        "Dimensions apply only to this driver/runtime/OS/workflow/fixture/SHA.".into(),
        "No official trust badge, global noninterference, crash durability or security certification.".into(),
        "Adapter-supplied mappings and negative-case references must be maintained by trusted test integration, not client metadata.".into(),
    ] })
}

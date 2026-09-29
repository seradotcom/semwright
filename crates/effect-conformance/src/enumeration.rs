use schemars::JsonSchema;
use semwright_semantic_composition::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnumerationBinding {
    pub owner: Owner,
    pub resource: ResourceKey,
    pub provider_session: String,
    pub generation: String,
    pub query_digest: Digest,
    pub snapshot_digest: Digest,
    pub universe: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EnumerationConsistency { Snapshot, BestEffort }
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnumerationPage {
    pub binding: EnumerationBinding,
    pub index: u32,
    pub cursor_in: Option<String>,
    pub cursor_out: Option<String>,
    pub items: Vec<String>,
    pub total: Option<u32>,
    pub final_page: bool,
    pub truncated: bool,
    pub consistency: EnumerationConsistency,
}
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct EnumerationAudit {
    pub verdict: Verdict,
    pub count: usize,
    pub members: BTreeSet<String>,
    pub reasons: Vec<String>,
}
/// Audits a transcript observed on a trusted channel. It does not authenticate a
/// JSON transcript or mint/validate a provider secret. Cursors remain provider-owned.
pub fn audit_enumeration(expected: &EnumerationBinding, pages: &[EnumerationPage]) -> EnumerationAudit {
    let mut audit = EnumerationAudit { verdict: Verdict::Unknown, count: 0, members: BTreeSet::new(), reasons: Vec::new() };
    let fail = |audit: &mut EnumerationAudit, reason: &str| { audit.reasons.push(reason.into()); };
    if pages.is_empty() || pages.len() > 128 {
        fail(&mut audit, "missing pages or page budget exceeded"); return audit;
    }
    let mut last_cursor: Option<String> = None;
    let mut cursors = BTreeSet::new();
    let mut last_item: Option<&str> = None;
    let total = pages[0].total;
    for (i, page) in pages.iter().enumerate() {
        if page.binding != *expected { fail(&mut audit, "query/snapshot/principal binding mismatch"); }
        if page.index as usize != i { fail(&mut audit, "missing, duplicate or reordered page"); }
        if page.cursor_in != last_cursor { fail(&mut audit, "forged or discontinuous cursor"); }
        if page.total != total { fail(&mut audit, "inconsistent page totals"); }
        if page.truncated { fail(&mut audit, "explicit truncation"); }
        if page.consistency != EnumerationConsistency::Snapshot { fail(&mut audit, "best-effort enumeration is not a consistent snapshot"); }
        if page.final_page != (i == pages.len() - 1) || page.final_page != page.cursor_out.is_none() {
            fail(&mut audit, "final-page loss or premature final page");
        }
        if !page.final_page && page.items.is_empty() { fail(&mut audit, "non-final page made no progress"); }
        if let Some(cursor) = &page.cursor_out {
            if bounded_id(cursor).is_err() || !cursors.insert(cursor.clone()) { fail(&mut audit, "invalid or repeated continuation cursor"); }
        }
        if page.items.len() > 4096 || audit.count.saturating_add(page.items.len()) > 4096 {
            fail(&mut audit, "enumeration item budget exceeded"); return audit;
        }
        for item in &page.items {
            if bounded_id(item).is_err() { fail(&mut audit, "invalid member identity"); }
            if last_item.is_some_and(|last| last >= item.as_str()) { fail(&mut audit, "unstable order or duplicate member"); }
            if !audit.members.insert(item.clone()) { fail(&mut audit, "duplicate member"); }
            last_item = Some(item);
            audit.count += 1;
        }
        last_cursor.clone_from(&page.cursor_out);
    }
    if total.is_some_and(|n| n as usize != audit.count) { fail(&mut audit, "count differs from complete snapshot total"); }
    if audit.reasons.is_empty() { audit.verdict = Verdict::Pass; }
    audit
}

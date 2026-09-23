//! Semantic changes are expressed in managed model terms, not source text.
use crate::{Result, model::Project};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PropertyChange {
    pub scope: String,
    pub id: String,
    pub property: String,
    pub before: Value,
    pub after: Value,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SemanticDiff {
    pub scenes_added: Vec<String>,
    pub scenes_removed: Vec<String>,
    pub nodes_added: Vec<String>,
    pub nodes_removed: Vec<String>,
    pub animations_changed: Vec<String>,
    pub cues_changed: Vec<String>,
    pub assets_changed: Vec<String>,
    pub properties_changed: Vec<PropertyChange>,
    pub scene_order_changed: bool,
    pub node_order_changed: Vec<String>,
    pub duration_before_ms: u64,
    pub duration_after_ms: u64,
}
impl SemanticDiff {
    pub fn is_empty(&self) -> bool {
        self.scenes_added.is_empty() && self.scenes_removed.is_empty()
            && self.nodes_added.is_empty() && self.nodes_removed.is_empty()
            && self.animations_changed.is_empty() && self.cues_changed.is_empty()
            && self.assets_changed.is_empty() && self.properties_changed.is_empty()
            && !self.scene_order_changed && self.node_order_changed.is_empty()
            && self.duration_before_ms == self.duration_after_ms
    }
}
fn map<'a, T: Serialize + 'a>(
    values: impl Iterator<Item = (&'a str, &'a T)>,
) -> Result<BTreeMap<String, Value>> {
    values.map(|(id, value)| Ok((id.to_owned(), serde_json::to_value(value)?))).collect()
}
fn changed(before: &BTreeMap<String, Value>, after: &BTreeMap<String, Value>) -> Vec<String> {
    before.keys().chain(after.keys()).collect::<BTreeSet<_>>()
        .into_iter().filter(|key| before.get(*key) != after.get(*key)).cloned().collect()
}
fn fields(scope: &str, id: &str, prefix: &str, before: &Value, after: &Value, out: &mut Vec<PropertyChange>) {
    if before == after { return; }
    if let (Value::Object(a), Value::Object(b)) = (before, after) {
        for key in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
            let path = if prefix.is_empty() { key.to_string() } else { format!("{prefix}.{key}") };
            fields(scope, id, &path, a.get(key).unwrap_or(&Value::Null), b.get(key).unwrap_or(&Value::Null), out);
        }
    } else {
        out.push(PropertyChange { scope: scope.into(), id: id.into(), property: prefix.into(), before: before.clone(), after: after.clone() });
    }
}
pub fn between(before: &Project, after: &Project) -> Result<SemanticDiff> {
    let mut out = SemanticDiff { duration_before_ms: before.duration_ms(), duration_after_ms: after.duration_ms(), ..Default::default() };
    let a = map(before.scenes.iter().map(|s| (s.id.as_str(), s)))?;
    let b = map(after.scenes.iter().map(|s| (s.id.as_str(), s)))?;
    out.scenes_added = b.keys().filter(|id| !a.contains_key(*id)).cloned().collect();
    out.scenes_removed = a.keys().filter(|id| !b.contains_key(*id)).cloned().collect();
    out.scene_order_changed = before.scenes.iter().map(|s| &s.id).collect::<Vec<_>>() != after.scenes.iter().map(|s| &s.id).collect::<Vec<_>>();
    let an = map(before.scenes.iter().flat_map(|s| s.nodes.iter().map(|n| (n.id.as_str(), n))))?;
    let bn = map(after.scenes.iter().flat_map(|s| s.nodes.iter().map(|n| (n.id.as_str(), n))))?;
    out.nodes_added = bn.keys().filter(|id| !an.contains_key(*id)).cloned().collect();
    out.nodes_removed = an.keys().filter(|id| !bn.contains_key(*id)).cloned().collect();
    for id in an.keys().filter(|id| bn.contains_key(*id)) {
        fields("node", id, "", &an[id], &bn[id], &mut out.properties_changed);
    }
    for scene in &before.scenes {
        if let Some(other) = after.scenes.iter().find(|s| s.id == scene.id) {
            if scene.nodes.iter().map(|n| &n.id).collect::<Vec<_>>() != other.nodes.iter().map(|n| &n.id).collect::<Vec<_>>() {
                out.node_order_changed.push(scene.id.clone());
            }
            for key in ["name", "duration_ms", "transition"] {
                fields("scene", &scene.id, key, &a[&scene.id][key], &b[&scene.id][key], &mut out.properties_changed);
            }
        }
    }
    out.animations_changed = changed(
        &map(before.scenes.iter().flat_map(|s| s.animations.iter().map(|a| (a.id.as_str(), a))))?,
        &map(after.scenes.iter().flat_map(|s| s.animations.iter().map(|a| (a.id.as_str(), a))))?,
    );
    out.cues_changed = changed(
        &map(before.scenes.iter().flat_map(|s| s.cues.iter().map(|c| (c.id.as_str(), c))))?,
        &map(after.scenes.iter().flat_map(|s| s.cues.iter().map(|c| (c.id.as_str(), c))))?,
    );
    out.assets_changed = changed(
        &map(before.assets.iter().map(|a| (a.id.as_str(), a)))?,
        &map(after.assets.iter().map(|a| (a.id.as_str(), a)))?,
    );
    let bv = serde_json::to_value(before)?;
    let av = serde_json::to_value(after)?;
    for key in ["settings", "theme", "audio", "component_version"] {
        fields("project", &before.id, key, &bv[key], &av[key], &mut out.properties_changed);
    }
    Ok(out)
}

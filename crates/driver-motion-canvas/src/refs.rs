//! References bind object identity to project generation, revision and content.
use crate::{Error, ErrorCode, Result, model::Project, security};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Project,
    Scene,
    Node,
    Asset,
    Cue,
    Animation,
    RenderJob,
}
impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Scene => "scene",
            Self::Node => "node",
            Self::Asset => "asset",
            Self::Cue => "cue",
            Self::Animation => "animation",
            Self::RenderJob => "render_job",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub project: String,
    pub generation: String,
    pub revision: u64,
    pub fingerprint: String,
    pub kind: Kind,
    pub id: String,
}
impl Reference {
    pub fn new(project: &Project, fingerprint: &str, kind: Kind, id: &str) -> Self {
        Self {
            project: project.id.clone(),
            generation: project.generation.clone(),
            revision: project.revision,
            fingerprint: fingerprint.into(),
            kind,
            id: id.into(),
        }
    }
    pub fn encode(&self) -> String {
        format!(
            "mc1:{}:{}:{}:{}:{}:{}",
            self.project,
            self.generation,
            self.revision,
            self.fingerprint,
            self.kind.as_str(),
            self.id
        )
    }
    pub fn decode(value: &str) -> Result<Self> {
        if value.len() > 320 || !value.is_ascii() {
            return Err(Error::invalid("Reference exceeds bounds"));
        }
        let fields = value.split(':').collect::<Vec<_>>();
        if fields.len() != 7
            || fields[0] != "mc1"
            || !security::identifier(fields[1])
            || fields[2].len() != 32
            || !fields[2].bytes().all(|b| b.is_ascii_hexdigit())
            || !security::digest(fields[4])
            || !security::identifier(fields[6])
        {
            return Err(Error::invalid("Malformed Motion Canvas reference"));
        }
        let kind = match fields[5] {
            "project" => Kind::Project,
            "scene" => Kind::Scene,
            "node" => Kind::Node,
            "asset" => Kind::Asset,
            "cue" => Kind::Cue,
            "animation" => Kind::Animation,
            "render_job" => Kind::RenderJob,
            _ => return Err(Error::invalid("Unknown reference kind")),
        };
        let revision = fields[3]
            .parse::<u64>()
            .map_err(|_| Error::invalid("Invalid reference revision"))?;
        let reference = Self {
            project: fields[1].into(),
            generation: fields[2].into(),
            revision,
            fingerprint: fields[4].into(),
            kind,
            id: fields[6].into(),
        };
        if revision == 0
            || reference.encode() != value
            || (kind == Kind::Project && reference.project != reference.id)
        {
            return Err(Error::invalid("Noncanonical reference"));
        }
        Ok(reference)
    }
    pub fn check(&self, project: &Project, fingerprint: &str, kind: Kind) -> Result<()> {
        if self.kind != kind {
            return Err(Error::invalid("Reference has the wrong object kind"));
        }
        if self.project != project.id
            || self.generation != project.generation
            || self.revision != project.revision
            || self.fingerprint != fingerprint
        {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Project generation, revision or file fingerprint changed; inspect again",
            ));
        }
        let exists = match kind {
            Kind::Project => self.id == project.id,
            Kind::Scene => project.scenes.iter().any(|s| s.id == self.id),
            Kind::Node => project
                .scenes
                .iter()
                .any(|s| s.nodes.iter().any(|n| n.id == self.id)),
            Kind::Asset => project.assets.iter().any(|a| a.id == self.id),
            Kind::Cue => project
                .scenes
                .iter()
                .any(|s| s.cues.iter().any(|c| c.id == self.id)),
            Kind::Animation => project
                .scenes
                .iter()
                .any(|s| s.animations.iter().any(|a| a.id == self.id)),
            Kind::RenderJob => true,
        };
        if !exists {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Referenced object no longer exists",
            ));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObjectRef {
    pub kind: Kind,
    pub id: String,
    pub reference: String,
}
pub fn all(project: &Project, fingerprint: &str) -> Vec<ObjectRef> {
    let mut refs = Vec::new();
    let mut add = |kind, id: &str| {
        refs.push(ObjectRef {
            kind,
            id: id.into(),
            reference: Reference::new(project, fingerprint, kind, id).encode(),
        })
    };
    add(Kind::Project, &project.id);
    for scene in &project.scenes {
        add(Kind::Scene, &scene.id);
        for node in &scene.nodes {
            add(Kind::Node, &node.id);
        }
        for cue in &scene.cues {
            add(Kind::Cue, &cue.id);
        }
        for animation in &scene.animations {
            add(Kind::Animation, &animation.id);
        }
    }
    for asset in &project.assets {
        add(Kind::Asset, &asset.id);
    }
    refs
}

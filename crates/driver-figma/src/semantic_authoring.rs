use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const COMPOSITION_VERSION: u32 = 1;
pub const MAX_COMPOSITION_BYTES: usize = 512 * 1024;
pub const MAX_COMPOSITION_NODES: usize = 512;
pub const MAX_COMPOSITION_DEPTH: usize = 24;
pub const MAX_RELATIONSHIPS: usize = 1024;
pub const MAX_PROFILES: usize = 8;
pub const MAX_ASSET_REFS: usize = 64;
pub const MAX_TEXT_BYTES: usize = 65_536;
pub const MAX_REPAIR_OPERATIONS: usize = 64;
pub const MAX_VALIDATORS: usize = 64;
pub const MAX_CONVERGENCE_ITERATIONS: u32 = 8;
pub const MAX_CONVERGENCE_MUTATIONS: u32 = 128;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FigmaCompositionSpecV1 {
    pub version: u32,
    pub target: CompositionTargetV1,
    pub nodes: Vec<CompositionNodeV1>,
    #[serde(default)]
    pub relationships: Vec<RelationshipV1>,
    #[serde(default)]
    pub profiles: Vec<ResponsiveProfileV1>,
    #[serde(default)]
    pub validators: Vec<ValidatorRequestV1>,
    #[serde(default)]
    pub budgets: CompositionBudgetsV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CompositionTargetV1 {
    pub page_id: Option<String>,
    pub parent_node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompositionNodeKindV1 {
    Frame,
    Section,
    Stack,
    Row,
    Grid,
    Split,
    Overlay,
    Text,
    Shape,
    Media,
    ComponentInstance,
    SemanticRegion,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompositionNodeV1 {
    pub id: String,
    pub kind: CompositionNodeKindV1,
    pub name: String,
    pub parent: Option<String>,
    #[serde(default)]
    pub order: u32,
    pub role: Option<String>,
    pub profile: Option<String>,
    pub layout: Option<LayoutIntentV1>,
    pub sizing: Option<SizingIntentV1>,
    pub text: Option<TextIntentV1>,
    pub visual: Option<VisualIntentV1>,
    pub media: Option<MediaIntentV1>,
    pub component: Option<ComponentIntentV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LayoutDirectionV1 {
    None,
    Vertical,
    Horizontal,
    Grid,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LayoutIntentV1 {
    pub direction: LayoutDirectionV1,
    #[serde(default)]
    pub gap: f64,
    #[serde(default)]
    pub padding: EdgeInsetsV1,
    #[serde(default)]
    pub align: AlignIntentV1,
    #[serde(default)]
    pub distribute: DistributionIntentV1,
    #[serde(default)]
    pub wrap: bool,
    #[serde(default)]
    pub absolute_children: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct EdgeInsetsV1 {
    #[serde(default)]
    pub top: f64,
    #[serde(default)]
    pub right: f64,
    #[serde(default)]
    pub bottom: f64,
    #[serde(default)]
    pub left: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AlignIntentV1 {
    #[default]
    Start,
    Center,
    End,
    Baseline,
    Stretch,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum DistributionIntentV1 {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SizingModeV1 {
    Hug,
    Fill,
    Fixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AxisSizingV1 {
    pub mode: SizingModeV1,
    pub value: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SizingIntentV1 {
    pub width: AxisSizingV1,
    pub height: AxisSizingV1,
    pub aspect_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextFitStrategyV1 {
    GrowHeight,
    Reflow,
    MaxLines,
    Truncate,
    BoundedShrink,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextIntentV1 {
    pub characters: String,
    pub font_family: Option<String>,
    pub font_style: Option<String>,
    pub font_size: Option<f64>,
    pub line_height: Option<f64>,
    pub letter_spacing: Option<f64>,
    pub max_lines: Option<u32>,
    #[serde(default = "default_text_fit")]
    pub fit: TextFitStrategyV1,
}

fn default_text_fit() -> TextFitStrategyV1 {
    TextFitStrategyV1::GrowHeight
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct VisualIntentV1 {
    pub fill: Option<FillIntentV1>,
    pub radius: Option<f64>,
    pub opacity: Option<f64>,
    pub text_style: Option<DesignRefV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FillIntentV1 {
    Solid { r: f64, g: f64, b: f64, a: f64 },
    Variable { variable: DesignRefV1 },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DesignRefV1 {
    pub id: Option<String>,
    pub key: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MediaIntentV1 {
    pub image_hash: String,
    pub scale_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ComponentIntentV1 {
    pub component: DesignRefV1,
    #[serde(default)]
    pub variant_properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipKindV1 {
    Before,
    After,
    AlignedWith,
    BaselineWith,
    SameWidthAs,
    SameHeightAs,
    CenteredIn,
    AnchoredTo,
    MinimumGap,
    MaximumGap,
    AspectRatio,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RelationshipV1 {
    pub kind: RelationshipKindV1,
    pub subject: String,
    pub object: Option<String>,
    pub value: Option<f64>,
    pub tolerance: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResponsiveProfileV1 {
    pub name: String,
    pub width: f64,
    pub root_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorKindV1 {
    TextClipping,
    ParentOverflow,
    SiblingOverlap,
    DeclaredSpacing,
    Alignment,
    AspectRatio,
    TouchTarget,
    Contrast,
    Fonts,
    AutoLayout,
    ResponsiveProfile,
    PrototypeRefs,
    ComponentRelationships,
    RequiredBindings,
    MediaDeformation,
    HiddenOverflow,
    NativeText,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ValidatorRequestV1 {
    pub kind: ValidatorKindV1,
    pub severity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompositionBudgetsV1 {
    pub max_nodes: u32,
    pub max_depth: u32,
    pub max_relationships: u32,
    pub max_findings_per_round: u32,
    pub max_repair_operations: u32,
    pub max_iterations: u32,
    pub max_mutations: u32,
}

impl Default for CompositionBudgetsV1 {
    fn default() -> Self {
        Self {
            max_nodes: 256,
            max_depth: 16,
            max_relationships: 512,
            max_findings_per_round: 256,
            max_repair_operations: 32,
            max_iterations: 4,
            max_mutations: 64,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PlanBaseV1 {
    pub document_id: String,
    pub session_id: String,
    pub generation: u64,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlanPurposeV1 {
    Composition,
    Repair,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ResolvedBindingsV1 {
    pub component_id: Option<String>,
    pub text_style_id: Option<String>,
    pub fill_variable_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CreateChangeV1 {
    pub logical_id: String,
    pub parent_logical_id: Option<String>,
    pub resolved: ResolvedBindingsV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RepairActionV1 {
    GrowTextHeight,
    SetAutoLayoutGap { gap: f64 },
    RestoreAspectRatio { ratio: f64 },
    BindVariable { field: String, variable_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ModifyChangeV1 {
    pub node_id: String,
    pub logical_id: Option<String>,
    pub action: RepairActionV1,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FigmaChangeSetV1 {
    pub version: u32,
    #[serde(default)]
    pub creates: Vec<CreateChangeV1>,
    #[serde(default)]
    pub modifies: Vec<ModifyChangeV1>,
    #[serde(default)]
    pub deletes: Vec<String>,
    #[serde(default)]
    pub expected_effects: Vec<String>,
    #[serde(default)]
    pub postconditions: Vec<String>,
    pub required_scopes: Vec<String>,
    pub risk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FigmaPlanV1 {
    pub version: u32,
    pub purpose: PlanPurposeV1,
    pub base: PlanBaseV1,
    pub spec: FigmaCompositionSpecV1,
    pub changeset: FigmaChangeSetV1,
    pub validators: Vec<ValidatorRequestV1>,
    pub digest: String,
}

impl FigmaCompositionSpecV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != COMPOSITION_VERSION {
            return Err("unsupported composition spec version".into());
        }
        if self.nodes.is_empty() || self.nodes.len() > MAX_COMPOSITION_NODES {
            return Err("composition node count out of bounds".into());
        }
        if self.relationships.len() > MAX_RELATIONSHIPS || self.profiles.len() > MAX_PROFILES {
            return Err("composition relationship/profile count out of bounds".into());
        }
        if self.validators.len() > MAX_VALIDATORS {
            return Err("validator count out of bounds".into());
        }
        self.budgets.validate()?;
        if self.nodes.len() > self.budgets.max_nodes as usize
            || self.relationships.len() > self.budgets.max_relationships as usize
        {
            return Err("composition exceeds declared budgets".into());
        }
        if let Some(page_id) = &self.target.page_id {
            validate_id(page_id)?;
        }
        if let Some(parent_node_id) = &self.target.parent_node_id {
            validate_id(parent_node_id)?;
        }

        let mut ids = BTreeSet::new();
        let mut asset_refs = 0usize;
        for node in &self.nodes {
            validate_id(&node.id)?;
            if !ids.insert(node.id.as_str()) {
                return Err(format!("duplicate composition node id: {}", node.id));
            }
            if node.name.is_empty() || node.name.len() > 256 {
                return Err(format!("invalid node name for {}", node.id));
            }
            if let Some(role) = &node.role {
                if role.len() > 128 {
                    return Err(format!("role too large for {}", node.id));
                }
            }
            validate_node_numbers(node)?;
            if matches!(node.kind, CompositionNodeKindV1::Text) != node.text.is_some() {
                return Err(format!("text intent mismatch for {}", node.id));
            }
            if let Some(text) = &node.text {
                if text.characters.len() > MAX_TEXT_BYTES {
                    return Err(format!("text too large for {}", node.id));
                }
                if let Some(family) = &text.font_family {
                    validate_bounded_string(family, 256, "font family")?;
                }
                if let Some(style) = &text.font_style {
                    validate_bounded_string(style, 256, "font style")?;
                }
            }
            if let Some(visual) = &node.visual {
                if let Some(style) = &visual.text_style {
                    validate_design_ref(style)?;
                }
                if let Some(FillIntentV1::Variable { variable }) = &visual.fill {
                    validate_design_ref(variable)?;
                }
            }
            if let Some(media) = &node.media {
                asset_refs += 1;
                validate_bounded_string(&media.image_hash, 256, "image hash")?;
                if !matches!(
                    media.scale_mode.to_ascii_uppercase().as_str(),
                    "FILL" | "FIT" | "CROP" | "TILE"
                ) {
                    return Err(format!("invalid media scale mode for {}", node.id));
                }
            }
            if matches!(node.kind, CompositionNodeKindV1::Media) && node.media.is_none() {
                return Err(format!("media intent missing for {}", node.id));
            }
            if let Some(component) = &node.component {
                validate_design_ref(&component.component)?;
                if component.variant_properties.len() > 64 {
                    return Err(format!("too many component variants for {}", node.id));
                }
                for (key, value) in &component.variant_properties {
                    validate_bounded_string(key, 256, "variant property name")?;
                    validate_bounded_string(value, 256, "variant property value")?;
                }
            }
            if matches!(node.kind, CompositionNodeKindV1::ComponentInstance)
                && node.component.is_none()
            {
                return Err(format!("component intent missing for {}", node.id));
            }
        }
        if asset_refs > MAX_ASSET_REFS {
            return Err("asset reference count out of bounds".into());
        }

        for node in &self.nodes {
            if let Some(parent) = &node.parent {
                if !ids.contains(parent.as_str()) {
                    return Err(format!("unknown parent {parent} for {}", node.id));
                }
            }
            if let Some(profile) = &node.profile {
                if !self.profiles.iter().any(|p| p.name == *profile) {
                    return Err(format!("unknown profile {profile} for {}", node.id));
                }
            }
        }
        for relation in &self.relationships {
            if !ids.contains(relation.subject.as_str()) {
                return Err(format!("unknown relationship subject {}", relation.subject));
            }
            if let Some(object) = &relation.object {
                if !ids.contains(object.as_str()) {
                    return Err(format!("unknown relationship object {object}"));
                }
            } else if !matches!(relation.kind, RelationshipKindV1::AspectRatio) {
                return Err(format!(
                    "relationship {:?} requires an object",
                    relation.kind
                ));
            }
            validate_optional_finite(relation.value, "relationship value")?;
            validate_optional_finite(relation.tolerance, "relationship tolerance")?;
            if relation.tolerance.is_some_and(|value| value < 0.0) {
                return Err("relationship tolerance cannot be negative".into());
            }
            match relation.kind {
                RelationshipKindV1::MinimumGap | RelationshipKindV1::MaximumGap => {
                    if relation.value.is_none_or(|value| value < 0.0) {
                        return Err("gap relationship requires a non-negative value".into());
                    }
                }
                RelationshipKindV1::AspectRatio => {
                    if relation.value.is_none_or(|value| value <= 0.0) {
                        return Err("aspect ratio relationship requires a positive value".into());
                    }
                }
                _ => {}
            }
        }
        let mut profile_names = BTreeSet::new();
        for profile in &self.profiles {
            if profile.name.is_empty()
                || profile.name.len() > 64
                || !profile.width.is_finite()
                || profile.width <= 0.0
            {
                return Err("invalid responsive profile".into());
            }
            if !profile_names.insert(profile.name.as_str()) {
                return Err(format!("duplicate responsive profile {}", profile.name));
            }
            if !ids.contains(profile.root_id.as_str()) {
                return Err(format!("unknown profile root {}", profile.root_id));
            }
        }
        for validator in &self.validators {
            if let Some(severity) = &validator.severity {
                if !matches!(severity.as_str(), "error" | "warning" | "info") {
                    return Err("invalid validator severity".into());
                }
            }
        }
        self.validate_depth(&ids)
    }

    fn validate_depth(&self, ids: &BTreeSet<&str>) -> Result<(), String> {
        let parents: BTreeMap<&str, Option<&str>> = self
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node.parent.as_deref()))
            .collect();
        for id in ids {
            let mut current = Some(*id);
            let mut seen = BTreeSet::new();
            let mut depth = 0usize;
            while let Some(value) = current {
                if !seen.insert(value) {
                    return Err(format!("composition parent cycle at {value}"));
                }
                depth += 1;
                if depth > MAX_COMPOSITION_DEPTH || depth > self.budgets.max_depth as usize {
                    return Err(format!("composition depth exceeded at {id}"));
                }
                current = parents.get(value).copied().flatten();
            }
        }
        Ok(())
    }
}

impl CompositionBudgetsV1 {
    fn validate(&self) -> Result<(), String> {
        if self.max_nodes == 0
            || self.max_nodes as usize > MAX_COMPOSITION_NODES
            || self.max_depth == 0
            || self.max_depth as usize > MAX_COMPOSITION_DEPTH
            || self.max_relationships as usize > MAX_RELATIONSHIPS
            || self.max_repair_operations as usize > MAX_REPAIR_OPERATIONS
            || self.max_iterations == 0
            || self.max_iterations > MAX_CONVERGENCE_ITERATIONS
            || self.max_mutations == 0
            || self.max_mutations > MAX_CONVERGENCE_MUTATIONS
            || self.max_findings_per_round == 0
            || self.max_findings_per_round > 1000
        {
            return Err("composition budget out of bounds".into());
        }
        Ok(())
    }
}

impl FigmaChangeSetV1 {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != COMPOSITION_VERSION {
            return Err("unsupported changeset version".into());
        }
        if !self.deletes.is_empty() {
            return Err("semantic authoring changesets do not embed destructive deletes".into());
        }
        if self.creates.len() > MAX_COMPOSITION_NODES
            || self.modifies.len() > MAX_REPAIR_OPERATIONS
            || self.expected_effects.len() > MAX_RELATIONSHIPS
            || self.postconditions.len() > MAX_RELATIONSHIPS
        {
            return Err("changeset exceeds bounded limits".into());
        }
        if self.required_scopes != ["driver:figma"] {
            return Err("changeset must use the existing Figma driver scope".into());
        }
        if self.risk != "mutating_reversible" {
            return Err("semantic authoring changeset risk must be mutating_reversible".into());
        }
        for effect in self.expected_effects.iter().chain(&self.postconditions) {
            validate_bounded_string(effect, 512, "changeset effect")?;
        }
        let mut create_ids = BTreeSet::new();
        for change in &self.creates {
            validate_id(&change.logical_id)?;
            if !create_ids.insert(change.logical_id.as_str()) {
                return Err(format!("duplicate changeset create {}", change.logical_id));
            }
            if let Some(parent) = &change.parent_logical_id {
                validate_id(parent)?;
            }
            for resolved in [
                change.resolved.component_id.as_deref(),
                change.resolved.text_style_id.as_deref(),
                change.resolved.fill_variable_id.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                validate_id(resolved)?;
            }
        }
        for change in &self.creates {
            if let Some(parent) = &change.parent_logical_id {
                if !create_ids.contains(parent.as_str()) {
                    return Err(format!("unknown changeset parent {parent}"));
                }
            }
        }
        for change in &self.modifies {
            validate_id(&change.node_id)?;
            if let Some(logical_id) = &change.logical_id {
                validate_id(logical_id)?;
            }
            match &change.action {
                RepairActionV1::SetAutoLayoutGap { gap } if !gap.is_finite() || *gap < 0.0 => {
                    return Err("invalid repair gap".into());
                }
                RepairActionV1::RestoreAspectRatio { ratio }
                    if !ratio.is_finite() || *ratio <= 0.0 =>
                {
                    return Err("invalid repair aspect ratio".into());
                }
                RepairActionV1::BindVariable { field, variable_id } => {
                    if field != "fill_color" {
                        return Err("unsupported variable repair field".into());
                    }
                    validate_id(variable_id)?;
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl FigmaPlanV1 {
    pub fn new(
        purpose: PlanPurposeV1,
        base: PlanBaseV1,
        spec: FigmaCompositionSpecV1,
        changeset: FigmaChangeSetV1,
    ) -> Result<Self, String> {
        spec.validate()?;
        changeset.validate()?;
        validate_id(&base.document_id)?;
        validate_id(&base.session_id)?;
        let validators = spec.validators.clone();
        let mut plan = Self {
            version: COMPOSITION_VERSION,
            purpose,
            base,
            spec,
            changeset,
            validators,
            digest: String::new(),
        };
        plan.digest = plan.compute_digest()?;
        Ok(plan)
    }

    pub fn verify(&self) -> Result<(), String> {
        if self.version != COMPOSITION_VERSION {
            return Err("unsupported plan version".into());
        }
        self.spec.validate()?;
        self.changeset.validate()?;
        validate_id(&self.base.document_id)?;
        validate_id(&self.base.session_id)?;
        let expected = self.compute_digest()?;
        if self.digest != expected {
            return Err("plan digest mismatch".into());
        }
        Ok(())
    }

    fn compute_digest(&self) -> Result<String, String> {
        let mut canonical = self.clone();
        canonical.digest.clear();
        let bytes = serde_json::to_vec(&canonical).map_err(|_| "could not serialize plan")?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }
}

fn validate_id(value: &str) -> Result<(), String> {
    validate_bounded_string(value, 256, "identifier")
}

fn validate_bounded_string(value: &str, max: usize, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(format!("invalid bounded {label}"));
    }
    Ok(())
}

fn validate_design_ref(reference: &DesignRefV1) -> Result<(), String> {
    let mut selectors = 0usize;
    for (value, label) in [
        (reference.id.as_deref(), "design ref id"),
        (reference.key.as_deref(), "design ref key"),
        (reference.name.as_deref(), "design ref name"),
    ] {
        if let Some(value) = value {
            selectors += 1;
            validate_bounded_string(value, 256, label)?;
        }
    }
    if selectors == 0 {
        return Err("design ref requires id, key, or name".into());
    }
    Ok(())
}

fn validate_optional_finite(value: Option<f64>, label: &str) -> Result<(), String> {
    if value.is_some_and(|n| !n.is_finite()) {
        return Err(format!("{label} must be finite"));
    }
    Ok(())
}

fn validate_node_numbers(node: &CompositionNodeV1) -> Result<(), String> {
    if let Some(layout) = &node.layout {
        if !layout.gap.is_finite() || layout.gap < 0.0 {
            return Err(format!("invalid layout gap for {}", node.id));
        }
        for value in [
            layout.padding.top,
            layout.padding.right,
            layout.padding.bottom,
            layout.padding.left,
        ] {
            if !value.is_finite() || value < 0.0 {
                return Err(format!("invalid layout padding for {}", node.id));
            }
        }
    }
    if let Some(sizing) = &node.sizing {
        for axis in [&sizing.width, &sizing.height] {
            for value in [axis.value, axis.min, axis.max].into_iter().flatten() {
                if !value.is_finite() || value < 0.0 {
                    return Err(format!("invalid sizing value for {}", node.id));
                }
            }
        }
        if sizing
            .aspect_ratio
            .is_some_and(|n| !n.is_finite() || n <= 0.0)
        {
            return Err(format!("invalid aspect ratio for {}", node.id));
        }
    }
    if let Some(text) = &node.text {
        for value in [text.font_size, text.line_height, text.letter_spacing]
            .into_iter()
            .flatten()
        {
            if !value.is_finite() {
                return Err(format!("invalid typography value for {}", node.id));
            }
        }
        if text.max_lines.is_some_and(|n| n == 0 || n > 1000) {
            return Err(format!("invalid max_lines for {}", node.id));
        }
    }
    if let Some(visual) = &node.visual {
        validate_optional_finite(visual.radius, "radius")?;
        validate_optional_finite(visual.opacity, "opacity")?;
        if visual.opacity.is_some_and(|n| !(0.0..=1.0).contains(&n)) {
            return Err(format!("invalid opacity for {}", node.id));
        }
        if let Some(FillIntentV1::Solid { r, g, b, a }) = &visual.fill {
            for value in [r, g, b, a] {
                if !value.is_finite() || !(0.0..=1.0).contains(value) {
                    return Err(format!("invalid solid fill for {}", node.id));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, parent: Option<&str>) -> CompositionNodeV1 {
        CompositionNodeV1 {
            id: id.into(),
            kind: CompositionNodeKindV1::Frame,
            name: id.into(),
            parent: parent.map(str::to_owned),
            order: 0,
            role: None,
            profile: None,
            layout: None,
            sizing: None,
            text: None,
            visual: None,
            media: None,
            component: None,
        }
    }
    fn spec() -> FigmaCompositionSpecV1 {
        FigmaCompositionSpecV1 {
            version: 1,
            target: CompositionTargetV1 {
                page_id: None,
                parent_node_id: None,
            },
            nodes: vec![node("root", None), node("child", Some("root"))],
            relationships: vec![],
            profiles: vec![],
            validators: vec![ValidatorRequestV1 {
                kind: ValidatorKindV1::ParentOverflow,
                severity: None,
            }],
            budgets: CompositionBudgetsV1::default(),
        }
    }

    #[test]
    fn rejects_cycles_and_unknown_parents() {
        let mut value = spec();
        value.nodes[0].parent = Some("child".into());
        assert!(value.validate().unwrap_err().contains("cycle"));

        let mut value = spec();
        value.nodes[1].parent = Some("missing".into());
        assert!(value.validate().unwrap_err().contains("unknown parent"));
    }

    #[test]
    fn plan_digest_detects_tampering() {
        let changeset = FigmaChangeSetV1 {
            version: 1,
            creates: vec![CreateChangeV1 {
                logical_id: "root".into(),
                parent_logical_id: None,
                resolved: ResolvedBindingsV1::default(),
            }],
            modifies: vec![],
            deletes: vec![],
            expected_effects: vec![],
            postconditions: vec![],
            required_scopes: vec!["driver:figma".into()],
            risk: "mutating_reversible".into(),
        };
        let base = PlanBaseV1 {
            document_id: "doc".into(),
            session_id: "session".into(),
            generation: 7,
            revision: 4,
        };
        let mut plan =
            FigmaPlanV1::new(PlanPurposeV1::Composition, base, spec(), changeset).unwrap();
        plan.verify().unwrap();
        plan.base.revision += 1;
        assert_eq!(plan.verify().unwrap_err(), "plan digest mismatch");
    }

    #[test]
    fn destructive_delete_is_not_embeddable() {
        let changeset = FigmaChangeSetV1 {
            version: 1,
            creates: vec![],
            modifies: vec![],
            deletes: vec!["1:2".into()],
            expected_effects: vec![],
            postconditions: vec![],
            required_scopes: vec!["driver:figma".into()],
            risk: "destructive".into(),
        };
        assert!(changeset.validate().unwrap_err().contains("destructive"));
    }

    #[test]
    fn spec_deserialization_rejects_unknown_executable_fields() {
        let value = serde_json::json!({
            "version": 1,
            "target": {"page_id": null, "parent_node_id": null},
            "nodes": [{
                "id": "root",
                "kind": "frame",
                "name": "Root",
                "script": "figma.currentPage.remove()"
            }],
            "relationships": [],
            "profiles": [],
            "validators": [],
            "budgets": {
                "max_nodes": 16,
                "max_depth": 4,
                "max_relationships": 16,
                "max_findings_per_round": 16,
                "max_repair_operations": 4,
                "max_iterations": 2,
                "max_mutations": 8
            }
        });
        let error = serde_json::from_value::<FigmaCompositionSpecV1>(value).unwrap_err();
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn plan_digest_is_deterministic_for_identical_inputs() {
        let changeset = FigmaChangeSetV1 {
            version: 1,
            creates: vec![],
            modifies: vec![],
            deletes: vec![],
            expected_effects: vec!["native".into()],
            postconditions: vec!["verify".into()],
            required_scopes: vec!["driver:figma".into()],
            risk: "mutating_reversible".into(),
        };
        let base = PlanBaseV1 {
            document_id: "doc".into(),
            session_id: "session".into(),
            generation: 3,
            revision: 9,
        };
        let first = FigmaPlanV1::new(
            PlanPurposeV1::Composition,
            base.clone(),
            spec(),
            changeset.clone(),
        )
        .unwrap();
        let second = FigmaPlanV1::new(PlanPurposeV1::Composition, base, spec(), changeset).unwrap();
        assert_eq!(first.digest, second.digest);
    }

    #[test]
    fn declared_budget_cannot_expand_global_resource_limits() {
        let mut value = spec();
        value.budgets.max_nodes = (MAX_COMPOSITION_NODES + 1) as u32;
        assert!(value.validate().unwrap_err().contains("budget"));
        let mut value = spec();
        value.budgets.max_iterations = MAX_CONVERGENCE_ITERATIONS + 1;
        assert!(value.validate().unwrap_err().contains("budget"));
    }

    #[test]
    fn design_refs_require_bounded_identity_and_profiles_are_unique() {
        let mut value = spec();
        value.nodes[0].visual = Some(VisualIntentV1 {
            fill: Some(FillIntentV1::Variable {
                variable: DesignRefV1 {
                    id: None,
                    key: None,
                    name: None,
                },
            }),
            radius: None,
            opacity: None,
            text_style: None,
        });
        assert!(value.validate().unwrap_err().contains("design ref"));

        let mut value = spec();
        value.profiles = vec![
            ResponsiveProfileV1 {
                name: "desktop".into(),
                width: 1440.0,
                root_id: "root".into(),
            },
            ResponsiveProfileV1 {
                name: "desktop".into(),
                width: 1280.0,
                root_id: "root".into(),
            },
        ];
        assert!(
            value
                .validate()
                .unwrap_err()
                .contains("duplicate responsive profile")
        );
    }

    #[test]
    fn changeset_cannot_smuggle_scope_risk_or_unknown_parent() {
        let mut changeset = FigmaChangeSetV1 {
            version: 1,
            creates: vec![CreateChangeV1 {
                logical_id: "child".into(),
                parent_logical_id: Some("missing".into()),
                resolved: ResolvedBindingsV1::default(),
            }],
            modifies: vec![],
            deletes: vec![],
            expected_effects: vec![],
            postconditions: vec![],
            required_scopes: vec!["driver:figma".into()],
            risk: "mutating_reversible".into(),
        };
        assert!(
            changeset
                .validate()
                .unwrap_err()
                .contains("unknown changeset parent")
        );

        changeset.creates.clear();
        changeset.required_scopes = vec!["figma.superuser".into()];
        assert!(
            changeset
                .validate()
                .unwrap_err()
                .contains("existing Figma driver scope")
        );

        changeset.required_scopes = vec!["driver:figma".into()];
        changeset.risk = "destructive".into();
        assert!(
            changeset
                .validate()
                .unwrap_err()
                .contains("mutating_reversible")
        );
    }
}

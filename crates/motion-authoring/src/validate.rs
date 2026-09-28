use crate::*;
use semwright_media_time::Rational as Q;
use semwright_semantic_composition::{Result, canonical_bytes, ensure};
use std::collections::{BTreeMap, BTreeSet};
pub(crate) fn id(v: &str) -> Result<()> {
    ensure(
        !v.is_empty()
            && v.len() <= 96
            && v.bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-')),
        "logical identifier must be ASCII alphanumeric/underscore/hyphen",
    )
}
pub(crate) fn finite(v: f64, min: f64, max: f64) -> Result<()> {
    ensure(
        v.is_finite() && (min..=max).contains(&v),
        "numeric intent outside finite bounds",
    )
}
pub(crate) fn point(p: Point) -> Result<()> {
    finite(p.x, -32768.0, 32768.0)?;
    finite(p.y, -32768.0, 32768.0)
}
pub(crate) fn color(v: &str) -> Result<()> {
    ensure(
        [4, 7, 9].contains(&v.len())
            && v.starts_with('#')
            && v.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit),
        "color must be literal RGB/RGBA hex",
    )
}
fn text(v: &str, max: usize) -> Result<()> {
    ensure(
        v.len() <= max
            && !v
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
        "text budget or forbidden controls",
    )
}
fn insets(v: Insets) -> Result<()> {
    for n in [v.top, v.right, v.bottom, v.left] {
        finite(n, 0.0, 4096.0)?;
    }
    Ok(())
}
fn font(f: &FontSpec) -> Result<()> {
    ensure(!f.family.is_empty(), "font family required")?;
    text(&f.family, 256)?;
    ensure(f.permitted_fallbacks.len() <= 8, "font fallback count")?;
    for s in &f.permitted_fallbacks {
        text(s, 256)?;
    }
    ensure(
        f.fallback != FontFallback::Deny || f.permitted_fallbacks.is_empty(),
        "deny fallback cannot declare implicit alternatives",
    )
}
impl Film {
    pub fn validate(&self) -> Result<()> {
        ensure(self.version == AUTHORING_VERSION, "Film version mismatch")?;
        id(&self.id)?;
        self.output.frame_rate.validate()?;
        ensure(
            self.output.frame_rate.num as u64 <= 120 * u64::from(self.output.frame_rate.den),
            "frame rate above renderer limit",
        )?;
        ensure(
            self.output.frame_rate.num >= self.output.frame_rate.den,
            "frame rate below one",
        )?;
        ensure(
            (16..=4096).contains(&self.output.width)
                && (16..=4096).contains(&self.output.height)
                && u64::from(self.output.width) * u64::from(self.output.height) <= 8_847_360,
            "output dimensions exceed renderer bounds",
        )?;
        insets(self.output.safe_area)?;
        ensure(
            self.output.safe_area.left + self.output.safe_area.right < f64::from(self.output.width)
                && self.output.safe_area.top + self.output.safe_area.bottom
                    < f64::from(self.output.height),
            "safe area consumes entire output",
        )?;
        ensure(
            match self.output.aspect {
                AspectFamily::Landscape => self.output.width > self.output.height,
                AspectFamily::Portrait => self.output.height > self.output.width,
                AspectFamily::Square => self.output.width == self.output.height,
                AspectFamily::Arbitrary => true,
            },
            "aspect profile inconsistent with dimensions",
        )?;
        let e = &self.editorial;
        ensure(e.version == 1, "editorial system version")?;
        font(&e.font)?;
        font(&e.mono_font)?;
        ensure(
            !e.type_scale.is_empty()
                && e.type_scale.len() <= 32
                && e.colors.len() <= 30
                && e.spacing.len() <= 32,
            "editorial token budgets",
        )?;
        for (k, v) in &e.type_scale {
            id(k)?;
            finite(*v, 4.0, 512.0)?;
        }
        for (k, v) in &e.colors {
            id(k)?;
            color(v)?;
        }
        for (k, v) in &e.spacing {
            id(k)?;
            finite(*v, 0.0, 1024.0)?;
        }
        finite(e.stroke, 0.0, 64.0)?;
        finite(e.corner_radius, 0.0, 1024.0)?;
        ensure(
            !self.sequences.is_empty() && self.sequences.len() <= 32 && self.assets.len() <= 128,
            "Film collection budgets",
        )?;
        let mut assets = BTreeSet::new();
        for a in &self.assets {
            id(&a.id)?;
            ensure(assets.insert(&a.id), "duplicate asset")?;
            text(&a.media_type, 128)?;
            for s in [&a.provenance, &a.license].into_iter().flatten() {
                text(s, 1024)?;
            }
        }
        let cues = self.cues.resolve()?;
        let mut all = BTreeSet::new();
        let mut count = 0;
        let mut invocation_count = 0;
        for sequence in &self.sequences {
            id(&sequence.id)?;
            ensure(all.insert(sequence.id.clone()), "duplicate sequence")?;
            id(&sequence.span_id)?;
            ensure(
                !sequence.beats.is_empty() && sequence.beats.len() <= 32,
                "beat count",
            )?;
            for beat in &sequence.beats {
                id(&beat.id)?;
                ensure(all.insert(beat.id.clone()), "duplicate beat")?;
                ensure(
                    !beat.shots.is_empty() && beat.shots.len() <= 32,
                    "shot count",
                )?;
                for shot in &beat.shots {
                    id(&shot.id)?;
                    ensure(all.insert(shot.id.clone()), "duplicate shot")?;
                    id(&shot.span_id)?;
                    ensure(
                        !shot.subjects.is_empty()
                            && shot.subjects.len() <= 512
                            && shot.layers.len() <= 32
                            && shot.annotations.len() <= 64
                            && shot.captions.len() <= 256
                            && shot.constraints.len() <= 256,
                        "shot collection budgets",
                    )?;
                    let mut layers = BTreeSet::new();
                    for layer in &shot.layers {
                        id(&layer.id)?;
                        ensure(layers.insert(&layer.id), "duplicate layer")?;
                    }
                    let mut subjects = BTreeMap::new();
                    for subject in &shot.subjects {
                        id(&subject.id)?;
                        ensure(
                            all.insert(subject.id.clone()) && !subject.id.starts_with("sw-shot-"),
                            "duplicate/reserved logical subject",
                        )?;
                        ensure(layers.contains(&subject.layer), "unknown subject layer")?;
                        subjects.insert(subject.id.as_str(), subject);
                        text(&subject.role, 128)?;
                        count += 1;
                        match &subject.content {
                            SubjectContent::Group => {}
                            SubjectContent::Text {
                                runs,
                                style,
                                language,
                                ..
                            } => {
                                ensure(
                                    !runs.is_empty()
                                        && runs.len() <= 128
                                        && e.type_scale.contains_key(style),
                                    "text style/runs invalid",
                                )?;
                                text(language, 64)?;
                                let total = runs.iter().map(|r| r.text.len()).sum::<usize>();
                                ensure(total <= 16384, "text byte budget")?;
                                for run in runs {
                                    text(&run.text, 16384)?;
                                    ensure((100..=900).contains(&run.weight), "font weight")?;
                                    if let Some(v) = &run.color {
                                        color(v)?;
                                    }
                                }
                            }
                            SubjectContent::Rectangle {
                                fill,
                                stroke,
                                radius,
                            } => {
                                color(fill)?;
                                if let Some(v) = stroke {
                                    color(v)?;
                                }
                                finite(*radius, 0.0, 8192.0)?;
                            }
                            SubjectContent::Circle { fill, stroke } => {
                                color(fill)?;
                                if let Some(v) = stroke {
                                    color(v)?;
                                }
                            }
                            SubjectContent::Path {
                                points,
                                stroke,
                                stroke_width,
                                ..
                            } => {
                                ensure((2..=512).contains(&points.len()), "path point budget")?;
                                for p in points {
                                    point(*p)?;
                                }
                                color(stroke)?;
                                finite(*stroke_width, 0.0, 64.0)?;
                            }
                            SubjectContent::Image {
                                asset_id, ratio, ..
                            }
                            | SubjectContent::Video {
                                asset_id, ratio, ..
                            } => {
                                ensure(
                                    assets.contains(asset_id),
                                    "asset must have an explicit digest-bound reference",
                                )?;
                                finite(*ratio, 0.001, 1000.0)?;
                                if let SubjectContent::Video { source_offset, .. } =
                                    &subject.content
                                {
                                    source_offset.validate()?;
                                    ensure(
                                        *source_offset >= Q::ZERO,
                                        "negative source media offset",
                                    )?;
                                }
                            }
                            SubjectContent::Code {
                                source, font_style, ..
                            } => {
                                text(source, 65536)?;
                                ensure(
                                    e.type_scale.contains_key(font_style),
                                    "unknown code type scale",
                                )?;
                            }
                            SubjectContent::Camera { zoom } => finite(*zoom, 0.01, 100.0)?,
                        }
                        match &subject.layout {
                            SpatialIntent::Flow { grow, .. } => finite(*grow, 0.0, 100.0)?,
                            SpatialIntent::Fixed { position, size } => {
                                point(*position)?;
                                finite(size.width, 0.0, 16384.0)?;
                                finite(size.height, 0.0, 16384.0)?;
                            }
                            SpatialIntent::Stack { gap, padding, .. } => {
                                finite(*gap, 0.0, 4096.0)?;
                                insets(*padding)?;
                            }
                            SpatialIntent::Split { ratio, gap, .. } => {
                                finite(*ratio, 0.01, 0.99)?;
                                finite(*gap, 0.0, 4096.0)?;
                            }
                            SpatialIntent::Grid {
                                columns,
                                gap,
                                portrait_columns,
                            } => {
                                ensure(
                                    (1..=32).contains(columns)
                                        && (1..=32).contains(portrait_columns),
                                    "grid column bounds",
                                )?;
                                finite(*gap, 0.0, 4096.0)?;
                            }
                            SpatialIntent::Overlay {
                                anchor,
                                offset,
                                intentional,
                            } => {
                                id(anchor)?;
                                point(*offset)?;
                                ensure(*intentional, "overlay must be explicitly intentional")?;
                            }
                        }
                    }
                    ensure(count <= 1024, "Film subject budget")?;
                    for s in &shot.subjects {
                        let mut seen = BTreeSet::new();
                        let mut next = s.parent.as_deref();
                        while let Some(parent) = next {
                            ensure(
                                parent != s.id && seen.insert(parent) && seen.len() <= 24,
                                "subject parent cycle/depth",
                            )?;
                            let p = subjects.get(parent).ok_or_else(|| {
                                semwright_semantic_composition::ContractError::Invalid(
                                    "missing subject parent".into(),
                                )
                            })?;
                            ensure(
                                !matches!(
                                    p.content,
                                    SubjectContent::Text { .. }
                                        | SubjectContent::Code { .. }
                                        | SubjectContent::Image { .. }
                                        | SubjectContent::Video { .. }
                                ),
                                "subject parent cannot contain semantic children",
                            )?;
                            next = p.parent.as_deref();
                        }
                        if let SpatialIntent::Overlay { anchor, .. } = &s.layout {
                            ensure(
                                subjects.contains_key(anchor.as_str()) && anchor != &s.id,
                                "unknown/self overlay anchor",
                            )?;
                        }
                        if matches!(s.layout, SpatialIntent::Split { .. }) {
                            ensure(
                                shot.subjects
                                    .iter()
                                    .filter(|c| c.parent.as_deref() == Some(&s.id))
                                    .count()
                                    == 2,
                                "split must contain exactly two subjects",
                            )?;
                        }
                        if matches!(s.content, SubjectContent::Camera { .. }) {
                            ensure(
                                matches!(s.layout, SpatialIntent::Flow { .. }),
                                "camera layout is native camera-space, not a layout box",
                            )?;
                        }
                    }
                    for a in &shot.annotations {
                        id(&a.id)?;
                        ensure(
                            all.insert(a.id.clone())
                                && subjects.contains_key(a.subject.as_str())
                                && subjects.contains_key(a.anchor_subject.as_str()),
                            "annotation identity/bindings",
                        )?;
                        point(a.offset)?;
                    }
                    for c in &shot.captions {
                        id(&c.id)?;
                        ensure(
                            all.insert(c.id.clone())
                                && subjects.get(c.subject.as_str()).is_some_and(|s| {
                                    matches!(s.content, SubjectContent::Text { .. })
                                })
                                && cues.contains_key(&c.cue_id),
                            "caption needs native text and existing cue",
                        )?;
                        text(&c.text, 16384)?;
                        text(&c.language, 64)?;
                    }
                    for c in &shot.constraints {
                        let subject = match c {
                            VisualConstraint::SafeArea { subject, tolerance } => {
                                finite(*tolerance, 0.0, 1024.0)?;
                                subject
                            }
                            VisualConstraint::NoOverlap {
                                subject,
                                other,
                                tolerance,
                            } => {
                                finite(*tolerance, 0.0, 1024.0)?;
                                ensure(
                                    subjects.contains_key(other.as_str()) && subject != other,
                                    "overlap relation target",
                                )?;
                                subject
                            }
                            VisualConstraint::MinimumVisible { subject, duration } => {
                                duration.validate()?;
                                ensure(*duration >= Q::ZERO, "negative minimum visibility")?;
                                subject
                            }
                            VisualConstraint::AspectRatio {
                                subject,
                                ratio,
                                tolerance,
                            } => {
                                finite(*ratio, 0.001, 1000.0)?;
                                finite(*tolerance, 0.0, 100.0)?;
                                subject
                            }
                            VisualConstraint::NativeText { subject }
                            | VisualConstraint::FontLoaded { subject }
                            | VisualConstraint::NoTruncation { subject } => subject,
                            VisualConstraint::MaximumSpeed {
                                subject,
                                pixels_per_second,
                            } => {
                                finite(*pixels_per_second, 0.0, 1_000_000.0)?;
                                subject
                            }
                        };
                        ensure(
                            subjects.contains_key(subject.as_str()),
                            "constraint subject not found",
                        )?;
                    }
                    for invocation in &shot.motion {
                        id(&invocation.id)?;
                        ensure(
                            all.insert(invocation.id.clone()),
                            "duplicate motion invocation",
                        )?;
                        id(&invocation.span_id)?;
                        invocation.primitive.validate(shot)?;
                        invocation_count += 1;
                    }
                    ensure(invocation_count <= 1024, "motion invocation budget")?;
                }
            }
        }
        for sequence in &self.sequences {
            let ids = sequence
                .beats
                .iter()
                .flat_map(|b| &b.shots)
                .flat_map(|s| &s.subjects)
                .map(|s| s.id.as_str())
                .collect::<BTreeSet<_>>();
            for motion in sequence
                .beats
                .iter()
                .flat_map(|b| &b.shots)
                .flat_map(|s| &s.motion)
            {
                ensure(
                    motion
                        .primitive
                        .references()
                        .iter()
                        .all(|r| ids.contains(r)),
                    "grammar reference must resolve within its native sequence",
                )?;
            }
        }
        ensure(
            self.shots().map(|s| s.constraints.len()).sum::<usize>() <= 240,
            "aggregate validation rule budget",
        )?;
        canonical_bytes(self)?;
        Ok(())
    }
}

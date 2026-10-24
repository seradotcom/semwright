//! Managed OpenType assets in the existing Motion Canvas provider.
//! Only the ordinary asset import/handoff may admit bytes. FontSpec carries a
//! digest reference; it is never treated as a path, URL or authority grant.
use crate::{
    Error, Result,
    model::{Asset, AssetKind, MAX_ASSET_BYTES, Project},
    security, store,
};
use semwright_motion_authoring::{Film, FontFallback, FontSpec, SubjectContent};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub const MAX_MANAGED_FONTS: usize = 2;
const MAX_REQUIRED_CODEPOINTS: usize = 4096;
const MAX_CODEPOINT_RANGES: usize = 256;

fn unsupported(message: impl Into<String>) -> Error {
    Error::new(semwright_types::ErrorCode::Unsupported, message)
}

fn bundled(family: &str) -> bool {
    ["Instrument Sans Variable", "IBM Plex Mono"]
        .iter()
        .any(|known| known.eq_ignore_ascii_case(family))
}

#[derive(Debug, Clone)]
pub struct FontRequest {
    pub family: String,
    pub asset_id: String,
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub weights: Vec<u16>,
    pub codepoints: Vec<u32>,
}

fn selected<'a>(film: &Film, spec: &FontSpec, assets: &'a [Asset]) -> Result<Option<&'a Asset>> {
    if spec.asset_digest.is_none() {
        if !matches!(
            spec.family.as_str(),
            "Instrument Sans Variable" | "IBM Plex Mono"
        ) || spec.permitted_fallbacks.iter().any(|family| {
            !matches!(
                family.as_str(),
                "Instrument Sans Variable" | "IBM Plex Mono"
            )
        }) {
            return Err(unsupported(
                "Unpinned font family has no managed font Asset digest",
            ));
        }
        return Ok(None);
    }
    if bundled(&spec.family)
        || !security::font(&spec.family)
        || spec.fallback != FontFallback::Deny
        || !spec.permitted_fallbacks.is_empty()
    {
        return Err(unsupported(
            "Managed fonts require an unambiguous family and explicit no-fallback policy",
        ));
    }
    let digest = spec.asset_digest.as_ref().expect("checked above").as_str();
    let references = film
        .assets
        .iter()
        .filter(|asset| asset.sha256.as_str() == digest)
        .collect::<Vec<_>>();
    if references.len() != 1
        || !["font/ttf", "font/otf"].contains(&references[0].media_type.as_str())
    {
        return Err(unsupported(
            "FontSpec digest must identify exactly one font AssetRef",
        ));
    }
    let matches = assets
        .iter()
        .filter(|asset| asset.id == references[0].id)
        .collect::<Vec<_>>();
    if matches.len() != 1 || matches[0].kind != AssetKind::Font || matches[0].sha256 != digest {
        return Err(unsupported(
            "Font must first enter the project through managed asset import",
        ));
    }
    Ok(Some(matches[0]))
}

/// Source planning only. Face identity and cmap coverage are checked again from
/// the actual digest-bound bytes before a Host render job can be started.
pub fn requests(film: &Film, assets: &[Asset]) -> Result<Vec<FontRequest>> {
    let mut by_family: BTreeMap<String, FontRequest> = BTreeMap::new();
    for (spec, is_mono) in [
        (&film.editorial.font, false),
        (&film.editorial.mono_font, true),
    ] {
        let asset = selected(film, spec, assets)?;
        let mut weights = BTreeSet::new();
        let mut codepoints = BTreeSet::new();
        for subject in film.shots().flat_map(|shot| &shot.subjects) {
            match &subject.content {
                SubjectContent::Text { runs, .. } if !is_mono => {
                    for run in runs {
                        if run.emphasis && asset.is_some() {
                            return Err(unsupported(
                                "Managed font profile requires an actual normal face; italic synthesis is not admitted",
                            ));
                        }
                        weights.insert(run.weight);
                        codepoints.extend(
                            run.text
                                .chars()
                                .map(u32::from)
                                .filter(|cp| needs_glyph(*cp)),
                        );
                    }
                }
                SubjectContent::Code { source, .. } if is_mono => {
                    weights.insert(400);
                    codepoints.extend(source.chars().map(u32::from).filter(|cp| needs_glyph(*cp)));
                }
                _ => {}
            }
        }
        if weights.is_empty() {
            weights.insert(400);
        }
        if weights.len() > 16
            || weights.iter().any(|weight| !(1..=1000).contains(weight))
            || codepoints.len() > MAX_REQUIRED_CODEPOINTS
        {
            return Err(unsupported(
                "Managed font face or glyph request exceeds its bounded profile",
            ));
        }
        let Some(asset) = asset else {
            // The pinned CSS and actual wght axis are 400..700 for Instrument
            // Sans; the existing readiness contract observes these four weights.
            // IBM Plex Mono is the actual 400 face. Refuse other authored weights
            // before realization/planning instead of letting CSS clamp/synthesize.
            let supported: &[u16] = match spec.family.as_str() {
                "Instrument Sans Variable" => &[400, 500, 600, 700],
                "IBM Plex Mono" => &[400],
                _ => return Err(unsupported("Bundled font family is not pinned")),
            };
            if weights.iter().any(|weight| !supported.contains(weight)) {
                return Err(unsupported(
                    "Authored bundled font weight has no pinned readiness support",
                ));
            }
            continue;
        };
        let key = spec.family.to_lowercase();
        if let Some(previous) = by_family.get_mut(&key) {
            if previous.family != spec.family
                || previous.sha256 != asset.sha256
                || previous.asset_id != asset.id
            {
                return Err(unsupported(
                    "A font family cannot identify multiple managed faces",
                ));
            }
            weights.extend(previous.weights.iter().copied());
            codepoints.extend(previous.codepoints.iter().copied());
            previous.weights = weights.into_iter().collect();
            previous.codepoints = codepoints.into_iter().collect();
        } else {
            by_family.insert(
                key,
                FontRequest {
                    family: spec.family.clone(),
                    asset_id: asset.id.clone(),
                    path: asset.path.clone(),
                    sha256: asset.sha256.clone(),
                    bytes: asset.bytes,
                    weights: weights.into_iter().collect(),
                    codepoints: codepoints.into_iter().collect(),
                },
            );
        }
    }
    if by_family.len() > MAX_MANAGED_FONTS
        || by_family
            .values()
            .any(|font| font.weights.len() > 16 || font.codepoints.len() > MAX_REQUIRED_CODEPOINTS)
        || by_family
            .values()
            .map(|font| &font.sha256)
            .collect::<BTreeSet<_>>()
            .len()
            != by_family.len()
    {
        return Err(unsupported(
            "Managed font family, digest or glyph budget exceeded",
        ));
    }
    Ok(by_family.into_values().collect())
}

fn needs_glyph(cp: u32) -> bool {
    cp > 32 && ![0x200c, 0x200d, 0xfe0f].contains(&cp)
}

fn css_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Deterministic first-party CSS; paths are existing managed Asset paths and
/// every family is quoted. No remote URL, local() fallback or arbitrary CSS.
pub fn stylesheet(project: &Project) -> Result<Option<Vec<u8>>> {
    let Some(binding) = &project.authoring else {
        return Ok(None);
    };
    let requests = requests(&binding.intent, &project.assets)?;
    if requests.is_empty() {
        return Ok(None);
    }
    let mut css = String::from("/* Semwright managed font assets v1. */\n");
    for request in requests {
        security::relative_path(&request.path)?;
        let first = request
            .weights
            .first()
            .expect("requests always includes a weight");
        let last = request
            .weights
            .last()
            .expect("requests always includes a weight");
        let weight = if first == last {
            first.to_string()
        } else {
            format!("{first} {last}")
        };
        css.push_str(&format!(
            "@font-face{{font-family:{};src:url({});font-style:normal;font-weight:{};font-display:block;}}\n",
            css_string(&request.family), css_string(&format!("../{}", request.path)), weight,
        ));
    }
    css.push_str("*{font-synthesis:none;}\n");
    Ok(Some(css.into_bytes()))
}

#[derive(Debug, Clone, Serialize)]
pub struct FontMetadata {
    pub family_names: Vec<String>,
    pub normal: bool,
    pub weight_min: u16,
    pub weight_max: u16,
    pub variable_weight: bool,
    pub glyph_count: u16,
    pub embedding_rights: u16,
}

/// Host configuration generated only after reading and parsing the actual
/// immutable managed asset. codepoints contains observed cmap coverage of the
/// requested text, not a promise inferred from a family name or font license.
#[derive(Debug, Clone, Serialize)]
pub struct ManagedFontEvidence {
    pub family: String,
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
    pub weights: Vec<u16>,
    pub codepoints: Vec<[u32; 2]>,
    pub sample: String,
    pub face_loaded: bool,
    pub metadata: FontMetadata,
}

fn ranges(points: &[u32]) -> Result<Vec<[u32; 2]>> {
    let mut output: Vec<[u32; 2]> = Vec::new();
    for point in points {
        if let Some(last) = output.last_mut()
            && last[1].checked_add(1) == Some(*point)
        {
            last[1] = *point;
        } else {
            output.push([*point, *point]);
        }
    }
    if output.len() > MAX_CODEPOINT_RANGES {
        return Err(unsupported(
            "Verified glyph coverage exceeds Host metadata bounds",
        ));
    }
    Ok(output)
}

pub fn evidence(project: &Project, generated_root: &Path) -> Result<Vec<ManagedFontEvidence>> {
    let Some(binding) = &project.authoring else {
        return Ok(Vec::new());
    };
    requests(&binding.intent, &project.assets)?
        .into_iter()
        .map(|request| {
            let bytes = store::read_granted_file(generated_root, &request.path, MAX_ASSET_BYTES)?;
            if bytes.len() as u64 != request.bytes || security::sha256(&bytes) != request.sha256 {
                return Err(unsupported(
                    "Managed font bytes changed before native render admission",
                ));
            }
            let (metadata, covered) = inspect_for_request(&bytes, &request)?;
            Ok(ManagedFontEvidence {
                family: request.family,
                path: request.path,
                sha256: request.sha256,
                bytes: request.bytes,
                weights: request.weights,
                codepoints: ranges(&covered)?,
                sample: covered
                    .first()
                    .and_then(|cp| char::from_u32(*cp))
                    .unwrap_or(' ')
                    .to_string(),
                face_loaded: false,
                metadata,
            })
        })
        .collect()
}

fn face(bytes: &[u8]) -> Result<ttf_parser::Face<'_>> {
    if bytes.is_empty()
        || bytes.len() > MAX_ASSET_BYTES
        || !(bytes.starts_with(&[0, 1, 0, 0]) || bytes.starts_with(b"OTTO"))
    {
        return Err(unsupported(
            "Managed font profile accepts one bounded TTF/OpenType face, not collections or WOFF",
        ));
    }
    ttf_parser::Face::parse(bytes, 0)
        .map_err(|_| unsupported("Managed font bytes are not a valid OpenType face"))
}

fn metadata(face: &ttf_parser::Face<'_>) -> Result<FontMetadata> {
    use ttf_parser::{Permissions, Tag, name_id};
    if face.is_italic()
        || face.is_oblique()
        || (face.tables().glyf.is_none()
            && face.tables().cff.is_none()
            && face.tables().cff2.is_none())
    {
        return Err(unsupported(
            "Managed font profile requires normal scalable outlines",
        ));
    }
    if !matches!(
        face.permissions(),
        Some(Permissions::Installable | Permissions::Editable)
    ) || !face.is_outline_embedding_allowed()
    {
        return Err(unsupported(
            "Font embedding metadata does not permit an editable outline asset",
        ));
    }
    let names = face.names();
    if names.len() > 256 {
        return Err(unsupported("Managed font naming table exceeds bounds"));
    }
    let mut family_names = BTreeSet::new();
    for name in names {
        if matches!(name.name_id, name_id::FAMILY | name_id::TYPOGRAPHIC_FAMILY)
            && name.name.len() <= 1024
            && let Some(value) = name.to_string()
            && security::font(&value)
        {
            family_names.insert(value);
        }
    }
    if family_names.is_empty() || family_names.len() > 16 {
        return Err(unsupported(
            "Managed font has no bounded, decodable family identity",
        ));
    }
    let axes = face.variation_axes();
    if axes.len() > 1
        || axes
            .into_iter()
            .any(|axis| axis.tag != Tag::from_bytes(b"wght"))
    {
        return Err(unsupported(
            "Only a single explicit weight variation axis is admitted",
        ));
    }
    let (weight_min, weight_max, variable_weight) = if let Some(axis) = axes.into_iter().next() {
        if !axis.min_value.is_finite()
            || !axis.max_value.is_finite()
            || axis.min_value < 1.0
            || axis.max_value > 1000.0
            || axis.min_value > axis.max_value
        {
            return Err(unsupported("Managed font weight axis is invalid"));
        }
        (
            axis.min_value.ceil() as u16,
            axis.max_value.floor() as u16,
            true,
        )
    } else {
        let weight = face.weight().to_number();
        if !(1..=1000).contains(&weight) {
            return Err(unsupported("Managed font has no valid weight class"));
        }
        (weight, weight, false)
    };
    if weight_min > weight_max {
        return Err(unsupported("Managed font exposes no integral CSS weight"));
    }
    // OS/2 has already been parsed and its permissions checked by ttf-parser.
    let os2 = face
        .raw_face()
        .table(Tag::from_bytes(b"OS/2"))
        .ok_or_else(|| unsupported("Managed font has no OS/2 evidence"))?;
    let fs_type = os2
        .get(8..10)
        .ok_or_else(|| unsupported("Managed font embedding metadata is truncated"))?;
    let embedding_rights = u16::from_be_bytes([fs_type[0], fs_type[1]]);
    if !matches!(embedding_rights & 0x000f, 0 | 8) || embedding_rights & 0x0200 != 0 {
        return Err(unsupported(
            "Managed font embedding flags require unambiguous editable outline permission",
        ));
    }
    Ok(FontMetadata {
        family_names: family_names.into_iter().collect(),
        normal: true,
        weight_min,
        weight_max,
        variable_weight,
        glyph_count: face.number_of_glyphs(),
        embedding_rights,
    })
}

/// Called by the existing typed asset.import before installation.
pub fn inspect(bytes: &[u8]) -> Result<FontMetadata> {
    metadata(&face(bytes)?)
}

fn inspect_for_request(bytes: &[u8], request: &FontRequest) -> Result<(FontMetadata, Vec<u32>)> {
    let face = face(bytes)?;
    let metadata = metadata(&face)?;
    if !metadata.family_names.contains(&request.family) {
        return Err(unsupported(
            "Selected font family differs from the imported font naming table",
        ));
    }
    if request.weights.is_empty()
        || request
            .weights
            .iter()
            .any(|weight| *weight < metadata.weight_min || *weight > metadata.weight_max)
    {
        return Err(unsupported(
            "A requested weight has no real face or weight-axis coverage",
        ));
    }
    for point in &request.codepoints {
        let glyph = char::from_u32(*point).and_then(|point| face.glyph_index(point));
        if !glyph.is_some_and(|glyph| glyph.0 != 0 && glyph.0 < metadata.glyph_count) {
            return Err(unsupported(format!(
                "Managed font does not cover required code point U+{point:04X}"
            )));
        }
    }
    Ok((metadata, request.codepoints.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_font_css_quotes_source_and_disallows_local_fallback() {
        assert_eq!(
            css_string("Brand \"Face\"\\Name"),
            "\"Brand \\\"Face\\\"\\\\Name\""
        );
    }

    #[test]
    fn managed_glyph_ranges_only_cover_exact_observed_cmap_points() {
        assert_eq!(
            ranges(&[65, 66, 68, 0x1f600]).unwrap(),
            vec![[65, 66], [68, 68], [0x1f600, 0x1f600]]
        );
        assert!(!needs_glyph(10));
        assert!(!needs_glyph(0xfe0f));
        assert!(needs_glyph(0x00f1));
        assert!(ranges(&(0..514).map(|n| n * 2 + 33).collect::<Vec<_>>()).is_err());
    }
    const TEST_FONT: &[u8] = include_bytes!("../tests/fixtures/managed-font-source.ttf");

    fn request(bytes: &[u8]) -> FontRequest {
        FontRequest {
            family: "COLRv1 Static Test Glyphs".into(),
            asset_id: "font-fixture".into(),
            path: "assets/font-fixture.ttf".into(),
            sha256: security::sha256(bytes),
            bytes: bytes.len() as u64,
            weights: vec![400],
            codepoints: vec![0xF0100],
        }
    }

    fn with_os2_field(offset: usize, value: u16) -> Vec<u8> {
        let face = face(TEST_FONT).unwrap();
        let table = face
            .raw_face()
            .table(ttf_parser::Tag::from_bytes(b"OS/2"))
            .unwrap();
        let at = table.as_ptr() as usize - TEST_FONT.as_ptr() as usize + offset;
        let mut altered = TEST_FONT.to_vec();
        altered[at..at + 2].copy_from_slice(&value.to_be_bytes());
        altered
    }

    #[test]
    fn managed_font_parser_observes_actual_face_name_weight_and_cmap() {
        let (metadata, covered) = inspect_for_request(TEST_FONT, &request(TEST_FONT)).unwrap();
        assert!(
            metadata
                .family_names
                .contains(&"COLRv1 Static Test Glyphs".into())
        );
        assert_eq!((metadata.weight_min, metadata.weight_max), (400, 400));
        assert!(!metadata.variable_weight);
        assert!(metadata.normal);
        assert_eq!(metadata.embedding_rights, 0);
        assert_eq!(covered, vec![0xF0100]);
    }

    #[test]
    fn managed_font_parser_rejects_wrong_family_missing_glyph_and_synthetic_weight() {
        let original = request(TEST_FONT);
        let mut wrong = original.clone();
        wrong.family = "Unrelated Brand".into();
        assert!(inspect_for_request(TEST_FONT, &wrong).is_err());
        wrong = original.clone();
        wrong.weights = vec![700];
        assert!(inspect_for_request(TEST_FONT, &wrong).is_err());
        wrong = original;
        wrong.codepoints = vec![u32::from('A')];
        assert!(inspect_for_request(TEST_FONT, &wrong).is_err());
    }

    #[test]
    fn managed_font_parser_rejects_restricted_preview_and_bitmap_embedding() {
        for fs_type in [2, 4, 0x200] {
            assert!(
                inspect(&with_os2_field(8, fs_type)).is_err(),
                "fsType={fs_type}"
            );
        }
        let editable = inspect(&with_os2_field(8, 8)).unwrap();
        assert_eq!(editable.embedding_rights, 8);
    }

    #[test]
    fn managed_font_parser_rejects_italic_and_oblique_faces() {
        // fsSelection offset62: real binary flags, not caller-authored metadata.
        for flags in [1, 0x200] {
            assert!(inspect(&with_os2_field(62, flags)).is_err());
        }
    }

    #[test]
    fn managed_font_parser_rejects_malformed_wrapped_and_collection_sources() {
        for bytes in [
            b"wOF2not-a-font".as_slice(),
            b"ttcfnot-a-font",
            b"OTTO",
            b"",
        ] {
            assert!(inspect(bytes).is_err());
        }
        let mut broken = TEST_FONT.to_vec();
        broken.truncate(20);
        assert!(inspect(&broken).is_err());
    }
}

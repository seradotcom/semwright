//! Pure input boundaries shared by the driver, compiler and fuzz targets.
use crate::{Error, Result, model::*};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
}
pub fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn relative_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > 512
        || path.split('/').count() > 12
        || path.contains(['\\', ':', '%', '\0'])
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_-.".contains(&b))
        || path
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == ".." || s.len() > 128)
    {
        return Err(Error::invalid(
            "Path must be a bounded relative path without traversal or URLs",
        ));
    }
    Ok(())
}
pub fn literal_color(value: &str) -> bool {
    value == "transparent"
        || (matches!(value.len(), 4 | 5 | 7 | 9)
            && value.starts_with('#')
            && value[1..].bytes().all(|b| b.is_ascii_hexdigit()))
}
pub fn font(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" _-".contains(&b))
}
/// The compiler emits JSON string expressions in external TypeScript modules.
/// No user content is inserted into HTML or a JavaScript template literal.
pub fn js_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing a string is infallible")
}
/// A deliberately small, non-active SVG geometry subset.
pub fn validate_svg(value: &str) -> Result<()> {
    if value.len() > MAX_SVG {
        return Err(Error::invalid("SVG exceeds bounds"));
    }
    let doc = roxmltree::Document::parse(value).map_err(|_| Error::invalid("Malformed SVG"))?;
    if doc.root_element().tag_name().name() != "svg" {
        return Err(Error::invalid("Expected an SVG root"));
    }
    let mut count = 0;
    let mut ids = BTreeSet::new();
    for node in doc.descendants() {
        if node.is_pi() {
            return Err(Error::invalid(
                "SVG processing instructions are not supported",
            ));
        }
        if !node.is_element() {
            continue;
        }
        count += 1;
        if count > 2048 || node.attributes().len() > 32 {
            return Err(Error::invalid("SVG tree exceeds bounds"));
        }
        if !matches!(
            node.tag_name().name(),
            "svg"
                | "g"
                | "path"
                | "rect"
                | "circle"
                | "ellipse"
                | "line"
                | "polyline"
                | "polygon"
                | "title"
                | "desc"
        ) {
            return Err(Error::invalid("Unsupported SVG element"));
        }
        for attr in node.attributes() {
            if attr.namespace().is_some() {
                return Err(Error::invalid(
                    "Namespaced SVG attributes are not supported",
                ));
            }
            let name = attr.name();
            let v = attr.value();
            if v.len() > 65536 {
                return Err(Error::invalid("SVG attribute exceeds bounds"));
            }
            if !matches!(
                name,
                "id" | "viewBox"
                    | "width"
                    | "height"
                    | "x"
                    | "y"
                    | "x1"
                    | "x2"
                    | "y1"
                    | "y2"
                    | "cx"
                    | "cy"
                    | "r"
                    | "rx"
                    | "ry"
                    | "d"
                    | "points"
                    | "transform"
                    | "fill"
                    | "stroke"
                    | "stroke-width"
                    | "stroke-linecap"
                    | "stroke-linejoin"
                    | "stroke-miterlimit"
                    | "stroke-dasharray"
                    | "stroke-dashoffset"
                    | "fill-rule"
                    | "opacity"
                    | "fill-opacity"
                    | "stroke-opacity"
                    | "preserveAspectRatio"
                    | "version"
            ) {
                return Err(Error::invalid("Unsupported SVG attribute"));
            }
            if name == "id" {
                if !identifier(v) || !ids.insert(v) {
                    return Err(Error::invalid("Invalid SVG id"));
                }
            } else if matches!(name, "fill" | "stroke") {
                if !literal_color(v) && v != "none" && v != "currentColor" {
                    return Err(Error::invalid("SVG paint must be a literal color"));
                }
            } else if !v
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b" .,()+-%\t\r\n".contains(&b))
            {
                return Err(Error::invalid("Unsupported SVG attribute value"));
            }
        }
    }
    Ok(())
}
/// Only a fixed vocabulary of mathematical commands reaches the math renderer.
pub fn validate_latex(value: &str) -> Result<()> {
    if value.len() > 8192 {
        return Err(Error::invalid("LaTeX exceeds bounds"));
    }
    let command = regex::Regex::new(r"\\([A-Za-z]+)").expect("constant regex");
    for token in command.captures_iter(value) {
        if !matches!(
            &token[1],
            "frac"
                | "sqrt"
                | "sum"
                | "prod"
                | "int"
                | "lim"
                | "sin"
                | "cos"
                | "tan"
                | "log"
                | "ln"
                | "exp"
                | "alpha"
                | "beta"
                | "gamma"
                | "delta"
                | "theta"
                | "pi"
                | "sigma"
                | "phi"
                | "omega"
                | "Delta"
                | "Theta"
                | "Pi"
                | "Sigma"
                | "Omega"
                | "infty"
                | "cdot"
                | "times"
                | "div"
                | "pm"
                | "leq"
                | "geq"
                | "neq"
                | "approx"
                | "rightarrow"
                | "left"
                | "right"
                | "begin"
                | "end"
                | "mathrm"
                | "mathbf"
                | "mathbb"
                | "operatorname"
                | "text"
                | "quad"
                | "qquad"
                | "overline"
                | "underline"
                | "vec"
                | "hat"
                | "binom"
                | "displaystyle"
        ) {
            return Err(Error::invalid("Unsupported mathematical command"));
        }
    }
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PngEvidence {
    pub width: u32,
    pub height: u32,
    pub min_alpha: u8,
    pub max_alpha: u8,
    pub pixel_sha256: String,
}
pub fn inspect_png(bytes: &[u8]) -> Result<PngEvidence> {
    if bytes.len() > MAX_ASSET_BYTES {
        return Err(Error::invalid("PNG exceeds byte limit"));
    }
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_limits(png::Limits { bytes: 67_108_864 });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|_| Error::invalid("Invalid PNG header"))?;
    let info = reader.info();
    if info.width == 0
        || info.height == 0
        || info.width > 4096
        || info.height > 4096
        || u64::from(info.width) * u64::from(info.height) > 8_847_360
        || info.animation_control.is_some()
    {
        return Err(Error::invalid("PNG dimensions or animation exceed limits"));
    }
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| Error::invalid("PNG buffer overflow"))?;
    if size > 67_108_864 {
        return Err(Error::invalid("PNG decode allocation exceeds limit"));
    }
    let mut buffer = vec![0; size];
    let output = reader
        .next_frame(&mut buffer)
        .map_err(|_| Error::invalid("Invalid PNG frame or checksum"))?;
    let buffer = &buffer[..output.buffer_size()];
    let mut hash = Sha256::new();
    let mut min_alpha = 255;
    let mut max_alpha = 0;
    let stride = match output.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => return Err(Error::invalid("Unexpanded PNG pixels")),
    };
    for pixel in buffer.chunks_exact(stride) {
        let rgba = match stride {
            1 => [pixel[0], pixel[0], pixel[0], 255],
            2 => [pixel[0], pixel[0], pixel[0], pixel[1]],
            3 => [pixel[0], pixel[1], pixel[2], 255],
            _ => [pixel[0], pixel[1], pixel[2], pixel[3]],
        };
        min_alpha = min_alpha.min(rgba[3]);
        max_alpha = max_alpha.max(rgba[3]);
        hash.update(rgba);
    }
    Ok(PngEvidence {
        width: output.width,
        height: output.height,
        min_alpha,
        max_alpha,
        pixel_sha256: format!("{:x}", hash.finalize()),
    })
}

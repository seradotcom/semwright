//! Exact Driver Protocol v1 shapes over 4-byte big-endian length-prefixed UTF-8 JSON.
use crate::{
    Error, Result,
    app::{App, VERSION},
    catalog,
    hash::{reader_hash, valid_digest},
    json::{self, Value, obj},
};
use std::io::{Read, Write};
pub const MAX_FRAME: usize = 1_048_576;
pub fn read_frame(reader: &mut impl Read) -> Result<Option<Value>> {
    let mut prefix = [0; 4];
    let n = reader.read(&mut prefix[..1])?;
    if n == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut prefix[1..])?;
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(Error::limit("Driver frame length outside 1..1048576"));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Some(json::parse(&body)?))
}
pub fn write_raw(writer: &mut impl Write, text: &str) -> Result<()> {
    if text.is_empty() || text.len() > MAX_FRAME {
        return Err(Error::limit("Driver response frame exceeds budget"));
    }
    writer.write_all(&(text.len() as u32).to_be_bytes())?;
    writer.write_all(text.as_bytes())?;
    writer.flush()?;
    Ok(())
}
pub fn write_frame(writer: &mut impl Write, value: &Value) -> Result<()> {
    write_raw(writer, &value.encode())
}
fn request_id(v: &Value) -> Result<String> {
    let id = v.str("id")?;
    if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
        return Err(Error::invalid("Invalid request ID"));
    }
    Ok(id.into())
}
pub fn validate_hello(v: &Value, own_digest: &str) -> Result<()> {
    v.strict(
        &["type", "protocol", "provider", "executable_sha256"],
        &["type", "protocol", "provider", "executable_sha256"],
    )?;
    if v.str("type")? != "hello" || v.uint("protocol")? != 1 {
        return Err(Error::new(
            "ProtocolMismatch",
            "Driver Protocol v1 Hello required",
        ));
    }
    let p = v.get("provider")?;
    p.strict(
        &[
            "id",
            "kind",
            "version",
            "namespace",
            "application",
            "origin",
        ],
        &[
            "id",
            "kind",
            "version",
            "namespace",
            "application",
            "origin",
        ],
    )?;
    if p.str("id")? != catalog::PROVIDER
        || p.str("kind")? != "driver"
        || p.str("version")? != VERSION
        || p.str("namespace")? != catalog::PREFIX
    {
        return Err(Error::new(
            "PermissionDenied",
            "Owner provider identity does not match this driver",
        ));
    }
    let origin = p.str("origin")?;
    if origin.is_empty() || origin.len() > 256 || origin.chars().any(char::is_control) {
        return Err(Error::invalid("Invalid provider origin"));
    }
    if let Value::String(app) = p.get("application")? {
        if app.is_empty() || app.len() > 256 || app.chars().any(char::is_control) {
            return Err(Error::invalid("Invalid application identity"));
        }
    } else if p.get("application")? != &Value::Null {
        return Err(Error::invalid("Application must be string or null"));
    }
    let digest = v.str("executable_sha256")?;
    if !valid_digest(digest) || digest != own_digest {
        return Err(Error::new(
            "PermissionDenied",
            "Hello executable digest does not match the running ELF",
        ));
    }
    Ok(())
}
pub fn serve(
    app: &mut App,
    reader: &mut impl Read,
    writer: &mut impl Write,
    own_digest: &str,
) -> Result<()> {
    let hello =
        read_frame(reader)?.ok_or_else(|| Error::new("ProtocolMismatch", "Hello missing"))?;
    validate_hello(&hello, own_digest)?;
    write_frame(
        writer,
        &obj([
            ("type", "ready".into()),
            ("protocol", 1u64.into()),
            ("id", "mlt-video".into()),
            ("version", VERSION.into()),
        ]),
    )?;
    while let Some(request) = read_frame(reader)? {
        let op = request.str("type")?;
        if op == "hello" {
            return Err(Error::new("ProtocolMismatch", "Hello cannot be repeated"));
        }
        let id = request_id(&request)?;
        let result: Result<Option<Value>> = (|| match op {
            "capabilities" => {
                request.strict(&["type", "id"], &["type", "id"])?;
                let raw = catalog::catalog_wire(&app.capabilities);
                let frame = json::ordered(&[
                    ("type", json::quote("capabilities")),
                    ("id", json::quote(&id)),
                    ("capabilities", raw),
                    ("digest", json::quote(&catalog::digest(&app.capabilities))),
                ]);
                write_raw(writer, &frame)?;
                Ok(None)
            }
            "health" => {
                request.strict(&["type", "id"], &["type", "id"])?;
                Ok(Some(obj([
                    ("type", "healthy".into()),
                    ("id", id.clone().into()),
                    ("details", app.doctor()),
                ])))
            }
            "execute" => {
                request.strict(
                    &["type", "id", "command", "descriptor_sha256", "args"],
                    &["type", "id", "command", "descriptor_sha256", "args"],
                )?;
                let value = app.execute(
                    request.str("command")?,
                    request.str("descriptor_sha256")?,
                    request.get("args")?.clone(),
                )?;
                Ok(Some(obj([
                    ("type", "result".into()),
                    ("id", id.clone().into()),
                    ("value", value),
                ])))
            }
            "shutdown" => {
                request.strict(&["type", "id"], &["type", "id"])?;
                app.jobs.shutdown();
                Ok(Some(obj([
                    ("type", "shutdown".into()),
                    ("id", id.clone().into()),
                ])))
            }
            _ => Err(Error::new(
                "ProtocolMismatch",
                "Unsupported Driver Protocol v1 request",
            )),
        })();
        let shutdown_ok = op == "shutdown" && result.is_ok();
        match result {
            Ok(Some(v)) => write_frame(writer, &v)?,
            Ok(None) => {}
            Err(e) => write_frame(
                writer,
                &obj([
                    ("type", "failure".into()),
                    ("id", id.into()),
                    ("error", e.json()),
                ]),
            )?,
        }
        if shutdown_ok {
            return Ok(());
        }
    }
    app.jobs.shutdown();
    Ok(())
}
pub fn executable_digest() -> Result<String> {
    reader_hash(std::fs::File::open("/proc/self/exe")?, 64 * 1024 * 1024).map(|p| p.0)
}

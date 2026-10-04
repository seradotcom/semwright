//! Static cooperation metadata for explicit operator review. This grants nothing.
//! Uses the pinned canonical descriptor hash and canonical JSON implementation.
use crate::{Error, Model, NativeApp, Result, Value, descriptor_digest, json, sha256};
use semwright_effect_conformance::composition::canonical_bytes;

fn hash(domain: &str, value: &Value) -> Result<String> {
    let mut bytes = domain.as_bytes().to_vec();
    bytes.push(0);
    bytes.extend(canonical_bytes(value).map_err(|e| Error::invalid(e.to_string()))?);
    Ok(format!("sha256:{}", sha256(&bytes)))
}
pub fn native_copy_profile<M: Model>(model: M, driver_binary_sha256: &str) -> Result<Value> {
    let id = model.id();
    if id.is_empty()
        || id.len() > 64
        || !id.as_bytes()[0].is_ascii_lowercase()
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(Error::invalid(
            "Native copy profile requires a canonical domain",
        ));
    }
    let sha = driver_binary_sha256
        .strip_prefix("sha256:")
        .ok_or_else(|| Error::invalid("Binary SHA256 required"))?;
    if sha.len() != 64
        || !sha
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::invalid("Binary SHA256 required"));
    }
    let operations = model.operations();
    if operations.is_empty() || operations.len() > 16 {
        return Err(Error::invalid("Bounded native mutations required"));
    }
    let caps = NativeApp::describe_model(model);
    let pin = |name: &str| -> Result<Value> {
        let expected = format!("driver.{id}.{name}");
        let cap = caps
            .iter()
            .find(|c| c.descriptor.name == expected)
            .ok_or_else(|| Error::invalid("Profile capability absent"))?;
        Ok(
            json!({"capability_id":expected,"descriptor_digest":format!("sha256:{}",descriptor_digest(&cap.descriptor)?),"input_schema_digest":hash("semwright-native-copy-schema-v1",&cap.descriptor.input_schema)?,"output_schema_digest":hash("semwright-native-copy-schema-v1",&cap.descriptor.output_schema)?}),
        )
    };
    let mutations = operations
        .iter()
        .map(|op| pin(op.name))
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"schema_version":"semwright-native-copy-profile/1","id":"native-sdk-fork/v1","domain":id,"provider":format!("driver:{id}"),"driver_binary_sha256":driver_binary_sha256,"source_semantics":"SELF_CONTAINED_NATIVE_DOCUMENT","commands":{"inspect":pin("inspect")?,"fork":pin("fork")?,"export":pin("export")?},"mutations":mutations}),
    )
}

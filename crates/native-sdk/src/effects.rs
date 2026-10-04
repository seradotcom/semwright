//! Explicit read-only protected verifier CLI. No application/Host launch.
use semwright_effect_conformance::composition::{
    ContractError, Digest, Result, canonical_bytes, strict_decode,
};
use semwright_native_sdk::effects_readback::{
    self, ProtectedSpec, SpecificationInput, VerificationResult,
};
use std::{
    io::{Read, Write},
    path::Path,
};
fn run() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.len() == 1 && args[0] == "--prepare" {
        // Pure preparation only: this authenticates nothing and grants no filesystem
        // or execution authority. The resulting spec is re-derived by verify().
        const MAX_DEFINITION_BYTES: usize = 256 * 1024;
        let mut input = Vec::new();
        std::io::stdin()
            .take((MAX_DEFINITION_BYTES + 1) as u64)
            .read_to_end(&mut input)
            .map_err(|_| ContractError::Invalid("definition input unavailable".into()))?;
        if input.len() > MAX_DEFINITION_BYTES {
            return Err(ContractError::Limit(
                "definition byte budget exceeded".into(),
            ));
        }
        let definition: SpecificationInput = strict_decode(&input)?;
        let spec = effects_readback::prepare_spec(definition)?;
        let bytes = canonical_bytes(&spec)?;
        std::io::stdout()
            .write_all(&bytes)
            .map_err(|_| ContractError::Unknown("prepared spec output unavailable".into()))?;
        println!();
        return Ok(());
    }
    if args.len() == 2 && args[0] == "--schema" {
        let value = match args[1].as_str() {
            "spec" => serde_json::to_value(schemars::schema_for!(ProtectedSpec)),
            "result" => serde_json::to_value(schemars::schema_for!(VerificationResult)),
            _ => {
                return Err(ContractError::Invalid(
                    "schema must be spec or result".into(),
                ));
            }
        }
        .map_err(|e| ContractError::Invalid(e.to_string()))?;
        println!(
            "{}",
            serde_json::to_string_pretty(&value)
                .map_err(|e| ContractError::Invalid(e.to_string()))?
        );
        return Ok(());
    }
    if args.len() != 6
        || args[0] != "--spec"
        || args[2] != "--spec-sha256"
        || args[4] != "--artifact-root"
    {
        return Err(ContractError::Invalid(
            "expected --prepare | --spec FILE --spec-sha256 HEX --artifact-root ROOT".into(),
        ));
    }
    let mut input = Vec::new();
    std::io::stdin()
        .take(65)
        .read_to_end(&mut input)
        .map_err(|_| ContractError::Invalid("stdin unavailable".into()))?;
    effects_readback::validate_public_input(&input)?;
    let digest = Digest::parse(args[3].clone())?;
    let verified = effects_readback::verify(Path::new(&args[1]), &digest, Path::new(&args[5]))?;
    let bytes =
        serde_json::to_vec(verified.result()).map_err(|e| ContractError::Invalid(e.to_string()))?;
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|_| ContractError::Unknown("result output unavailable".into()))?;
    println!();
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!(
            "{}",
            serde_json::json!({"schema_version":"semwright-native-effects-error/1","state":"ERROR","message":error.to_string(),"canonical_report":null,"execution_authority":false})
        );
        std::process::exit(2);
    }
}

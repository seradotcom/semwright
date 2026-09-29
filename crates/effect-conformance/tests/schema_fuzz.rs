//! Reproducible bounded mutation fuzz, not a coverage-guided libFuzzer claim.
use semwright_effect_conformance::composition::*;
use semwright_effect_conformance::*;
#[test]
fn bounded_schema_mutation_fuzz_15000_cases() {
    let seed = include_bytes!("../fixtures/e0.json");
    let mut attempted = 0usize;
    let mut accepted = 0usize;
    for initial in [0x58b1f2u64, 0x9ac730u64, 0xd02efeu64] {
        let mut state = initial;
        for iteration in 0..5000 {
            let mut bytes = seed.to_vec();
            for _ in 0..=iteration % 8 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let pos = (state as usize) % bytes.len();
                bytes[pos] = (state >> 24) as u8;
            }
            if let Ok(contract) = strict_decode::<EffectContract>(&bytes) {
                let _ = contract.validate();
                let _ = contract.digest();
                accepted += 1;
            }
            let _ = strict_decode::<AdapterObservation>(&bytes);
            let _ = strict_decode::<Predicate>(&bytes);
            attempted += 1;
        }
    }
    assert_eq!(attempted, 15000);
    println!(
        "FUZZ_RECEIPT attempted={attempted} accepted_contracts={accepted} seeds=3 max_bytes={} mode=bounded-mutation",
        seed.len()
    );
}

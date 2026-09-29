//! Reproducible bounded mutation fuzz, not a coverage-guided libFuzzer claim.
mod common;
use common::*;
use semwright_effect_conformance::composition::*;
use semwright_effect_conformance::*;
#[test]
fn bounded_schema_mutation_fuzz_15000_cases() {
    let (contract, context) = fixture();
    let corpus = [
        canonical_bytes(&contract).unwrap(),
        canonical_bytes(&observation(&context, &contract.rules[0])).unwrap(),
        canonical_bytes(&contract.rules[0].predicate).unwrap(),
    ];
    assert!(strict_decode::<EffectContract>(&corpus[0]).is_ok());
    assert!(strict_decode::<AdapterObservation>(&corpus[1]).is_ok());
    assert!(strict_decode::<Predicate>(&corpus[2]).is_ok());
    let mut attempted = 0usize;
    let mut accepted = [0usize; 3];
    for initial in [0x58b1f2u64, 0x9ac730u64, 0xd02efeu64] {
        let mut state = initial;
        for iteration in 0..5000 {
            let target = iteration % corpus.len();
            let mut bytes = corpus[target].clone();
            for _ in 0..iteration % 8 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                if bytes.is_empty() {
                    break;
                }
                let pos = (state as usize) % bytes.len();
                match state % 4 {
                    0 => bytes[pos] = (state >> 24) as u8,
                    1 => {
                        bytes.remove(pos);
                    }
                    2 => bytes.insert(pos, (state >> 32) as u8),
                    _ => bytes.truncate(pos),
                }
            }
            match target {
                0 => {
                    if let Ok(c) = strict_decode::<EffectContract>(&bytes) {
                        let _ = c.validate();
                        let _ = c.digest();
                        accepted[0] += 1;
                    }
                }
                1 => {
                    if let Ok(o) = strict_decode::<AdapterObservation>(&bytes) {
                        if let Some(value) = o.value {
                            let _ = value.validate();
                        }
                        let _ = o.observation.base.validate();
                        accepted[1] += 1;
                    }
                }
                _ => {
                    if let Ok(p) = strict_decode::<Predicate>(&bytes) {
                        let _ = p.validate();
                        let _ = p.compare(&ObservedValue::Bool { value: false });
                        accepted[2] += 1;
                    }
                }
            }
            attempted += 1;
        }
    }
    assert_eq!(attempted, 15000);
    assert!(accepted.iter().all(|n| *n > 0));
    println!(
        "FUZZ_RECEIPT attempted={attempted} accepted_by_schema={accepted:?} seeds=3 schemas=3 mode=bounded-mutation"
    );
}

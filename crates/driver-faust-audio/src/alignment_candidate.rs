//! Bounded candidate transport validation. This module creates no native authority.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use semwright_audio_domain::wav::WaveReader;
use semwright_types::{Error, ErrorCode, Result};
use serde::Deserialize;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{fmt, fs::File, path::Path};

pub(crate) const COMMAND: &str = "driver.audio-analysis.voice.align";
pub(crate) const PROFILE: &str = "audio-analysis-alignment-cpu-py312-v1";
pub(crate) const BRIDGE_SHA: &str =
    "ffca843d76030f264bd017285154f2cbb7fca8818e7eeed8d04d7e7715d8e15b";
pub(crate) const WORKER_SHA: &str =
    "60cfbdfe02796b0ee6333808a0c847c1642a0ff80a400e3ee88933570bf5ab42";
pub(crate) const CONTRACT_SHA: &str =
    "cf63e26be28d143e1ee541e93ce5d55c3f438ac9edb791ae6153f4764041ded6";
pub(crate) const CHECKPOINT_SHA: &str =
    "488fd4f16de84438ffc945334278c1b9fb9b7159a806c1080b16111a958c945d";
pub(crate) const MAX_WAV: u64 = 1_048_576;
pub(crate) const MAX_STDIN: usize = 65_536;
pub(crate) const MAX_STDOUT: usize = 245_760;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const ARTIFACTS: [&str; 5] = [
    "source.pcm16le",
    "model-input.f32le",
    "emissions.f32le",
    "ctc-path.i32le",
    "ctc-logprobs.f32le",
];
const INLINE: [&str; 3] = ["emissions.f32le", "ctc-path.i32le", "ctc-logprobs.f32le"];
const LABELS: [&str; 29] = [
    "-", "|", "E", "T", "A", "O", "N", "I", "H", "S", "R", "D", "L", "U", "M", "W", "C", "F", "G",
    "Y", "P", "B", "V", "K", "'", "X", "J", "Q", "Z",
];

pub(crate) struct ValidatedRequest {
    pub(crate) file_name: String,
    pub(crate) value: Value,
    pub(crate) bytes: Vec<u8>,
    pub(crate) sha256: String,
    pub(crate) frames: u64,
}
impl ValidatedRequest {
    pub(crate) fn parse(args: &Value) -> Result<Self> {
        closed(args, &["file_name", "request"])?;
        let file_name = string(&args["file_name"])?;
        if !valid_wav_name(file_name) {
            return Err(Error::invalid(
                "Alignment input must be a bounded WAV basename",
            ));
        }
        let value = &args["request"];
        closed(
            value,
            &[
                "schema",
                "request_id",
                "project_id",
                "project_generation",
                "project_revision",
                "voice_track_id",
                "asset_id",
                "source_commit",
                "language",
                "transcript",
                "source",
                "runtime",
            ],
        )?;
        equal(&value["schema"], &json!("motionwright.alignment-request/1"))?;
        equal(&value["language"], &json!("en"))?;
        for field in [
            "request_id",
            "project_id",
            "project_generation",
            "voice_track_id",
            "asset_id",
        ] {
            if !canonical_uuid(string(&value[field])?) {
                return Err(Error::invalid(
                    "Alignment identity must be a canonical UUID",
                ));
            }
        }
        bounded_integer(&value["project_revision"], 0, MAX_SAFE_INTEGER)?;
        if !lower_hex(string(&value["source_commit"])?, 40) {
            return Err(Error::invalid(
                "Alignment source commit is not a pinned Git object",
            ));
        }
        let transcript = &value["transcript"];
        closed(transcript, &["version", "text", "sha256"])?;
        bounded_integer(&transcript["version"], 1, MAX_SAFE_INTEGER)?;
        let text = string(&transcript["text"])?;
        if !valid_transcript(text) || string(&transcript["sha256"])? != sha(text.as_bytes()) {
            return Err(Error::invalid(
                "Alignment transcript violates the exact ASCII profile",
            ));
        }
        let source = &value["source"];
        closed(
            source,
            &[
                "sha256",
                "size_bytes",
                "sample_rate_hz",
                "channels",
                "sample_frames",
            ],
        )?;
        digest_string(&source["sha256"])?;
        bounded_integer(&source["size_bytes"], 1, MAX_WAV)?;
        equal(&source["sample_rate_hz"], &json!(16_000))?;
        equal(&source["channels"], &json!(1))?;
        let frames = bounded_integer(&source["sample_frames"], 400, 128_000)?;
        let mut previous = None;
        let mut minimum_ctc_frames = 0u64;
        for byte in text.bytes() {
            let token = if byte == b' ' {
                b'|'
            } else {
                byte.to_ascii_uppercase()
            };
            minimum_ctc_frames += 1 + u64::from(previous == Some(token));
            previous = Some(token);
        }
        let model_frames = (frames - 400) / 320 + 1;
        if minimum_ctc_frames > model_frames {
            return Err(Error::invalid(
                "Transcript cannot fit the fixed CTC frame grid",
            ));
        }
        equal(&value["runtime"], &runtime_pins())?;
        let mut bytes = Vec::new();
        canonical_request(value, &mut bytes)?;
        if bytes.len() > MAX_STDIN {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Alignment request exceeds stdin budget",
            ));
        }
        Ok(Self {
            file_name: file_name.to_owned(),
            value: value.clone(),
            sha256: sha(&bytes),
            bytes,
            frames,
        })
    }
    pub(crate) fn id(&self) -> &str {
        self.value["request_id"]
            .as_str()
            .expect("validated alignment UUID")
    }
    pub(crate) fn source_sha(&self) -> &str {
        self.value["source"]["sha256"]
            .as_str()
            .expect("validated source digest")
    }
    pub(crate) fn source_bytes(&self) -> u64 {
        self.value["source"]["size_bytes"]
            .as_u64()
            .expect("validated source length")
    }
    fn words(&self) -> usize {
        self.value["transcript"]["text"]
            .as_str()
            .expect("validated transcript")
            .split(' ')
            .count()
    }
}

pub(crate) fn runtime_pins() -> Value {
    json!({
        "contract_version":"acc007-en-ctc-1",
        "torch_version":"2.8.0+cpu",
        "torchaudio_version":"2.8.0+cpu",
        "numpy_version":"2.2.6",
        "model_id":"WAV2VEC2_ASR_BASE_960H",
        "checkpoint_sha256":CHECKPOINT_SHA,
        "checkpoint_size_bytes":377_664_473,
        "worker_sha256":WORKER_SHA,
        "contract_sha256":CONTRACT_SHA
    })
}
fn valid_wav_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        && !value.contains("..")
        && value.ends_with(".wav")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}
fn valid_transcript(value: &str) -> bool {
    if value.is_empty() || value.len() > 256 || !value.is_ascii() {
        return false;
    }
    let words: Vec<_> = value.split(' ').collect();
    if words.len() > 32 {
        return false;
    }
    words.iter().all(|word| {
        let bytes = word.as_bytes();
        !bytes.is_empty()
            && bytes[0].is_ascii_alphabetic()
            && bytes[bytes.len() - 1].is_ascii_alphabetic()
            && bytes.iter().enumerate().all(|(index, byte)| {
                byte.is_ascii_alphabetic()
                    || (*byte == b'\''
                        && index > 0
                        && index + 1 < bytes.len()
                        && bytes[index - 1].is_ascii_alphabetic()
                        && bytes[index + 1].is_ascii_alphabetic())
            })
    })
}
fn canonical_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}
fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn string(value: &Value) -> Result<&str> {
    value
        .as_str()
        .ok_or_else(|| Error::invalid("Alignment field must be a string"))
}
fn digest_string(value: &Value) -> Result<&str> {
    let value = string(value)?;
    if !lower_hex(value, 64) {
        return Err(Error::invalid("Alignment digest must be lowercase SHA-256"));
    }
    Ok(value)
}
fn closed<'a>(value: &'a Value, fields: &[&str]) -> Result<&'a Map<String, Value>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::invalid("Alignment object expected"))?;
    if object.len() != fields.len() || fields.iter().any(|field| !object.contains_key(*field)) {
        return Err(Error::invalid(
            "Alignment object contains missing or unknown fields",
        ));
    }
    Ok(object)
}
fn equal(actual: &Value, expected: &Value) -> Result<()> {
    if actual != expected {
        return Err(Error::invalid(
            "Alignment value does not match the fixed contract",
        ));
    }
    Ok(())
}
fn bounded_integer(value: &Value, minimum: u64, maximum: u64) -> Result<u64> {
    value
        .as_u64()
        .filter(|v| *v >= minimum && *v <= maximum)
        .ok_or_else(|| Error::invalid("Alignment integer exceeds its exact bounds"))
}
fn probability(value: &Value) -> Result<f64> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
        .ok_or_else(|| Error::invalid("Alignment emission probability is invalid"))
}

fn canonical_request(value: &Value, output: &mut Vec<u8>) -> Result<()> {
    match value {
        Value::Object(object) => {
            output.push(b'{');
            let mut fields: Vec<_> = object.iter().collect();
            fields.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
            for (index, (key, value)) in fields.into_iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                serde_json::to_writer(&mut *output, key)?;
                output.push(b':');
                canonical_request(value, output)?;
            }
            output.push(b'}');
        }
        Value::String(value) => serde_json::to_writer(output, value)?,
        Value::Number(value) if value.as_u64().is_some() => serde_json::to_writer(output, value)?,
        _ => {
            return Err(Error::invalid(
                "Alignment request has an unsupported JSON value",
            ));
        }
    }
    Ok(())
}

pub(crate) struct SourceObservation {
    pub(crate) frames: u64,
    pcm_sha: String,
    model_sha: String,
}
pub(crate) fn observe_pcm(path: &Path, request: &ValidatedRequest) -> Result<SourceObservation> {
    let mut reader = WaveReader::open(File::open(path)?, MAX_WAV)
        .map_err(|_| Error::invalid("Alignment WAV could not be decoded"))?;
    let info = reader.info();
    if info.sample_rate.0 != 16_000
        || info.channels != 1
        || info.container_bits != 16
        || info.valid_bits != 16
        || info.ieee_float
        || info.frames != request.frames
    {
        return Err(Error::invalid(
            "Alignment requires exact PCM16 mono 16000 Hz WAV",
        ));
    }
    let mut pcm = Sha256::new();
    let mut model = Sha256::new();
    let mut count = 0u64;
    loop {
        let values = reader
            .read_frames(16_384)
            .map_err(|_| Error::invalid("Alignment WAV PCM read failed"))?;
        if values.is_empty() {
            break;
        }
        count = count
            .checked_add(values.len() as u64)
            .filter(|v| *v <= request.frames)
            .ok_or_else(|| Error::invalid("Alignment WAV frame count exceeds request"))?;
        for value in values {
            let scaled = value * 32_768.0;
            if !scaled.is_finite()
                || scaled.fract() != 0.0
                || !(f64::from(i16::MIN)..=f64::from(i16::MAX)).contains(&scaled)
            {
                return Err(Error::invalid("Alignment WAV is not exact signed PCM16"));
            }
            let sample = scaled as i16;
            pcm.update(sample.to_le_bytes());
            model.update((f32::from(sample) / 32_768.0).to_le_bytes());
        }
    }
    if count != request.frames {
        return Err(Error::invalid(
            "Alignment WAV ended before its declared frame count",
        ));
    }
    Ok(SourceObservation {
        frames: count,
        pcm_sha: hex::encode(pcm.finalize()),
        model_sha: hex::encode(model.finalize()),
    })
}

struct StrictJson(Value);
impl<'de> Deserialize<'de> for StrictJson {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct JsonVisitor;
        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = StrictJson;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JSON with unique object keys and finite numbers")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(json!(value)))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(json!(value)))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(json!(value)))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> std::result::Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|n| StrictJson(Value::Number(n)))
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(Value::String(value.to_owned())))
            }
            fn visit_string<E: de::Error>(
                self,
                value: String,
            ) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(Value::String(value)))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictJson(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(StrictJson(value)) = sequence.next_element()? {
                    values.push(value);
                }
                Ok(StrictJson(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut access: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some((key, StrictJson(value))) =
                    access.next_entry::<String, StrictJson>()?
                {
                    if values.insert(key, value).is_some() {
                        return Err(de::Error::custom("duplicate JSON object key"));
                    }
                }
                Ok(StrictJson(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(JsonVisitor)
    }
}
fn strict_json(bytes: &[u8]) -> Result<Value> {
    let mut parser = serde_json::Deserializer::from_slice(bytes);
    let StrictJson(value) = StrictJson::deserialize(&mut parser)?;
    parser.end()?;
    Ok(value)
}

fn decode_blob(metadata: &Value, maximum: usize, original_name: Option<&str>) -> Result<Vec<u8>> {
    if let Some(name) = original_name {
        closed(
            metadata,
            &["name", "encoding", "sha256", "size_bytes", "data"],
        )?;
        equal(&metadata["name"], &json!(name))?;
    } else {
        closed(metadata, &["encoding", "sha256", "size_bytes", "data"])?;
    }
    equal(&metadata["encoding"], &json!("base64"))?;
    let declared = bounded_integer(&metadata["size_bytes"], 1, maximum as u64)? as usize;
    let expected_sha = digest_string(&metadata["sha256"])?;
    let encoded = string(&metadata["data"])?;
    if encoded.len() > maximum.div_ceil(3) * 4 {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Alignment base64 exceeds bounded allocation",
        ));
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| Error::invalid("Alignment base64 is malformed"))?;
    if bytes.len() != declared || STANDARD.encode(&bytes) != encoded || sha(&bytes) != expected_sha
    {
        return Err(Error::invalid(
            "Alignment original bytes failed exact length/digest/encoding binding",
        ));
    }
    Ok(bytes)
}
fn original_json(metadata: &Value, name: &str, maximum: usize, embedded: &Value) -> Result<Value> {
    let bytes = decode_blob(metadata, maximum, Some(name))?;
    if bytes.last() != Some(&b'\n') {
        return Err(Error::invalid("Alignment original JSON lacks its final LF"));
    }
    let value = strict_json(&bytes)?;
    equal(&value, embedded)?;
    Ok(value)
}

fn artifact(metadata: &Value, expected_size: u64, expected_sha: Option<&str>) -> Result<()> {
    closed(metadata, &["sha256", "size_bytes"])?;
    let digest = digest_string(&metadata["sha256"])?;
    equal(&metadata["size_bytes"], &json!(expected_size))?;
    if expected_sha.is_some_and(|expected| digest != expected) {
        return Err(Error::invalid(
            "Alignment artifact differs from independent source reconstruction",
        ));
    }
    Ok(())
}
fn validate_arrays(envelope: &Value, observation: &SourceObservation) -> Result<()> {
    let retained = &envelope["retained_artifacts"];
    let inline = &envelope["inline_artifacts"];
    closed(retained, &ARTIFACTS)?;
    closed(inline, &INLINE)?;
    artifact(
        &retained[ARTIFACTS[0]],
        observation.frames * 2,
        Some(&observation.pcm_sha),
    )?;
    artifact(
        &retained[ARTIFACTS[1]],
        observation.frames * 4,
        Some(&observation.model_sha),
    )?;
    let rows = ((observation.frames - 400) / 320 + 1) as usize;
    let lengths = [rows * 29 * 4, rows * 4, rows * 4];
    let mut decoded = Vec::new();
    for (name, length) in INLINE.into_iter().zip(lengths) {
        artifact(&retained[name], length as u64, None)?;
        let bytes = decode_blob(&inline[name], length, None)?;
        if bytes.len() != length {
            return Err(Error::invalid(
                "Alignment array shape differs from the fixed frame clock",
            ));
        }
        equal(&inline[name]["sha256"], &retained[name]["sha256"])?;
        equal(&inline[name]["size_bytes"], &retained[name]["size_bytes"])?;
        decoded.push(bytes);
    }
    let emissions: Vec<f32> = decoded[0]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect();
    let path: Vec<i32> = decoded[1]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| i32::from_le_bytes(*bytes))
        .collect();
    let scores: Vec<f32> = decoded[2]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect();
    for row in 0..rows {
        let probabilities = &emissions[row * 29..(row + 1) * 29];
        if probabilities
            .iter()
            .any(|value| !value.is_finite() || *value > 1e-5)
            || !(0..29).contains(&path[row])
            || !scores[row].is_finite()
        {
            return Err(Error::invalid(
                "Alignment model arrays contain invalid values",
            ));
        }
        let total: f64 = probabilities
            .iter()
            .map(|value| f64::from(*value).exp())
            .sum();
        if (total - 1.0).abs() > 2e-4
            || (f64::from(scores[row]) - f64::from(probabilities[path[row] as usize])).abs() > 2e-6
        {
            return Err(Error::invalid(
                "Alignment model arrays disagree numerically",
            ));
        }
    }
    Ok(())
}
fn validate_candidate(
    candidate: &Value,
    request: &ValidatedRequest,
    retained: &Value,
) -> Result<()> {
    closed(
        candidate,
        &[
            "schema",
            "contract_version",
            "authority",
            "backend_authority",
            "request_sha256",
            "binding",
            "runtime",
            "source",
            "preprocessing",
            "frame_clock",
            "labels",
            "greedy",
            "token_spans",
            "words",
            "unresolved_word_count",
            "word_policy",
            "artifacts",
        ],
    )?;
    equal(
        &candidate["schema"],
        &json!("motionwright.alignment-candidate/1"),
    )?;
    equal(&candidate["contract_version"], &json!("acc007-en-ctc-1"))?;
    equal(&candidate["authority"], &json!("CANDIDATE_ONLY"))?;
    equal(&candidate["backend_authority"], &json!(false))?;
    equal(&candidate["request_sha256"], &json!(request.sha256))?;
    let input = &request.value;
    equal(
        &candidate["binding"],
        &json!({
            "request_id":input["request_id"],
            "project_id":input["project_id"],
            "project_generation":input["project_generation"],
            "project_revision":input["project_revision"],
            "voice_track_id":input["voice_track_id"],
            "asset_id":input["asset_id"],
            "source_commit":input["source_commit"],
            "source_sha256":input["source"]["sha256"],
            "transcript_version":input["transcript"]["version"],
            "transcript_sha256":input["transcript"]["sha256"]
        }),
    )?;
    equal(&candidate["runtime"], &input["runtime"])?;
    equal(&candidate["source"], &input["source"])?;
    equal(&candidate["artifacts"], retained)?;
    equal(
        &candidate["preprocessing"],
        &json!({
            "pcm_format":"signed-pcm16le",
            "normalization":"sample/32768.0",
            "downmix":"none",
            "resampling":null,
            "model_input_sample_rate_hz":16_000,
            "model_input_sample_frames":request.frames,
            "stdlib_recomputed_exact_source_to_model_input":true
        }),
    )?;
    let rows = (request.frames - 400) / 320 + 1;
    equal(
        &candidate["frame_clock"],
        &json!({
            "frame_count":rows,"class_count":29,"blank_id":0,
            "stride_samples":320,"receptive_samples":400,"sample_rate_hz":16_000,
            "temporal_resolution":{"numerator":1,"denominator":50},
            "receptive_window":{"numerator":1,"denominator":40},
            "interval_convention":"half_open_frame_grid",
            "phonetic_boundary_error":"uncalibrated","precision_claim":"none"
        }),
    )?;
    equal(&candidate["labels"], &json!(LABELS))?;
    equal(
        &candidate["word_policy"],
        &json!({
            "requires_whole_transcript_greedy_match":true,
            "minimum_mean_model_emission_probability":0.80,
            "probability_is_calibrated_confidence":false,
            "unknown_word_interval":null,
            "promotion_to_domain_measured":"forbidden_without_qualified_receiver"
        }),
    )?;
    let greedy = &candidate["greedy"];
    closed(
        greedy,
        &[
            "token_ids",
            "raw_text",
            "text",
            "exact_corrected_transcript_match",
        ],
    )?;
    let greedy_tokens = greedy["token_ids"]
        .as_array()
        .filter(|values| values.len() <= rows as usize)
        .ok_or_else(|| Error::invalid("Alignment greedy tokens exceed the frame budget"))?;
    for token in greedy_tokens {
        bounded_integer(token, 0, 28)?;
    }
    for field in ["raw_text", "text"] {
        let value = string(&greedy[field])?;
        if !value.is_ascii() || value.len() > rows as usize * 2 {
            return Err(Error::invalid(
                "Alignment greedy text exceeds its bounded model output",
            ));
        }
    }
    if !greedy["exact_corrected_transcript_match"].is_boolean() {
        return Err(Error::invalid("Alignment greedy agreement must be boolean"));
    }
    let spans = candidate["token_spans"]
        .as_array()
        .filter(|values| values.len() <= 256)
        .ok_or_else(|| Error::invalid("Alignment token spans exceed transcript bounds"))?;
    for (index, span) in spans.iter().enumerate() {
        closed(
            span,
            &[
                "index",
                "token_id",
                "token",
                "start_frame",
                "end_frame_exclusive",
                "mean_emission_probability",
            ],
        )?;
        equal(&span["index"], &json!(index))?;
        let token = bounded_integer(&span["token_id"], 1, 28)? as usize;
        equal(&span["token"], &json!(LABELS[token]))?;
        let start = bounded_integer(&span["start_frame"], 0, rows - 1)?;
        bounded_integer(&span["end_frame_exclusive"], start + 1, rows)?;
        probability(&span["mean_emission_probability"])?;
    }
    let words = candidate["words"]
        .as_array()
        .filter(|values| values.len() == request.words())
        .ok_or_else(|| Error::invalid("Alignment candidate word count differs from transcript"))?;
    let mut unknown = 0u64;
    for (index, word) in words.iter().enumerate() {
        closed(
            word,
            &[
                "index",
                "text",
                "token_start_index",
                "token_end_index_exclusive",
                "status",
                "reasons",
                "interval",
                "mean_emission_probability",
            ],
        )?;
        equal(&word["index"], &json!(index))?;
        if !valid_transcript(string(&word["text"])?) || string(&word["text"])?.contains(' ') {
            return Err(Error::invalid(
                "Alignment candidate contains an invalid word",
            ));
        }
        let start = bounded_integer(&word["token_start_index"], 0, spans.len() as u64)?;
        bounded_integer(
            &word["token_end_index_exclusive"],
            start + 1,
            spans.len() as u64,
        )?;
        probability(&word["mean_emission_probability"])?;
        let reasons = word["reasons"]
            .as_array()
            .filter(|values| values.len() <= 2)
            .ok_or_else(|| Error::invalid("Alignment word reasons exceed the fixed policy"))?;
        for reason in reasons {
            if !matches!(
                string(reason)?,
                "whole_transcript_greedy_disagreement" | "low_model_emission_probability"
            ) {
                return Err(Error::invalid(
                    "Alignment candidate has an unknown uncertainty reason",
                ));
            }
        }
        match string(&word["status"])? {
            "unknown" => {
                unknown += 1;
                equal(&word["interval"], &Value::Null)?;
                if reasons.is_empty() {
                    return Err(Error::invalid("Unknown alignment word requires a reason"));
                }
            }
            "aligned_candidate" => {
                if !reasons.is_empty() {
                    return Err(Error::invalid(
                        "Aligned candidate must not hide uncertainty",
                    ));
                }
                let interval = &word["interval"];
                closed(
                    interval,
                    &[
                        "start",
                        "end",
                        "source_start_sample",
                        "source_end_sample_exclusive",
                        "receptive_support_start_sample",
                        "receptive_support_end_sample_exclusive",
                    ],
                )?;
                for field in ["start", "end"] {
                    closed(&interval[field], &["numerator", "denominator"])?;
                    bounded_integer(&interval[field]["numerator"], 0, 128_000)?;
                    bounded_integer(&interval[field]["denominator"], 1, 16_000)?;
                }
                let first = bounded_integer(&interval["source_start_sample"], 0, request.frames)?;
                bounded_integer(
                    &interval["source_end_sample_exclusive"],
                    first + 1,
                    request.frames,
                )?;
                let support = bounded_integer(
                    &interval["receptive_support_start_sample"],
                    0,
                    request.frames,
                )?;
                bounded_integer(
                    &interval["receptive_support_end_sample_exclusive"],
                    support + 1,
                    request.frames,
                )?;
            }
            _ => {
                return Err(Error::invalid(
                    "Alignment candidate status is not part of this wire contract",
                ));
            }
        }
    }
    equal(&candidate["unresolved_word_count"], &json!(unknown))?;
    Ok(())
}
fn bounded_ascii(value: &Value, maximum: usize) -> Result<&str> {
    let value = string(value)?;
    if value.is_empty()
        || value.len() > maximum
        || !value.is_ascii()
        || value.bytes().any(|b| b.is_ascii_control())
    {
        return Err(Error::invalid(
            "Alignment metadata exceeds its bounded ASCII representation",
        ));
    }
    Ok(value)
}
fn validate_execution(execution: &Value, request: &ValidatedRequest) -> Result<()> {
    closed(
        execution,
        &[
            "schema",
            "authority",
            "backend_authority",
            "request_sha256",
            "started_at_utc",
            "finished_at_utc",
            "python",
            "system",
            "machine",
            "observed_numpy_version",
            "observed_torch_version",
            "observed_torchaudio_version",
            "device",
            "inference_threads",
            "interop_threads",
            "deterministic_algorithms",
            "network_allowed",
            "checkpoint_layout",
            "weights_only_load",
            "checkpoint_sha256",
            "self_report_is_native_attestation",
            "qualified_receipt_required",
        ],
    )?;
    for (field, expected) in [
        ("schema", json!("motionwright.alignment-worker-execution/1")),
        ("authority", json!("CANDIDATE_ONLY")),
        ("backend_authority", json!(false)),
        ("request_sha256", json!(request.sha256)),
        ("system", json!("Linux")),
        ("machine", json!("x86_64")),
        ("observed_numpy_version", json!("2.2.6")),
        ("observed_torch_version", json!("2.8.0+cpu")),
        ("observed_torchaudio_version", json!("2.8.0+cpu")),
        ("device", json!("cpu")),
        ("inference_threads", json!(2)),
        ("interop_threads", json!(1)),
        ("deterministic_algorithms", json!(true)),
        ("network_allowed", json!(false)),
        ("weights_only_load", json!(true)),
        ("checkpoint_sha256", json!(CHECKPOINT_SHA)),
        ("self_report_is_native_attestation", json!(false)),
        ("qualified_receipt_required", json!(true)),
    ] {
        equal(&execution[field], &expected)?;
    }
    let python = bounded_ascii(&execution["python"], 32)?;
    if !python
        .strip_prefix("3.12.")
        .is_some_and(|patch| !patch.is_empty() && patch.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(Error::invalid(
            "Alignment execution reports another Python profile",
        ));
    }
    bounded_ascii(&execution["started_at_utc"], 64)?;
    bounded_ascii(&execution["finished_at_utc"], 64)?;
    if !matches!(
        string(&execution["checkpoint_layout"])?,
        "official_32_class_remove_axes_1_2_3" | "already_normalized_29_class"
    ) {
        return Err(Error::invalid(
            "Alignment checkpoint layout is not in the fixed profile",
        ));
    }
    Ok(())
}

pub(crate) fn validate_envelope(
    bytes: &[u8],
    request: &ValidatedRequest,
    observation: &SourceObservation,
) -> Result<Value> {
    if bytes.is_empty() || bytes.len() > MAX_STDOUT || bytes.last() != Some(&b'\n') {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Alignment runtime envelope exceeds its bounded framing",
        ));
    }
    let envelope = strict_json(bytes)?;
    closed(
        &envelope,
        &[
            "schema",
            "profile_id",
            "status",
            "backend_authority",
            "native_execution_attested",
            "human_acceptance",
            "qualified_ingress",
            "request_sha256",
            "producer",
            "candidate",
            "candidate_file",
            "execution",
            "execution_file",
            "worker_summary",
            "retained_artifacts",
            "inline_artifacts",
            "reconstruction",
            "validation",
        ],
    )?;
    for (field, expected) in [
        ("schema", json!("motionwright.alignment-runtime-envelope/2")),
        ("profile_id", json!(PROFILE)),
        ("status", json!("CANDIDATE_ONLY")),
        ("backend_authority", json!(false)),
        ("native_execution_attested", json!(false)),
        ("human_acceptance", json!(false)),
        ("qualified_ingress", json!("NOT_EXECUTED")),
        ("request_sha256", json!(request.sha256)),
    ] {
        equal(&envelope[field], &expected)?;
    }
    equal(
        &envelope["producer"],
        &json!({
            "bridge_sha256":BRIDGE_SHA,"worker_sha256":WORKER_SHA,
            "contract_sha256":CONTRACT_SHA,"checkpoint_sha256":CHECKPOINT_SHA
        }),
    )?;
    let candidate = original_json(
        &envelope["candidate_file"],
        "result.json",
        32_768,
        &envelope["candidate"],
    )?;
    let execution = original_json(
        &envelope["execution_file"],
        "execution.json",
        8_192,
        &envelope["execution"],
    )?;
    validate_candidate(&candidate, request, &envelope["retained_artifacts"])?;
    validate_execution(&execution, request)?;
    validate_arrays(&envelope, observation)?;
    equal(
        &envelope["reconstruction"],
        &json!({
            "method":"pcm16le-mono-16000-divide-32768-to-f32le/1",
            "source_sha256":request.source_sha(),"sample_frames":request.frames,"exact":true
        }),
    )?;
    equal(
        &envelope["worker_summary"],
        &json!({
            "status":"CANDIDATE_ONLY","backend_authority":false,
            "request_id":request.id(),"request_sha256":request.sha256,
            "result_sha256":envelope["candidate_file"]["sha256"],
            "execution_sha256":envelope["execution_file"]["sha256"],
            "word_count":request.words(),
            "unresolved_word_count":candidate["unresolved_word_count"],
            "stdlib_recomputed_exact_source_to_model_input":true,
            "qualified_ingress":"NOT_EXECUTED","human_acceptance":"NOT_DECLARED"
        }),
    )?;
    equal(
        &envelope["validation"],
        &json!({
            "status":"CANDIDATE_ONLY","backend_authority":false,"request_sha256":request.sha256,
            "unresolved_word_count":candidate["unresolved_word_count"],
            "native_execution_attested":false,"human_acceptance":false,
            "stdlib_recomputed_exact_source_to_model_input":true
        }),
    )?;
    Ok(envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn args() -> Value {
        let text = "A";
        json!({
            "file_name":"voice.wav",
            "request":{
                "schema":"motionwright.alignment-request/1",
                "request_id":"00000000-0000-0000-0000-000000000001",
                "project_id":"00000000-0000-0000-0000-000000000002",
                "project_generation":"00000000-0000-0000-0000-000000000003",
                "project_revision":1,
                "voice_track_id":"00000000-0000-0000-0000-000000000004",
                "asset_id":"00000000-0000-0000-0000-000000000005",
                "source_commit":"ab".repeat(20),
                "language":"en",
                "transcript":{"version":1,"text":text,"sha256":sha(text.as_bytes())},
                "source":{"sha256":"cd".repeat(32),"size_bytes":844,
                    "sample_rate_hz":16000,"channels":1,"sample_frames":400},
                "runtime":runtime_pins()
            }
        })
    }
    fn blob(bytes: &[u8], name: Option<&str>) -> Value {
        let mut value = json!({
            "encoding":"base64","sha256":sha(bytes),"size_bytes":bytes.len(),
            "data":STANDARD.encode(bytes)
        });
        if let Some(name) = name {
            value["name"] = json!(name);
        }
        value
    }
    fn original(value: &Value, name: &str) -> Value {
        let mut bytes = serde_json::to_vec(value).unwrap();
        bytes.push(b'\n');
        blob(&bytes, Some(name))
    }
    fn observation() -> SourceObservation {
        SourceObservation {
            frames: 400,
            pcm_sha: sha(&[0; 800]),
            model_sha: sha(&[0; 1600]),
        }
    }
    fn arrays(observed: &SourceObservation) -> Value {
        let mut emission = Vec::new();
        for index in 0..29 {
            let probability = if index == 4 { 0.9f32 } else { 0.1f32 / 28.0 };
            emission.extend_from_slice(&probability.ln().to_le_bytes());
        }
        let path = 4i32.to_le_bytes();
        let score = 0.9f32.ln().to_le_bytes();
        let mut retained = json!({
            "source.pcm16le":{"sha256":observed.pcm_sha,"size_bytes":800},
            "model-input.f32le":{"sha256":observed.model_sha,"size_bytes":1600}
        });
        let mut inline = json!({});
        for (name, bytes) in
            INLINE
                .into_iter()
                .zip([emission.as_slice(), path.as_slice(), score.as_slice()])
        {
            retained[name] = json!({"sha256":sha(bytes),"size_bytes":bytes.len()});
            inline[name] = blob(bytes, None);
        }
        json!({"retained_artifacts":retained,"inline_artifacts":inline})
    }
    fn fixture() -> (ValidatedRequest, SourceObservation, Value) {
        let request = ValidatedRequest::parse(&args()).unwrap();
        let observed = observation();
        let arrays = arrays(&observed);
        let input = &request.value;
        let candidate = json!({
            "schema":"motionwright.alignment-candidate/1",
            "contract_version":"acc007-en-ctc-1","authority":"CANDIDATE_ONLY",
            "backend_authority":false,"request_sha256":request.sha256,
            "binding":{
                "request_id":input["request_id"],"project_id":input["project_id"],
                "project_generation":input["project_generation"],"project_revision":1,
                "voice_track_id":input["voice_track_id"],"asset_id":input["asset_id"],
                "source_commit":input["source_commit"],"source_sha256":input["source"]["sha256"],
                "transcript_version":1,"transcript_sha256":input["transcript"]["sha256"]
            },
            "runtime":input["runtime"],"source":input["source"],
            "preprocessing":{"pcm_format":"signed-pcm16le","normalization":"sample/32768.0",
                "downmix":"none","resampling":null,"model_input_sample_rate_hz":16000,
                "model_input_sample_frames":400,"stdlib_recomputed_exact_source_to_model_input":true},
            "frame_clock":{"frame_count":1,"class_count":29,"blank_id":0,"stride_samples":320,
                "receptive_samples":400,"sample_rate_hz":16000,
                "temporal_resolution":{"numerator":1,"denominator":50},
                "receptive_window":{"numerator":1,"denominator":40},
                "interval_convention":"half_open_frame_grid","phonetic_boundary_error":"uncalibrated",
                "precision_claim":"none"},
            "labels":LABELS,
            "greedy":{"token_ids":[4],"raw_text":"A","text":"A","exact_corrected_transcript_match":true},
            "token_spans":[{"index":0,"token_id":4,"token":"A","start_frame":0,
                "end_frame_exclusive":1,"mean_emission_probability":0.9}],
            "words":[{"index":0,"text":"A","token_start_index":0,"token_end_index_exclusive":1,
                "status":"aligned_candidate","reasons":[],"mean_emission_probability":0.9,
                "interval":{"start":{"numerator":0,"denominator":1},
                    "end":{"numerator":1,"denominator":50},"source_start_sample":0,
                    "source_end_sample_exclusive":320,"receptive_support_start_sample":0,
                    "receptive_support_end_sample_exclusive":400}}],
            "unresolved_word_count":0,
            "word_policy":{"requires_whole_transcript_greedy_match":true,
                "minimum_mean_model_emission_probability":0.8,
                "probability_is_calibrated_confidence":false,"unknown_word_interval":null,
                "promotion_to_domain_measured":"forbidden_without_qualified_receiver"},
            "artifacts":arrays["retained_artifacts"]
        });
        let execution = json!({
            "schema":"motionwright.alignment-worker-execution/1","authority":"CANDIDATE_ONLY",
            "backend_authority":false,"request_sha256":request.sha256,
            "started_at_utc":"2026-10-24T00:00:00+00:00",
            "finished_at_utc":"2026-10-24T00:00:01+00:00",
            "python":"3.12.11","system":"Linux","machine":"x86_64",
            "observed_numpy_version":"2.2.6","observed_torch_version":"2.8.0+cpu",
            "observed_torchaudio_version":"2.8.0+cpu","device":"cpu","inference_threads":2,
            "interop_threads":1,"deterministic_algorithms":true,"network_allowed":false,
            "checkpoint_layout":"official_32_class_remove_axes_1_2_3","weights_only_load":true,
            "checkpoint_sha256":CHECKPOINT_SHA,"self_report_is_native_attestation":false,
            "qualified_receipt_required":true
        });
        let candidate_file = original(&candidate, "result.json");
        let execution_file = original(&execution, "execution.json");
        let envelope = json!({
            "schema":"motionwright.alignment-runtime-envelope/2","profile_id":PROFILE,
            "status":"CANDIDATE_ONLY","backend_authority":false,"native_execution_attested":false,
            "human_acceptance":false,"qualified_ingress":"NOT_EXECUTED","request_sha256":request.sha256,
            "producer":{"bridge_sha256":BRIDGE_SHA,"worker_sha256":WORKER_SHA,
                "contract_sha256":CONTRACT_SHA,"checkpoint_sha256":CHECKPOINT_SHA},
            "candidate":candidate,"candidate_file":candidate_file,
            "execution":execution,"execution_file":execution_file,
            "worker_summary":{"status":"CANDIDATE_ONLY","backend_authority":false,
                "request_id":request.id(),"request_sha256":request.sha256,
                "result_sha256":candidate_file["sha256"],"execution_sha256":execution_file["sha256"],
                "word_count":1,"unresolved_word_count":0,
                "stdlib_recomputed_exact_source_to_model_input":true,
                "qualified_ingress":"NOT_EXECUTED","human_acceptance":"NOT_DECLARED"},
            "retained_artifacts":arrays["retained_artifacts"],"inline_artifacts":arrays["inline_artifacts"],
            "reconstruction":{"method":"pcm16le-mono-16000-divide-32768-to-f32le/1",
                "source_sha256":request.source_sha(),"sample_frames":400,"exact":true},
            "validation":{"status":"CANDIDATE_ONLY","backend_authority":false,
                "request_sha256":request.sha256,"unresolved_word_count":0,
                "native_execution_attested":false,"human_acceptance":false,
                "stdlib_recomputed_exact_source_to_model_input":true}
        });
        (request, observed, envelope)
    }
    fn encoded(value: &Value) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(value).unwrap();
        bytes.push(b'\n');
        bytes
    }
    #[test]
    fn synthetic_candidate_remains_without_authority() {
        let (request, observed, envelope) = fixture();
        let result = validate_envelope(&encoded(&envelope), &request, &observed).unwrap();
        assert_eq!(result["status"], "CANDIDATE_ONLY");
        assert_eq!(result["native_execution_attested"], false);
        assert_eq!(result["qualified_ingress"], "NOT_EXECUTED");
    }
    #[test]
    fn request_rejects_paths_flac_and_unknown_fields() {
        for name in [
            "../voice.wav",
            "a/b.wav",
            r"a\b.wav",
            ".voice.wav",
            "a..wav",
            "voice.flac",
            "--bad.wav",
        ] {
            let mut value = args();
            value["file_name"] = json!(name);
            assert!(ValidatedRequest::parse(&value).is_err());
        }
        let mut value = args();
        value["expected_sha256"] = json!("cd".repeat(32));
        assert!(ValidatedRequest::parse(&value).is_err());
    }
    #[test]
    fn request_rejects_float_boolean_identity_and_runtime_substitution() {
        for revision in [
            json!(true),
            json!(1.0),
            json!(-1),
            json!(MAX_SAFE_INTEGER + 1),
        ] {
            let mut value = args();
            value["request"]["project_revision"] = revision;
            assert!(ValidatedRequest::parse(&value).is_err());
        }
        let mut value = args();
        value["request"]["project_generation"] = json!(1);
        assert!(ValidatedRequest::parse(&value).is_err());
        let mut value = args();
        value["request"]["runtime"]["worker_sha256"] = json!("00".repeat(32));
        assert!(ValidatedRequest::parse(&value).is_err());
    }
    #[test]
    fn request_rejects_extended_duration_rate_channels_and_size() {
        for (field, replacement) in [
            ("sample_frames", json!(399)),
            ("sample_frames", json!(128001)),
            ("sample_rate_hz", json!(48000)),
            ("channels", json!(2)),
            ("size_bytes", json!(1048577)),
        ] {
            let mut value = args();
            value["request"]["source"][field] = replacement;
            assert!(ValidatedRequest::parse(&value).is_err());
        }
    }
    #[test]
    fn exact_transcript_bytes_are_bound_without_normalization() {
        for text in [" A", "A ", "A  B", "A\nB", "é", "A''B", "A1", "A-BC"] {
            let mut value = args();
            value["request"]["transcript"]["text"] = json!(text);
            value["request"]["transcript"]["sha256"] = json!(sha(text.as_bytes()));
            assert!(ValidatedRequest::parse(&value).is_err());
        }
        let mut value = args();
        value["request"]["transcript"]["text"] = json!("Don't Stop");
        value["request"]["transcript"]["sha256"] = json!(sha(b"Don't Stop"));
        value["request"]["source"]["sample_frames"] = json!(3280);
        value["request"]["source"]["size_bytes"] = json!(6604);
        assert!(ValidatedRequest::parse(&value).is_ok());
        value["request"]["transcript"]["sha256"] = json!(sha(b"DON'T STOP"));
        assert!(ValidatedRequest::parse(&value).is_err());
    }
    #[test]
    fn transcript_word_and_byte_limits_are_exact() {
        for text in [["A"; 33].join(" "), "A".repeat(257)] {
            let mut value = args();
            value["request"]["transcript"]["text"] = json!(text);
            value["request"]["transcript"]["sha256"] = json!(sha(text.as_bytes()));
            assert!(ValidatedRequest::parse(&value).is_err());
        }
    }
    #[test]
    fn request_canonicalizer_sorts_and_rejects_float_representation() {
        let mut out = Vec::new();
        canonical_request(&json!({"z":2,"a":{"v":"A","i":1}}), &mut out).unwrap();
        assert_eq!(out, br#"{"a":{"i":1,"v":"A"},"z":2}"#);
        assert!(canonical_request(&json!(1.0), &mut Vec::new()).is_err());
    }
    #[test]
    fn strict_json_rejects_nested_duplicate_fields_and_trailing_data() {
        assert!(strict_json(br#"{"x":{"a":1,"a":2}}"#).is_err());
        assert!(strict_json(br#"{"x":1}{"x":2}"#).is_err());
        assert!(strict_json(br#"{"x":NaN}"#).is_err());
    }
    #[test]
    fn original_float_bytes_are_hashed_without_reserialization() {
        let bytes = b"{\"value\":0.1234000000000000}\n";
        let value: Value = serde_json::from_slice(bytes).unwrap();
        let metadata = blob(bytes, Some("result.json"));
        assert!(original_json(&metadata, "result.json", 32768, &value).is_ok());
        assert_ne!(sha(bytes), sha(&encoded(&value)));
        assert!(original_json(&metadata, "result.json", 32768, &json!({"value":0.9})).is_err());
    }
    #[test]
    fn original_files_reject_size_boundaries_and_changed_bytes() {
        for (name, maximum) in [("result.json", 32768), ("execution.json", 8192)] {
            let metadata = blob(&vec![b' '; maximum + 1], Some(name));
            assert!(original_json(&metadata, name, maximum, &json!({})).is_err());
        }
        let mut metadata = blob(b"{}\n", Some("result.json"));
        metadata["sha256"] = json!("00".repeat(32));
        assert!(original_json(&metadata, "result.json", 32768, &json!({})).is_err());
    }
    #[test]
    fn base64_rejects_noncanonical_spelling() {
        let mut metadata = blob(&[0], None);
        metadata["data"] = json!("AB==");
        assert!(decode_blob(&metadata, 1, None).is_err());
        metadata["data"] = json!("AA==\n");
        assert!(decode_blob(&metadata, 1, None).is_err());
    }
    #[test]
    fn envelope_rejects_old_version_and_authority_claims() {
        let (request, observed, envelope) = fixture();
        for (field, value) in [
            ("schema", json!("motionwright.alignment-runtime-envelope/1")),
            ("backend_authority", json!(true)),
            ("native_execution_attested", json!(true)),
            ("human_acceptance", json!(true)),
            ("qualified_ingress", json!("EXECUTED")),
        ] {
            let mut changed = envelope.clone();
            changed[field] = value;
            assert!(validate_envelope(&encoded(&changed), &request, &observed).is_err());
        }
    }
    #[test]
    fn envelope_rejects_extra_fields_and_stdout_overflow() {
        let (request, observed, mut envelope) = fixture();
        envelope["trusted"] = json!(true);
        assert!(validate_envelope(&encoded(&envelope), &request, &observed).is_err());
        let oversized = vec![b' '; MAX_STDOUT + 1];
        assert!(validate_envelope(&oversized, &request, &observed).is_err());
    }
    #[test]
    fn envelope_rejects_embedded_file_disagreement() {
        let (request, observed, mut envelope) = fixture();
        envelope["candidate"]["words"][0]["mean_emission_probability"] = json!(0.8);
        assert!(validate_envelope(&encoded(&envelope), &request, &observed).is_err());
    }
    #[test]
    fn arrays_reject_nan_even_when_digest_is_recomputed() {
        let observed = observation();
        let mut envelope = arrays(&observed);
        let mut bytes = STANDARD
            .decode(
                envelope["inline_artifacts"][INLINE[0]]["data"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
        bytes[..4].copy_from_slice(&f32::NAN.to_le_bytes());
        envelope["inline_artifacts"][INLINE[0]] = blob(&bytes, None);
        envelope["retained_artifacts"][INLINE[0]]["sha256"] = json!(sha(&bytes));
        assert!(validate_arrays(&envelope, &observed).is_err());
    }
    #[test]
    fn arrays_reject_forged_reconstructed_source_digest() {
        let observed = observation();
        let mut envelope = arrays(&observed);
        envelope["retained_artifacts"]["model-input.f32le"]["sha256"] = json!("00".repeat(32));
        assert!(validate_arrays(&envelope, &observed).is_err());
    }
    fn wav(samples: &[i16], channels: u16, rate: u32) -> Vec<u8> {
        let size = samples.len() as u32 * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + size).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&size.to_le_bytes());
        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }
    #[test]
    fn existing_wave_reader_reconstructs_extreme_pcm16_exactly() {
        let samples: Vec<i16> = [i16::MIN, -1, 0, i16::MAX]
            .into_iter()
            .cycle()
            .take(400)
            .collect();
        let bytes = wav(&samples, 1, 16000);
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&bytes).unwrap();
        let mut value = args();
        value["request"]["source"]["sha256"] = json!(sha(&bytes));
        let request = ValidatedRequest::parse(&value).unwrap();
        let observed = observe_pcm(file.path(), &request).unwrap();
        let raw: Vec<u8> = samples.iter().flat_map(|v| v.to_le_bytes()).collect();
        let model: Vec<u8> = samples
            .iter()
            .flat_map(|v| (f32::from(*v) / 32768.0).to_le_bytes())
            .collect();
        assert_eq!(observed.pcm_sha, sha(&raw));
        assert_eq!(observed.model_sha, sha(&model));
    }
    #[test]
    fn existing_wave_reader_rejects_real_stereo_even_if_request_claims_mono() {
        let bytes = wav(&[0; 800], 2, 16000);
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&bytes).unwrap();
        let mut value = args();
        value["request"]["source"]["sha256"] = json!(sha(&bytes));
        value["request"]["source"]["size_bytes"] = json!(bytes.len());
        let request = ValidatedRequest::parse(&value).unwrap();
        assert!(observe_pcm(file.path(), &request).is_err());
    }
    #[test]
    fn impossible_ctc_request_is_rejected_before_any_runtime_work() {
        let mut input = args();
        let text = "ABCDEFGHIJ";
        input["request"]["transcript"]["text"] = json!(text);
        input["request"]["transcript"]["sha256"] = json!(sha(text.as_bytes()));
        input["request"]["source"]["sample_frames"] = json!(2960);
        input["request"]["source"]["size_bytes"] = json!(5964);
        assert!(ValidatedRequest::parse(&input).is_err());
    }
    #[test]
    fn ctc_minimum_counts_repetitions_after_ascii_uppercase() {
        let mut input = args();
        let text = "Aa";
        input["request"]["transcript"]["text"] = json!(text);
        input["request"]["transcript"]["sha256"] = json!(sha(text.as_bytes()));
        input["request"]["source"]["sample_frames"] = json!(720);
        input["request"]["source"]["size_bytes"] = json!(1484);
        assert!(ValidatedRequest::parse(&input).is_err());
        input["request"]["source"]["sample_frames"] = json!(1040);
        input["request"]["source"]["size_bytes"] = json!(2124);
        assert!(ValidatedRequest::parse(&input).is_ok());
    }
}

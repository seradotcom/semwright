use semwright_types::{Error, ErrorCode, Result};
use std::{net::SocketAddr, time::Duration};
use tokio::{net::UdpSocket, time::timeout};

pub const DEFAULT_ARDOUR_OSC_PORT: u16 = 3819;
const MAX_PACKET: usize = 65_536;
const MAX_MESSAGES: usize = 2_048;
const MAX_ARGS: usize = 128;
const MAX_BUNDLE_DEPTH: usize = 4;

#[derive(Clone, Debug, PartialEq)]
pub enum OscArg {
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    String(String),
    Bool(bool),
    Nil,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OscMessage {
    pub address: String,
    pub args: Vec<OscArg>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArdourStrip {
    pub kind: String,
    pub name: String,
    pub inputs: u32,
    pub outputs: u32,
    pub muted: bool,
    pub soloed: bool,
    pub ssid: u32,
    pub record_enabled: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StripList {
    pub strips: Vec<ArdourStrip>,
    pub sample_rate: u32,
    pub last_frame: u64,
    pub monitor_present: bool,
}

pub struct OscClient {
    socket: UdpSocket,
    remote: SocketAddr,
}

impl OscClient {
    pub async fn loopback(port: u16) -> Result<Self> {
        let remote = SocketAddr::from(([127, 0, 0, 1], port));
        let socket = UdpSocket::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .map_err(|error| io_error("bind Ardour OSC loopback socket", error))?;
        socket
            .connect(remote)
            .await
            .map_err(|error| io_error("connect Ardour OSC loopback socket", error))?;
        Ok(Self { socket, remote })
    }

    pub fn remote(&self) -> SocketAddr {
        self.remote
    }

    pub async fn send(&self, address: &str, args: &[OscArg]) -> Result<()> {
        let packet = encode_message(address, args)?;
        self.socket
            .send(&packet)
            .await
            .map_err(|error| io_error("send Ardour OSC packet", error))?;
        Ok(())
    }

    pub async fn query_strip_list(&self) -> Result<StripList> {
        self.send("/strip/list", &[]).await?;
        timeout(Duration::from_secs(4), async {
            let mut strips = Vec::new();
            let mut messages_seen = 0usize;
            let mut buffer = vec![0u8; MAX_PACKET];
            loop {
                let size = self
                    .socket
                    .recv(&mut buffer)
                    .await
                    .map_err(|error| io_error("receive Ardour OSC reply", error))?;
                if size == 0 || size > MAX_PACKET {
                    return Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "Ardour OSC reply has invalid datagram size",
                    ));
                }
                for message in decode_packet(&buffer[..size])? {
                    messages_seen += 1;
                    if messages_seen > MAX_MESSAGES {
                        return Err(Error::new(
                            ErrorCode::ResourceExhausted,
                            "Ardour OSC strip query exceeded message budget",
                        ));
                    }
                    if let Some(end) = parse_end_route_list(&message)? {
                        if strips.len() > 1_024 {
                            return Err(Error::new(
                                ErrorCode::ResourceExhausted,
                                "Ardour OSC strip count exceeds semantic budget",
                            ));
                        }
                        return Ok(StripList {
                            strips,
                            sample_rate: end.0,
                            last_frame: end.1,
                            monitor_present: end.2,
                        });
                    }
                    if let Some(strip) = parse_strip(&message)? {
                        strips.push(strip);
                    }
                }
            }
        })
        .await
        .map_err(|_| Error::new(ErrorCode::Timeout, "Ardour OSC strip query timed out"))?
    }
}

pub fn encode_message(address: &str, args: &[OscArg]) -> Result<Vec<u8>> {
    validate_address(address)?;
    if args.len() > MAX_ARGS {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "OSC argument count exceeds budget",
        ));
    }
    let mut out = Vec::new();
    push_string(&mut out, address)?;
    let mut tags = String::from(",");
    for arg in args {
        tags.push(match arg {
            OscArg::Int(_) => 'i',
            OscArg::Long(_) => 'h',
            OscArg::Float(_) => 'f',
            OscArg::Double(_) => 'd',
            OscArg::String(_) => 's',
            OscArg::Bool(true) => 'T',
            OscArg::Bool(false) => 'F',
            OscArg::Nil => 'N',
        });
    }
    push_string(&mut out, &tags)?;
    for arg in args {
        match arg {
            OscArg::Int(value) => out.extend(value.to_be_bytes()),
            OscArg::Long(value) => out.extend(value.to_be_bytes()),
            OscArg::Float(value) => out.extend(value.to_bits().to_be_bytes()),
            OscArg::Double(value) => out.extend(value.to_bits().to_be_bytes()),
            OscArg::String(value) => push_string(&mut out, value)?,
            OscArg::Bool(_) | OscArg::Nil => {}
        }
    }
    if out.len() > MAX_PACKET {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "OSC packet exceeds datagram budget",
        ));
    }
    Ok(out)
}

pub fn decode_packet(packet: &[u8]) -> Result<Vec<OscMessage>> {
    if packet.is_empty() || packet.len() > MAX_PACKET {
        return Err(Error::new(
            ErrorCode::ProtocolMismatch,
            "OSC packet size is invalid",
        ));
    }
    let mut out = Vec::new();
    decode_into(packet, 0, &mut out)?;
    if out.len() > MAX_MESSAGES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "OSC packet expands to too many messages",
        ));
    }
    Ok(out)
}

fn decode_into(packet: &[u8], depth: usize, out: &mut Vec<OscMessage>) -> Result<()> {
    if depth > MAX_BUNDLE_DEPTH {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "OSC bundle nesting exceeds budget",
        ));
    }
    if packet.starts_with(b"#bundle\0") {
        if packet.len() < 16 {
            return Err(protocol("Truncated OSC bundle"));
        }
        let mut offset = 16usize;
        while offset < packet.len() {
            let size = read_i32(packet, &mut offset)?;
            if size <= 0 {
                return Err(protocol("Invalid OSC bundle element length"));
            }
            let size = usize::try_from(size).map_err(|_| protocol("Invalid OSC bundle length"))?;
            let end = offset
                .checked_add(size)
                .filter(|end| *end <= packet.len())
                .ok_or_else(|| protocol("Truncated OSC bundle element"))?;
            decode_into(&packet[offset..end], depth + 1, out)?;
            offset = end;
            if out.len() > MAX_MESSAGES {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "OSC bundle message count exceeds budget",
                ));
            }
        }
        return Ok(());
    }

    let mut offset = 0usize;
    let address = read_string(packet, &mut offset, 512)?;
    validate_address(&address)?;
    let tags = read_string(packet, &mut offset, 256)?;
    let tags = tags
        .strip_prefix(',')
        .ok_or_else(|| protocol("OSC type tag string is missing comma"))?;
    if tags.chars().count() > MAX_ARGS {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "OSC type tag count exceeds budget",
        ));
    }
    let mut args = Vec::new();
    for tag in tags.chars() {
        args.push(match tag {
            'i' => OscArg::Int(read_i32(packet, &mut offset)?),
            'h' => OscArg::Long(read_i64(packet, &mut offset)?),
            'f' => OscArg::Float(f32::from_bits(read_u32(packet, &mut offset)?)),
            'd' => OscArg::Double(f64::from_bits(read_u64(packet, &mut offset)?)),
            's' => OscArg::String(read_string(packet, &mut offset, 8_192)?),
            'T' => OscArg::Bool(true),
            'F' => OscArg::Bool(false),
            'N' => OscArg::Nil,
            _ => return Err(protocol("Unsupported OSC type tag")),
        });
    }
    if offset != packet.len() {
        return Err(protocol("OSC message contains trailing bytes"));
    }
    out.push(OscMessage { address, args });
    Ok(())
}

fn parse_strip(message: &OscMessage) -> Result<Option<ArdourStrip>> {
    let Some(OscArg::String(kind)) = message.args.first() else {
        return Ok(None);
    };
    if !matches!(kind.as_str(), "AT" | "MT" | "B" | "MB" | "FB" | "V") {
        return Ok(None);
    }
    if message.args.len() < 7 || message.args.len() > 8 {
        return Err(protocol("Ardour strip reply has unexpected arity"));
    }
    let name = string_at(&message.args, 1)?;
    let inputs = u32_num(&message.args, 2)?;
    let outputs = u32_num(&message.args, 3)?;
    let muted = bool_num(&message.args, 4)?;
    let soloed = bool_num(&message.args, 5)?;
    let ssid = u32_num(&message.args, 6)?;
    if ssid == 0 {
        return Err(protocol("Ardour SSID must be positive"));
    }
    let record_enabled = if message.args.len() == 8 {
        Some(bool_num(&message.args, 7)?)
    } else {
        None
    };
    Ok(Some(ArdourStrip {
        kind: kind.clone(),
        name,
        inputs,
        outputs,
        muted,
        soloed,
        ssid,
        record_enabled,
    }))
}

fn parse_end_route_list(message: &OscMessage) -> Result<Option<(u32, u64, bool)>> {
    let Some(OscArg::String(marker)) = message.args.first() else {
        return Ok(None);
    };
    if marker != "end_route_list" {
        return Ok(None);
    }
    if message.args.len() != 4 {
        return Err(protocol("Ardour end_route_list reply has unexpected arity"));
    }
    let sample_rate = u32_num(&message.args, 1)?;
    if !(8_000..=384_000).contains(&sample_rate) {
        return Err(protocol(
            "Ardour session sample rate is outside semantic bounds",
        ));
    }
    let last_frame = u64_num(&message.args, 2)?;
    let monitor = bool_num(&message.args, 3)?;
    Ok(Some((sample_rate, last_frame, monitor)))
}

fn push_string(out: &mut Vec<u8>, value: &str) -> Result<()> {
    if value.len() > 8_192 || value.contains('\0') {
        return Err(Error::invalid("OSC string exceeds bounded contract"));
    }
    out.extend(value.as_bytes());
    out.push(0);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
    Ok(())
}

fn read_string(packet: &[u8], offset: &mut usize, max: usize) -> Result<String> {
    if *offset >= packet.len() {
        return Err(protocol("Truncated OSC string"));
    }
    let rest = &packet[*offset..];
    let nul = rest
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| protocol("Unterminated OSC string"))?;
    if nul > max {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "OSC string exceeds budget",
        ));
    }
    let value = std::str::from_utf8(&rest[..nul])
        .map_err(|_| protocol("OSC string is not UTF-8"))?
        .to_owned();
    let consumed = (nul + 1 + 3) & !3;
    *offset = offset
        .checked_add(consumed)
        .filter(|next| *next <= packet.len())
        .ok_or_else(|| protocol("Truncated OSC string padding"))?;
    Ok(value)
}

fn read_i32(packet: &[u8], offset: &mut usize) -> Result<i32> {
    Ok(i32::from_be_bytes(read_array(packet, offset)?))
}
fn read_u32(packet: &[u8], offset: &mut usize) -> Result<u32> {
    Ok(u32::from_be_bytes(read_array(packet, offset)?))
}
fn read_i64(packet: &[u8], offset: &mut usize) -> Result<i64> {
    Ok(i64::from_be_bytes(read_array(packet, offset)?))
}
fn read_u64(packet: &[u8], offset: &mut usize) -> Result<u64> {
    Ok(u64::from_be_bytes(read_array(packet, offset)?))
}
fn read_array<const N: usize>(packet: &[u8], offset: &mut usize) -> Result<[u8; N]> {
    let end = offset
        .checked_add(N)
        .filter(|end| *end <= packet.len())
        .ok_or_else(|| protocol("Truncated OSC numeric argument"))?;
    let mut value = [0u8; N];
    value.copy_from_slice(&packet[*offset..end]);
    *offset = end;
    Ok(value)
}

fn string_at(args: &[OscArg], index: usize) -> Result<String> {
    match args.get(index) {
        Some(OscArg::String(value)) => Ok(value.clone()),
        _ => Err(protocol("Expected OSC string argument")),
    }
}
fn u32_num(args: &[OscArg], index: usize) -> Result<u32> {
    let value = i64_num(args, index)?;
    u32::try_from(value).map_err(|_| protocol("OSC integer is outside u32 bounds"))
}
fn u64_num(args: &[OscArg], index: usize) -> Result<u64> {
    let value = i64_num(args, index)?;
    u64::try_from(value).map_err(|_| protocol("OSC integer is outside u64 bounds"))
}
fn i64_num(args: &[OscArg], index: usize) -> Result<i64> {
    match args.get(index) {
        Some(OscArg::Int(value)) => Ok(i64::from(*value)),
        Some(OscArg::Long(value)) => Ok(*value),
        Some(OscArg::Float(value)) if value.is_finite() => Ok(*value as i64),
        Some(OscArg::Double(value)) if value.is_finite() => Ok(*value as i64),
        _ => Err(protocol("Expected bounded OSC numeric argument")),
    }
}
fn bool_num(args: &[OscArg], index: usize) -> Result<bool> {
    match args.get(index) {
        Some(OscArg::Bool(value)) => Ok(*value),
        Some(OscArg::Int(0)) => Ok(false),
        Some(OscArg::Int(1)) => Ok(true),
        Some(OscArg::Float(value)) if *value == 0.0 => Ok(false),
        Some(OscArg::Float(value)) if *value == 1.0 => Ok(true),
        _ => Err(protocol("Expected OSC boolean argument")),
    }
}

fn validate_address(address: &str) -> Result<()> {
    if !address.starts_with('/')
        || address.len() > 512
        || address.contains('\0')
        || address.chars().any(char::is_control)
    {
        return Err(Error::invalid("Invalid OSC address"));
    }
    Ok(())
}

fn protocol(message: &'static str) -> Error {
    Error::new(ErrorCode::ProtocolMismatch, message)
}
fn io_error(step: &str, error: std::io::Error) -> Error {
    Error::new(
        ErrorCode::BackendFailed,
        format!("{step} failed ({:?})", error.kind()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc_message_roundtrip_preserves_typed_arguments() {
        let args = vec![
            OscArg::Int(-7),
            OscArg::Long(9_000_000_000),
            OscArg::Float(0.25),
            OscArg::Double(-0.5),
            OscArg::String("track".into()),
            OscArg::Bool(true),
            OscArg::Bool(false),
            OscArg::Nil,
        ];
        let encoded = encode_message("/semwright/test", &args).unwrap();
        let decoded = decode_packet(&encoded).unwrap();
        assert_eq!(
            decoded,
            vec![OscMessage {
                address: "/semwright/test".into(),
                args,
            }]
        );
    }

    #[test]
    fn osc_rejects_trailing_bytes_and_invalid_addresses() {
        let mut encoded = encode_message("/ok", &[]).unwrap();
        encoded.extend([0, 0, 0, 0]);
        assert!(decode_packet(&encoded).is_err());
        assert!(encode_message("relative", &[]).is_err());
        assert!(encode_message("/bad\0path", &[]).is_err());
    }

    #[test]
    fn strip_reply_is_strict_and_bounded() {
        let message = OscMessage {
            address: "/reply".into(),
            args: vec![
                OscArg::String("AT".into()),
                OscArg::String("Dialogue".into()),
                OscArg::Int(2),
                OscArg::Int(2),
                OscArg::Int(1),
                OscArg::Int(0),
                OscArg::Int(42),
                OscArg::Int(1),
            ],
        };
        let strip = parse_strip(&message).unwrap().unwrap();
        assert_eq!(strip.ssid, 42);
        assert_eq!(strip.name, "Dialogue");
        assert!(strip.muted);
        assert_eq!(strip.record_enabled, Some(true));

        let mut invalid = message;
        invalid.args[6] = OscArg::Int(0);
        assert!(parse_strip(&invalid).is_err());
    }

    #[test]
    fn end_route_list_validates_sample_rate_and_frame_count() {
        let message = OscMessage {
            address: "/reply".into(),
            args: vec![
                OscArg::String("end_route_list".into()),
                OscArg::Int(48_000),
                OscArg::Long(123_456),
                OscArg::Int(1),
            ],
        };
        assert_eq!(
            parse_end_route_list(&message).unwrap(),
            Some((48_000, 123_456, true))
        );

        let mut invalid = message;
        invalid.args[1] = OscArg::Int(100);
        assert!(parse_end_route_list(&invalid).is_err());
    }
}

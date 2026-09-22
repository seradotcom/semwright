//! Strict bounded JSON: duplicate object keys, invalid Unicode, NaN and trailing bytes fail.
//! Number lexemes preserve integer precision. This small implementation requires independent review.
use crate::{Error, Result};
use std::collections::BTreeMap;
pub const MAX_JSON: usize = 1_048_576;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Self::String(v.into())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Self::String(v)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Self::Number(v.to_string())
    }
}
impl From<usize> for Value {
    fn from(v: usize) -> Self {
        Self::Number(v.to_string())
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Self::Number(v.to_string())
    }
}
pub fn obj<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    Value::Object(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
pub fn array(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Array(values.into_iter().collect())
}
impl Value {
    pub fn object(&self) -> Result<&BTreeMap<String, Value>> {
        if let Self::Object(v) = self {
            Ok(v)
        } else {
            Err(Error::invalid("Object required"))
        }
    }
    pub fn as_array(&self) -> Result<&[Value]> {
        if let Self::Array(v) = self {
            Ok(v)
        } else {
            Err(Error::invalid("Array required"))
        }
    }
    pub fn string(&self) -> Result<&str> {
        if let Self::String(v) = self {
            Ok(v)
        } else {
            Err(Error::invalid("String required"))
        }
    }
    pub fn u64(&self) -> Result<u64> {
        if let Self::Number(v) = self {
            v.parse()
                .map_err(|_| Error::invalid("Unsigned integer required"))
        } else {
            Err(Error::invalid("Unsigned integer required"))
        }
    }
    pub fn i64(&self) -> Result<i64> {
        if let Self::Number(v) = self {
            v.parse().map_err(|_| Error::invalid("Integer required"))
        } else {
            Err(Error::invalid("Integer required"))
        }
    }
    pub fn boolean(&self) -> Result<bool> {
        if let Self::Bool(v) = self {
            Ok(*v)
        } else {
            Err(Error::invalid("Boolean required"))
        }
    }
    pub fn get(&self, key: &str) -> Result<&Value> {
        self.object()?
            .get(key)
            .ok_or_else(|| Error::invalid(format!("Missing field {key}")))
    }
    pub fn opt(&self, key: &str) -> Option<&Value> {
        if let Self::Object(v) = self {
            v.get(key)
        } else {
            None
        }
    }
    pub fn str(&self, key: &str) -> Result<&str> {
        self.get(key)?.string()
    }
    pub fn uint(&self, key: &str) -> Result<u64> {
        self.get(key)?.u64()
    }
    pub fn flag(&self, key: &str, default: bool) -> Result<bool> {
        self.opt(key).map_or(Ok(default), Value::boolean)
    }
    pub fn strict(&self, allowed: &[&str], required: &[&str]) -> Result<()> {
        let m = self.object()?;
        if m.keys().any(|k| !allowed.contains(&k.as_str())) {
            return Err(Error::invalid("Unknown object field"));
        }
        if required.iter().any(|k| !m.contains_key(*k)) {
            return Err(Error::invalid("Required object field missing"));
        }
        Ok(())
    }
    pub fn encode(&self) -> String {
        let mut s = String::new();
        write(self, &mut s);
        s
    }
}
pub fn quote(text: &str) -> String {
    let mut s = String::new();
    escape(text, &mut s);
    s
}
fn escape(text: &str, s: &mut String) {
    s.push('"');
    for c in text.chars() {
        match c {
            '"' => s.push_str("\\\""),
            '\\' => s.push_str("\\\\"),
            '\n' => s.push_str("\\n"),
            '\r' => s.push_str("\\r"),
            '\t' => s.push_str("\\t"),
            c if c < '\u{20}' => s.push_str(&format!("\\u{:04x}", c as u32)),
            c => s.push(c),
        }
    }
    s.push('"');
}
fn write(v: &Value, s: &mut String) {
    match v {
        Value::Null => s.push_str("null"),
        Value::Bool(b) => s.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => s.push_str(n),
        Value::String(t) => escape(t, s),
        Value::Array(a) => {
            s.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write(v, s);
            }
            s.push(']');
        }
        Value::Object(m) => {
            s.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                escape(k, s);
                s.push(':');
                write(v, s);
            }
            s.push('}');
        }
    }
}
/// Explicit field order is needed for hashes of Semwright serde structs.
pub fn ordered(pairs: &[(&str, String)]) -> String {
    let mut s = String::from("{");
    for (i, (k, v)) in pairs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&quote(k));
        s.push(':');
        s.push_str(v);
    }
    s.push('}');
    s
}
pub fn parse(bytes: &[u8]) -> Result<Value> {
    if bytes.is_empty() || bytes.len() > MAX_JSON {
        return Err(Error::limit("JSON byte budget exceeded"));
    }
    let s = std::str::from_utf8(bytes).map_err(|_| Error::invalid("JSON is not UTF-8"))?;
    let mut p = Parser { s, i: 0, nodes: 0 };
    let v = p.value(0)?;
    p.ws();
    if p.i != s.len() {
        return Err(p.error("Trailing JSON data"));
    }
    Ok(v)
}
struct Parser<'a> {
    s: &'a str,
    i: usize,
    nodes: usize,
}
impl Parser<'_> {
    fn error(&self, message: &str) -> Error {
        Error::invalid(format!("{message} at byte {}", self.i))
    }
    fn ws(&mut self) {
        while matches!(
            self.s.as_bytes().get(self.i),
            Some(b' ' | b'\n' | b'\r' | b'\t')
        ) {
            self.i += 1;
        }
    }
    fn take(&mut self, c: u8) -> bool {
        if self.s.as_bytes().get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn literal(&mut self, t: &str, v: Value) -> Result<Value> {
        if self.s[self.i..].starts_with(t) {
            self.i += t.len();
            Ok(v)
        } else {
            Err(self.error("Invalid literal"))
        }
    }
    fn value(&mut self, depth: usize) -> Result<Value> {
        self.nodes += 1;
        if depth > 40 || self.nodes > 16384 {
            return Err(Error::limit("JSON structure budget exceeded"));
        }
        self.ws();
        match self.s.as_bytes().get(self.i).copied() {
            Some(b'n') => self.literal("null", Value::Null),
            Some(b't') => self.literal("true", true.into()),
            Some(b'f') => self.literal("false", false.into()),
            Some(b'"') => Ok(self.text()?.into()),
            Some(b'[') => {
                self.i += 1;
                self.ws();
                let mut a = vec![];
                if self.take(b']') {
                    return Ok(Value::Array(a));
                }
                loop {
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    if self.take(b']') {
                        break;
                    }
                    if !self.take(b',') {
                        return Err(self.error("Expected array separator"));
                    }
                }
                Ok(Value::Array(a))
            }
            Some(b'{') => {
                self.i += 1;
                self.ws();
                let mut m = BTreeMap::new();
                if self.take(b'}') {
                    return Ok(Value::Object(m));
                }
                loop {
                    self.ws();
                    let k = self.text()?;
                    self.ws();
                    if !self.take(b':') {
                        return Err(self.error("Expected colon"));
                    }
                    let v = self.value(depth + 1)?;
                    if m.insert(k, v).is_some() {
                        return Err(self.error("Duplicate JSON key"));
                    }
                    self.ws();
                    if self.take(b'}') {
                        break;
                    }
                    if !self.take(b',') {
                        return Err(self.error("Expected object separator"));
                    }
                }
                Ok(Value::Object(m))
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.error("Expected JSON value")),
        }
    }
    fn number(&mut self) -> Result<Value> {
        let start = self.i;
        self.take(b'-');
        if !self.take(b'0') {
            let begin = self.i;
            while self
                .s
                .as_bytes()
                .get(self.i)
                .is_some_and(u8::is_ascii_digit)
            {
                self.i += 1;
            }
            if begin == self.i {
                return Err(self.error("Expected digit"));
            }
        }
        if self.take(b'.') {
            let begin = self.i;
            while self
                .s
                .as_bytes()
                .get(self.i)
                .is_some_and(u8::is_ascii_digit)
            {
                self.i += 1;
            }
            if begin == self.i {
                return Err(self.error("Empty decimal fraction"));
            }
        }
        if self.take(b'e') || self.take(b'E') {
            if !self.take(b'+') {
                self.take(b'-');
            }
            let begin = self.i;
            while self
                .s
                .as_bytes()
                .get(self.i)
                .is_some_and(u8::is_ascii_digit)
            {
                self.i += 1;
            }
            if begin == self.i {
                return Err(self.error("Empty exponent"));
            }
        }
        if self.i - start > 64 {
            return Err(Error::limit("Number lexeme exceeds budget"));
        }
        Ok(Value::Number(self.s[start..self.i].to_string()))
    }
    fn hex4(&mut self) -> Result<u32> {
        let mut n = 0;
        for _ in 0..4 {
            let b = *self
                .s
                .as_bytes()
                .get(self.i)
                .ok_or_else(|| self.error("Truncated Unicode escape"))?;
            self.i += 1;
            n = n * 16
                + (b as char)
                    .to_digit(16)
                    .ok_or_else(|| self.error("Invalid hex digit"))?;
        }
        Ok(n)
    }
    fn text(&mut self) -> Result<String> {
        if !self.take(b'"') {
            return Err(self.error("Expected string"));
        }
        let mut out = String::new();
        loop {
            let c = self.s[self.i..]
                .chars()
                .next()
                .ok_or_else(|| self.error("Unterminated string"))?;
            self.i += c.len_utf8();
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let e = *self
                        .s
                        .as_bytes()
                        .get(self.i)
                        .ok_or_else(|| self.error("Truncated escape"))?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let mut u = self.hex4()?;
                            if (0xd800..=0xdbff).contains(&u) {
                                if !self.take(b'\\') || !self.take(b'u') {
                                    return Err(self.error("Missing low surrogate"));
                                }
                                let l = self.hex4()?;
                                if !(0xdc00..=0xdfff).contains(&l) {
                                    return Err(self.error("Invalid low surrogate"));
                                }
                                u = 0x10000 + ((u - 0xd800) << 10) + (l - 0xdc00);
                            }
                            out.push(
                                char::from_u32(u)
                                    .ok_or_else(|| self.error("Invalid Unicode scalar"))?,
                            );
                        }
                        _ => return Err(self.error("Invalid escape")),
                    }
                }
                c if c < '\u{20}' => return Err(self.error("Unescaped control character")),
                c => out.push(c),
            }
            if out.len() > 65536 {
                return Err(Error::limit("JSON string budget exceeded"));
            }
        }
    }
}
/// Terminal/control-safe display only; raw application data stays in the AST.
pub fn display(text: &str) -> String {
    text.chars()
        .take(512)
        .map(|c| {
            if c.is_control() || matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') {
                format!("\\u{{{:x}}}", c as u32)
            } else {
                c.to_string()
            }
        })
        .collect()
}

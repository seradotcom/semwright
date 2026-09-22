//! Bounded XML 1.0 subset. DTDs, processing instructions and user-defined entities are rejected.
//! No resolver, network callback, archive handling or recursion beyond the explicit depth budget.
use crate::{Error, Result};
use std::collections::{BTreeMap, BTreeSet};
pub const MAX_XML: usize = 8 * 1024 * 1024;
pub const MAX_ELEMENTS: usize = 50_000;
pub const MAX_DEPTH: usize = 48;
pub const MAX_TEXT: usize = 65_536;
pub const MAX_ATTRIBUTES: usize = 64;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Child {
    Element(Node),
    Text(String),
    Comment(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub attrs: BTreeMap<String, String>,
    pub children: Vec<Child>,
}
impl Node {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            attrs: BTreeMap::new(),
            children: vec![],
        }
    }
    pub fn attr(mut self, key: &str, value: impl ToString) -> Self {
        self.attrs.insert(key.into(), value.to_string());
        self
    }
    pub fn set(&mut self, key: &str, value: impl ToString) {
        self.attrs.insert(key.into(), value.to_string());
    }
    pub fn a(&self, key: &str) -> Option<&str> {
        self.attrs.get(key).map(String::as_str)
    }
    pub fn push(&mut self, node: Node) {
        self.children.push(Child::Element(node));
    }
    pub fn elements(&self) -> impl Iterator<Item = &Node> {
        self.children.iter().filter_map(|c| {
            if let Child::Element(n) = c {
                Some(n)
            } else {
                None
            }
        })
    }
    pub fn elements_mut(&mut self) -> impl Iterator<Item = &mut Node> {
        self.children.iter_mut().filter_map(|c| {
            if let Child::Element(n) = c {
                Some(n)
            } else {
                None
            }
        })
    }
    pub fn text(&self) -> Option<String> {
        if self.children.iter().any(|c| matches!(c, Child::Element(_))) {
            return None;
        }
        Some(
            self.children
                .iter()
                .filter_map(|c| {
                    if let Child::Text(s) = c {
                        Some(s.as_str())
                    } else {
                        None
                    }
                })
                .collect(),
        )
    }
    pub fn property(&self, key: &str) -> Option<String> {
        self.elements()
            .find(|n| n.name == "property" && n.a("name") == Some(key))
            .and_then(Node::text)
    }
    pub fn set_property(&mut self, key: &str, value: &str) {
        if let Some(p) = self
            .elements_mut()
            .find(|n| n.name == "property" && n.a("name") == Some(key))
        {
            p.children = vec![Child::Text(value.into())];
            return;
        }
        self.push(property(key, value));
    }
    pub fn by_id(&self, id: &str) -> Option<&Node> {
        if self.a("id") == Some(id) {
            return Some(self);
        }
        self.elements().find_map(|n| n.by_id(id))
    }
    pub fn by_id_mut(&mut self, id: &str) -> Option<&mut Node> {
        if self.a("id") == Some(id) {
            return Some(self);
        }
        for n in self.elements_mut() {
            if let Some(found) = n.by_id_mut(id) {
                return Some(found);
            }
        }
        None
    }
    pub fn walk<'a>(&'a self, out: &mut Vec<&'a Node>) {
        out.push(self);
        for n in self.elements() {
            n.walk(out);
        }
    }
}
pub fn property(name: &str, value: &str) -> Node {
    let mut p = Node::new("property").attr("name", name);
    p.children.push(Child::Text(value.into()));
    p
}
pub fn valid_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r')
        || matches!(c as u32,0x20..=0xd7ff|0xe000..=0xfffd|0x10000..=0x10ffff)
}
fn name_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || matches!(c, b'_' | b':')
}
fn name_char(c: u8) -> bool {
    name_start(c) || c.is_ascii_digit() || matches!(c, b'-' | b'.')
}
pub fn parse(bytes: &[u8]) -> Result<Node> {
    if bytes.is_empty() || bytes.len() > MAX_XML {
        return Err(Error::limit("XML byte budget exceeded"));
    }
    let s = std::str::from_utf8(bytes).map_err(|_| Error::invalid("XML must be UTF-8"))?;
    if s.chars().any(|c| !valid_char(c)) {
        return Err(Error::invalid("Invalid XML character"));
    }
    let mut p = Parser {
        s,
        i: usize::from(s.starts_with('\u{feff}')) * 3,
        elements: 0,
        text: 0,
        nodes: 0,
    };
    p.ws();
    if p.starts("<?xml ") {
        let end = p.s[p.i..]
            .find("?>")
            .ok_or_else(|| p.err("Truncated XML declaration"))?;
        let decl = &p.s[p.i..p.i + end + 2];
        if decl.len() > 128 || !(decl.contains("version=\"1.0\"") || decl.contains("version='1.0'"))
        {
            return Err(p.err("Unsupported XML declaration"));
        }
        if decl.contains("encoding")
            && !decl.to_ascii_lowercase().contains("encoding=\"utf-8\"")
            && !decl.to_ascii_lowercase().contains("encoding='utf-8'")
        {
            return Err(p.err("Only UTF-8 XML is supported"));
        }
        p.i += end + 2;
        p.ws();
    }
    while p.starts("<!--") {
        p.comment()?;
        p.ws();
    }
    let root = p.node(0, &BTreeMap::new())?;
    p.ws();
    while p.starts("<!--") {
        p.comment()?;
        p.ws();
    }
    if p.i != p.s.len() {
        return Err(p.err("Trailing XML or multiple roots"));
    }
    if root.name != "mlt" {
        return Err(Error::invalid("Expected mlt root"));
    }
    validate_graph(&root)?;
    Ok(root)
}
struct Parser<'a> {
    s: &'a str,
    i: usize,
    elements: usize,
    text: usize,
    nodes: usize,
}
impl Parser<'_> {
    fn err(&self, m: &str) -> Error {
        Error::invalid(format!("{m} at byte {}", self.i))
    }
    fn starts(&self, s: &str) -> bool {
        self.s[self.i..].starts_with(s)
    }
    fn ws(&mut self) {
        while self
            .s
            .as_bytes()
            .get(self.i)
            .is_some_and(|c| matches!(c, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.i += 1;
        }
    }
    fn expect(&mut self, s: &str) -> Result<()> {
        if !self.starts(s) {
            return Err(self.err("Unexpected XML token"));
        }
        self.i += s.len();
        Ok(())
    }
    fn name(&mut self) -> Result<String> {
        let start = self.i;
        if !self
            .s
            .as_bytes()
            .get(self.i)
            .is_some_and(|c| name_start(*c))
        {
            return Err(self.err("Invalid XML name"));
        }
        self.i += 1;
        while self.s.as_bytes().get(self.i).is_some_and(|c| name_char(*c)) {
            self.i += 1;
        }
        if self.i - start > 256 {
            return Err(Error::limit("XML name too long"));
        }
        Ok(self.s[start..self.i].into())
    }
    fn quoted(&mut self) -> Result<String> {
        let q = *self
            .s
            .as_bytes()
            .get(self.i)
            .ok_or_else(|| self.err("Missing attribute quote"))?;
        if !matches!(q, b'\'' | b'"') {
            return Err(self.err("Attribute value needs quotes"));
        }
        self.i += 1;
        let start = self.i;
        while self.s.as_bytes().get(self.i).is_some_and(|b| *b != q) {
            if self.s.as_bytes()[self.i] == b'<' {
                return Err(self.err("Less-than in attribute"));
            }
            self.i += 1;
        }
        if self.i == self.s.len() {
            return Err(self.err("Unterminated attribute"));
        }
        let raw = &self.s[start..self.i];
        self.i += 1;
        if raw.len() > MAX_TEXT {
            return Err(Error::limit("Attribute too large"));
        }
        decode(raw)
    }
    fn comment(&mut self) -> Result<String> {
        self.expect("<!--")?;
        let end = self.s[self.i..]
            .find("-->")
            .ok_or_else(|| self.err("Unterminated comment"))?;
        if end > MAX_TEXT {
            return Err(Error::limit("Comment budget exceeded"));
        }
        let s = &self.s[self.i..self.i + end];
        if s.contains("--") || s.ends_with('-') {
            return Err(self.err("Invalid XML comment"));
        }
        let out = s.into();
        self.i += end + 3;
        Ok(out)
    }
    fn node(&mut self, depth: usize, parent_ns: &BTreeMap<String, String>) -> Result<Node> {
        if depth >= MAX_DEPTH {
            return Err(Error::limit("XML depth exceeded"));
        }
        self.nodes += 1;
        if self.nodes > MAX_ELEMENTS * 2 {
            return Err(Error::limit("XML node budget exceeded"));
        }
        self.elements += 1;
        if self.elements > MAX_ELEMENTS {
            return Err(Error::limit("XML element budget exceeded"));
        }
        self.expect("<")?;
        if self.starts("!") || self.starts("?") {
            return Err(self.err("DTD, entities and processing instructions are disabled"));
        }
        let name = self.name()?;
        let mut n = Node::new(&name);
        let mut ns = parent_ns.clone();
        loop {
            let before = self.i;
            self.ws();
            if self.starts("/") || self.starts(">") {
                break;
            }
            if self.i == before {
                return Err(self.err("Attribute separation required"));
            }
            let key = self.name()?;
            self.ws();
            self.expect("=")?;
            self.ws();
            let value = self.quoted()?;
            if n.attrs.len() >= MAX_ATTRIBUTES {
                return Err(Error::limit("Too many attributes"));
            }
            if n.attrs.insert(key.clone(), value.clone()).is_some() {
                return Err(self.err("Duplicate XML attribute"));
            }
            if let Some(prefix) = key.strip_prefix("xmlns:") {
                ns.insert(prefix.into(), value);
            }
        }
        for key in std::iter::once(n.name.as_str()).chain(n.attrs.keys().map(String::as_str)) {
            if let Some((prefix, _)) = key.split_once(':') {
                if prefix != "xmlns" && prefix != "xml" && !ns.contains_key(prefix) {
                    return Err(self.err("Undeclared namespace prefix"));
                }
            }
        }
        if self.starts("/>") {
            self.i += 2;
            return Ok(n);
        }
        self.expect(">")?;
        loop {
            if self.starts("</") {
                self.i += 2;
                let end = self.name()?;
                self.ws();
                self.expect(">")?;
                if end != name {
                    return Err(self.err("Mismatched closing tag"));
                }
                break;
            }
            if self.i >= self.s.len() {
                return Err(self.err("Unclosed element"));
            }
            if self.starts("<!--") {
                let c = self.comment()?;
                n.children.push(Child::Comment(c));
            } else if self.starts("<![CDATA[") {
                self.i += 9;
                let end = self.s[self.i..]
                    .find("]]>")
                    .ok_or_else(|| self.err("Unclosed CDATA"))?;
                if end > MAX_TEXT {
                    return Err(Error::limit("CDATA budget exceeded"));
                }
                n.children
                    .push(Child::Text(self.s[self.i..self.i + end].into()));
                self.i += end + 3;
            } else if self.starts("<") {
                n.push(self.node(depth + 1, &ns)?);
            } else {
                let end = self.s[self.i..].find('<').unwrap_or(self.s.len() - self.i);
                let raw = &self.s[self.i..self.i + end];
                if raw.len() > MAX_TEXT || raw.contains("]]>") {
                    return Err(self.err("Invalid or oversized text"));
                }
                self.text += raw.len();
                if self.text > MAX_XML {
                    return Err(Error::limit("Text budget exceeded"));
                }
                n.children.push(Child::Text(decode(raw)?));
                self.i += end;
            }
            if !matches!(n.children.last(), Some(Child::Element(_))) {
                self.nodes += 1;
            }
            if self.nodes > MAX_ELEMENTS * 2 || n.children.len() > MAX_ELEMENTS {
                return Err(Error::limit("XML children budget exceeded"));
            }
        }
        Ok(n)
    }
}
pub fn decode(raw: &str) -> Result<String> {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i + 1..];
        let end = rest
            .find(';')
            .ok_or_else(|| Error::invalid("Unterminated XML entity"))?;
        if end > 16 {
            return Err(Error::invalid("Entity token too long"));
        }
        let token = &rest[..end];
        let c = match token {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "apos" => '\'',
            "quot" => '"',
            _ => {
                let n = if let Some(hex) = token.strip_prefix("#x") {
                    u32::from_str_radix(hex, 16)
                } else if let Some(dec) = token.strip_prefix('#') {
                    dec.parse()
                } else {
                    return Err(Error::invalid("Named entity is disabled"));
                }
                .map_err(|_| Error::invalid("Invalid character reference"))?;
                char::from_u32(n)
                    .filter(|c| valid_char(*c))
                    .ok_or_else(|| Error::invalid("Invalid XML scalar"))?
            }
        };
        out.push(c);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    if out.len() > MAX_TEXT {
        return Err(Error::limit("Decoded text budget exceeded"));
    }
    Ok(out)
}
fn escaped(text: &str, attribute: bool, out: &mut String) -> Result<()> {
    for c in text.chars() {
        if !valid_char(c) {
            return Err(Error::invalid("Invalid output XML character"));
        }
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\r' => out.push_str("&#13;"),
            '\n' if attribute => out.push_str("&#10;"),
            '\t' if attribute => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
    Ok(())
}
fn emit(n: &Node, out: &mut String, depth: usize) -> Result<()> {
    if depth >= MAX_DEPTH {
        return Err(Error::limit("Writer depth exceeded"));
    }
    out.push('<');
    out.push_str(&n.name);
    for (k, v) in &n.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        escaped(v, true, out)?;
        out.push('"');
    }
    if n.children.is_empty() {
        out.push_str("/>");
    } else {
        out.push('>');
        for c in &n.children {
            match c {
                Child::Element(e) => emit(e, out, depth + 1)?,
                Child::Text(t) => escaped(t, false, out)?,
                Child::Comment(t) => {
                    if t.contains("--") || t.ends_with('-') {
                        return Err(Error::invalid("Invalid output comment"));
                    }
                    out.push_str("<!--");
                    out.push_str(t);
                    out.push_str("-->");
                }
            }
            if out.len() > MAX_XML {
                return Err(Error::limit("Writer byte budget exceeded"));
            }
        }
        out.push_str("</");
        out.push_str(&n.name);
        out.push('>');
    }
    Ok(())
}
pub fn serialize(n: &Node) -> Result<String> {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    emit(n, &mut s, 0)?;
    if s.len() > MAX_XML {
        return Err(Error::limit("Output XML too large"));
    }
    Ok(s)
}
/// Global IDs and producer IDREFs form a bounded DAG. No external graph traversal occurs.
pub fn validate_graph(root: &Node) -> Result<()> {
    let mut all = vec![];
    root.walk(&mut all);
    if all.len() > MAX_ELEMENTS {
        return Err(Error::limit("Too many graph nodes"));
    }
    let mut ids = BTreeMap::new();
    for n in &all {
        if let Some(id) = n.a("id") {
            if id.is_empty() || id.len() > 256 || ids.insert(id, *n).is_some() {
                return Err(Error::invalid("Empty, oversized or duplicate MLT ID"));
            }
        }
        let mut names = BTreeSet::new();
        for property in n.elements().filter(|p| p.name == "property") {
            let name = property
                .a("name")
                .ok_or_else(|| Error::invalid("Property requires a name"))?;
            if name.is_empty() || !names.insert(name) {
                return Err(Error::invalid(
                    "Duplicate or empty property name is ambiguous",
                ));
            }
            if property.text().is_none() {
                let key = property.a("name").unwrap_or("");
                if key.starts_with("semwright:")
                    || matches!(
                        key,
                        "mlt_service"
                            | "resource"
                            | "length"
                            | "shotcut"
                            | "level"
                            | "disable"
                            | "a_track"
                            | "b_track"
                            | "kdenlive:docproperties.version"
                    )
                {
                    return Err(Error::unsupported(
                        "A routing or curated semantic property must be scalar",
                    ));
                }
                // Structured application annotations (for example shotcut:markers) stay opaque.
                // They remain covered by XML depth/element/IDREF budgets and are never executed.
            }
        }
    }
    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for n in &all {
        if let Some(reference) = n.a("producer") {
            if !ids.contains_key(reference) {
                return Err(Error::invalid("Unresolved MLT producer IDREF"));
            }
        }
    }
    for (id, node) in &ids {
        let mut descendants = vec![];
        node.walk(&mut descendants);
        for descendant in descendants {
            if let Some(reference) = descendant.a("producer") {
                edges.entry(id).or_default().push(reference);
            }
        }
    }
    // Memoize height, not merely 'visited': a shared subgraph must not hide a long path.
    fn visit<'a>(
        id: &'a str,
        edges: &BTreeMap<&'a str, Vec<&'a str>>,
        active: &mut BTreeSet<&'a str>,
        heights: &mut BTreeMap<&'a str, usize>,
        depth: usize,
    ) -> Result<usize> {
        if depth > 32 {
            return Err(Error::limit("Nested composition depth exceeded"));
        }
        if let Some(height) = heights.get(id) {
            if depth + height > 32 {
                return Err(Error::limit("Nested composition depth exceeded"));
            }
            return Ok(*height);
        }
        if !active.insert(id) {
            return Err(Error::invalid("Cyclic MLT composition"));
        }
        let mut height = 0;
        if let Some(children) = edges.get(id) {
            for child in children {
                height = height.max(1 + visit(child, edges, active, heights, depth + 1)?);
            }
        }
        active.remove(id);
        heights.insert(id, height);
        if depth + height > 32 {
            return Err(Error::limit("Nested composition depth exceeded"));
        }
        Ok(height)
    }
    let mut heights = BTreeMap::new();
    for id in ids.keys() {
        visit(id, &edges, &mut BTreeSet::new(), &mut heights, 0)?;
    }
    Ok(())
}

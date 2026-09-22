use super::*;
pub struct KdenliveAdapter;
impl ProjectAdapter for KdenliveAdapter {
    fn name(&self) -> &'static str {
        "kdenlive-generation5-inspection/1"
    }
    fn detect(&self, root: &Node) -> bool {
        let mut nodes = vec![];
        root.walk(&mut nodes);
        nodes.iter().any(|n| {
            n.name == "property" && n.a("name").is_some_and(|s| s.starts_with("kdenlive:"))
        }) || root.a("xmlns:kdenlive").is_some()
    }
    fn parse(&self, root: Node) -> Result<Project> {
        let bin = root.by_id("main_bin").ok_or_else(|| {
            Error::unsupported("Kdenlive without main_bin: migration is not implemented")
        })?;
        let version = bin.property("kdenlive:docproperties.version");
        let seqs: Vec<_> = root
            .elements()
            .filter(|n| {
                n.name == "tractor"
                    && n.property("kdenlive:uuid").is_some()
                    && n.property("kdenlive:projectTractor").as_deref() != Some("1")
            })
            .filter_map(|n| n.a("id").map(str::to_owned))
            .collect();
        if seqs.is_empty() {
            return Err(Error::unsupported(
                "Pre-generation-5 or unrecognized Kdenlive sequence structure",
            ));
        }
        let mut p = common(root, Format::Kdenlive, seqs)?;
        p.format_version = version;
        if p.format_version.as_deref() != Some("1.1") {
            p.warnings
                .push("Unknown Kdenlive document version: all mutations disabled".into());
        }
        p.warnings.push("Bin masters, timeline instances, proxies, original URLs and sequence wrappers are preserved separately; no implicit migration".into());
        Ok(p)
    }
    fn serialize(&self, p: &Project) -> Result<String> {
        p.validate()?;
        xml::serialize(
            p.original
                .as_ref()
                .ok_or_else(|| Error::invalid("Kdenlive original graph missing"))?,
        )
    }
    fn supported_mutation(&self, p: &Project, op: &str) -> Support {
        if p.format_version.as_deref() == Some("1.1") && op == "track.rename" {
            Support::MetadataRisk
        } else {
            Support::Unsupported
        }
    }
}

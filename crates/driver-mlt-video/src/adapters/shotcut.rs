use super::*;
pub struct ShotcutAdapter;
impl ProjectAdapter for ShotcutAdapter {
    fn name(&self) -> &'static str {
        "shotcut-annotations-inspection/1"
    }
    fn detect(&self, root: &Node) -> bool {
        let mut nodes = vec![];
        root.walk(&mut nodes);
        nodes.iter().any(|n| {
            n.name == "property"
                && n.a("name")
                    .is_some_and(|s| s == "shotcut" || s.starts_with("shotcut:"))
        })
    }
    fn parse(&self, root: Node) -> Result<Project> {
        let tractors: Vec<_> = root
            .elements()
            .filter(|n| n.name == "tractor" && n.property("shotcut").as_deref() == Some("1"))
            .collect();
        if tractors.len() != 1 {
            return Err(Error::new(
                "AmbiguousTarget",
                "Expected exactly one Shotcut editable tractor",
            ));
        }
        let seq = id(tractors[0])?;
        let virtual_clip = tractors[0].property("shotcut:virtual").as_deref() == Some("1");
        let export = root.elements().any(|n| n.name == "consumer");
        let format = if virtual_clip {
            Format::ShotcutVirtual
        } else if export {
            Format::ShotcutExport
        } else {
            Format::Shotcut
        };
        let version = root.a("shotcut_version").map(str::to_owned);
        let mut p = common(root, format, vec![seq])?;
        p.format_version = version;
        p.warnings.push(
            "Shotcut annotation preservation is not proof of GUI reopen compatibility".into(),
        );
        if export {
            p.warnings
                .push("Embedded consumer is inert inspection data; never passed to melt".into());
        }
        Ok(p)
    }
    fn serialize(&self, p: &Project) -> Result<String> {
        p.validate()?;
        xml::serialize(
            p.original
                .as_ref()
                .ok_or_else(|| Error::invalid("Shotcut original graph missing"))?,
        )
    }
    fn supported_mutation(&self, p: &Project, op: VideoOperation) -> Support {
        // No real Shotcut version was executed here. Explicit metadata-risk acknowledgement is
        // still insufficient for unknown/future versions: only the synthetic fixture contract
        // is editable. Native application files remain read-only pending real version evidence.
        if p.format == Format::Shotcut
            && p.format_version.as_deref() == Some("synthetic-fixture-1")
            && op == VideoOperation::TrackRename
        {
            Support::MetadataRisk
        } else {
            Support::Unsupported
        }
    }
}

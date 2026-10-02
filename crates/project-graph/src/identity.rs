use crate::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de::Error as _};
macro_rules! identity {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            pub fn new() -> Self {
                Self(format!("{}{}", $prefix, uuid::Uuid::new_v4().simple()))
            }
            pub fn parse(value: String) -> Result<Self> {
                let suffix = value.strip_prefix($prefix).unwrap_or("");
                ensure(
                    suffix.len() == 32
                        && suffix
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                    "opaque identity namespace",
                )?;
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(
                d: D,
            ) -> std::result::Result<Self, D::Error> {
                Self::parse(String::deserialize(d)?).map_err(D::Error::custom)
            }
        }
    };
}
identity!(ProjectId, "prj_");
identity!(LogicalAssetId, "asset_");
identity!(AssetRevision, "rev_");
identity!(DerivationId, "drv_");
identity!(ReceiptId, "receipt_");
identity!(ExternalIntentId, "intent_");

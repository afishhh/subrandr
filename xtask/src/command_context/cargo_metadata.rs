use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct CargoMetadata {
    pub packages: Vec<Package>,
    #[serde(rename = "metadata")]
    pub workspace_metadata: MetadataTable,
}

#[derive(Debug, Deserialize)]
pub struct Package {
    pub name: Box<str>,
    pub version: Box<str>,
    pub metadata: MetadataTable,
}

#[derive(Debug)]
pub struct MetadataTable(serde_json::Map<String, serde_json::Value>);

impl<'de> Deserialize<'de> for MetadataTable {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Allow for `null` values here.
        Ok(Self(Option::deserialize(deserializer)?.unwrap_or_default()))
    }
}

impl MetadataTable {
    pub fn try_parse_key<T: for<'a> Deserialize<'a>>(&self, key: &str) -> Result<T> {
        let value = self
            .0
            .get(key)
            .with_context(|| format!("Key {key} missing"))?;
        T::deserialize(value).map_err(anyhow::Error::from)
    }
}

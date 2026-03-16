/// Describes one 1C configuration to index (main or extension).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ConfigEntry {
    pub id: String,
    pub path: String,
    pub role: String,            // "main" | "extension"
    pub extends: Option<String>, // id of the config this extension extends
    pub name: Option<String>,    // human-readable name (filled from Configuration.xml)
    pub onec_uuid: Option<String>,
    pub alias: Option<String>,
}

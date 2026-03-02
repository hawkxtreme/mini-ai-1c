use std::fs;
use std::path::Path;
use regex::Regex;
use rusqlite::{params, Connection};

/// Known top-level 1C object types that appear in Configuration.xml ChildObjects.
const OBJECT_TYPES: &[&str] = &[
    "Catalog", "Document", "CommonModule", "InformationRegister",
    "AccumulationRegister", "AccountingRegister", "CalculationRegister",
    "ExchangePlan", "BusinessProcess", "Task",
    "ChartOfCharacteristicTypes", "ChartOfAccounts", "ChartOfCalculationTypes",
    "DataProcessor", "Report", "Enum", "Constant",
    "DocumentJournal", "FilterCriterion", "ScheduledJob",
    "WebService", "HTTPService",
    "Role", "Language", "Subsystem", "SessionParameter",
    "FunctionalOption", "DefinedType", "XDTOPackage",
    "EventSubscription", "ExternalDataSource", "SettingsStorage",
    "Sequence", "CommandGroup", "CommonAttribute", "CommonCommand",
    "CommonForm", "CommonPicture", "CommonTemplate", "StyleItem",
];

/// Build the metadata graph (objects + object_items tables).
///
/// Sources (tried in order):
/// 1. `Configuration.xml` — always present; provides object type + name list
/// 2. `ConfigDumpInfo.xml` — optional; provides attributes, tabular sections, forms, modules
///
/// Returns the number of top-level objects indexed.
pub fn build_metadata(root: &Path, db_path: &Path) -> Result<usize, String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Ошибка открытия БД: {}", e))?;

    // Clear existing metadata
    conn.execute("DELETE FROM object_items", []).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM objects", []).map_err(|e| e.to_string())?;

    let mut object_ids: std::collections::HashMap<String, i64> = std::collections::HashMap::new();

    // Step 1: Parse Configuration.xml for the object list
    let config_xml = root.join("Configuration.xml");
    if config_xml.exists() {
        parse_configuration_xml(&config_xml, &conn, &mut object_ids)
            .unwrap_or_else(|e| eprintln!("[1c-search] Configuration.xml: {}", e));
    }

    // Step 2: Parse ConfigDumpInfo.xml for detailed structure (optional)
    let config_dump = root.join("ConfigDumpInfo.xml");
    if config_dump.exists() && !object_ids.is_empty() {
        parse_config_dump_info(&config_dump, &conn, &object_ids)
            .unwrap_or_else(|e| eprintln!("[1c-search] ConfigDumpInfo.xml: {}", e));
    }

    Ok(object_ids.len())
}

/// Parse `<ChildObjects>` section in Configuration.xml.
/// Populates the `objects` table and fills `object_ids` map ("Type.Name" → rowid).
fn parse_configuration_xml(
    path: &Path,
    conn: &Connection,
    object_ids: &mut std::collections::HashMap<String, i64>,
) -> Result<(), String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Чтение Configuration.xml: {}", e))?;

    // Find ChildObjects section
    let child_start = match content.find("<ChildObjects>") {
        Some(pos) => pos,
        None => return Ok(()), // No ChildObjects — possibly a root Configuration.xml without objects
    };
    let child_end = content.find("</ChildObjects>").unwrap_or(content.len());
    let section = &content[child_start..child_end];

    // Match: <ObjectType>ObjectName</ObjectType>
    // Must match only known types to avoid <Name>, <Version>, etc.
    let types_pattern = OBJECT_TYPES.join("|");
    let pattern = format!(r"<({})>([^<\n]+)</(?:{})>", types_pattern, types_pattern);
    let re = Regex::new(&pattern).map_err(|e| e.to_string())?;

    conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;

    for cap in re.captures_iter(section) {
        let obj_type = cap.get(1).unwrap().as_str();
        let obj_name = cap.get(2).unwrap().as_str().trim();
        if obj_name.is_empty() {
            continue;
        }
        if conn
            .execute(
                "INSERT INTO objects (obj_type, name, name_lower) VALUES (?1, ?2, ?3)",
                params![obj_type, obj_name, obj_name.to_lowercase()],
            )
            .is_ok()
        {
            let id = conn.last_insert_rowid();
            object_ids.insert(format!("{}.{}", obj_type, obj_name), id);
        }
    }

    conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
    Ok(())
}

/// Parse ConfigDumpInfo.xml: extract `<Metadata name="...">` entries and
/// populate `object_items` (attributes, tabular sections, forms, commands, modules).
fn parse_config_dump_info(
    path: &Path,
    conn: &Connection,
    object_ids: &std::collections::HashMap<String, i64>,
) -> Result<(), String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("Чтение ConfigDumpInfo.xml: {}", e))?;

    let re = Regex::new(r#"<Metadata\s[^>]*?name="([^"]+)""#)
        .map_err(|e| e.to_string())?;

    let names: Vec<String> = re
        .captures_iter(&content)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .collect();

    conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;

    for name in &names {
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() < 3 {
            continue;
        }

        let parent_key = format!("{}.{}", parts[0], parts[1]);
        let obj_id = match object_ids.get(&parent_key) {
            Some(&id) => id,
            None => continue,
        };

        match parts.len() {
            3 => {
                // e.g. Catalog.Agent.ObjectModule
                if parts[2].ends_with("Module") {
                    let _ = conn.execute(
                        "INSERT INTO object_items (object_id, item_type, item_name, parent_section) \
                         VALUES (?1, ?2, ?3, NULL)",
                        params![obj_id, parts[2], parts[2]],
                    );
                }
            }
            4 => {
                // e.g. Catalog.Agent.Attribute.Code
                let child_type = parts[2];
                let child_name = parts[3];
                let mapped = match child_type {
                    "Attribute" | "Dimension" | "Resource" | "AccountingFlag"
                    | "ExtDimensionAccountingFlag" | "AddressingAttribute" => "Attribute",
                    "TabularSection" | "StandardTabularSection" => "TabularSection",
                    "Form" => "Form",
                    "Command" => "Command",
                    t if t.ends_with("Module") => t,
                    _ => continue,
                };
                let _ = conn.execute(
                    "INSERT INTO object_items (object_id, item_type, item_name, parent_section) \
                     VALUES (?1, ?2, ?3, NULL)",
                    params![obj_id, mapped, child_name],
                );
            }
            6 => {
                // e.g. Catalog.Agent.TabularSection.Tools.Attribute.Name
                if parts[2] == "TabularSection"
                    && (parts[4] == "Attribute" || parts[4] == "Dimension")
                {
                    let _ = conn.execute(
                        "INSERT INTO object_items (object_id, item_type, item_name, parent_section) \
                         VALUES (?1, ?2, ?3, ?4)",
                        params![obj_id, "Attribute", parts[5], parts[3]],
                    );
                }
            }
            _ => {}
        }
    }

    conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
    Ok(())
}

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

/// Read configuration identity from Configuration.xml:
/// - config name (first `<Name>` before `<ChildObjects>`)
/// - UUID from `<Configuration uuid="...">` attribute
///
/// Returns `(name, uuid)` — both `None` if not found or file missing.
pub fn read_config_identity(root: &Path) -> (Option<String>, Option<String>) {
    let config_xml = root.join("Configuration.xml");
    if !config_xml.exists() {
        return (None, None);
    }
    let content = match crate::index::read_file_to_string_lossy(&config_xml) {
        Ok(c) => c,
        Err(_) => return (None, None),
    };

    // Extract UUID from <Configuration uuid="...">
    let uuid = Regex::new(r#"<Configuration[^>]+uuid="([^"]+)""#)
        .ok()
        .and_then(|re| re.captures(&content))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string());

    // Extract first <Name>...</Name> before <ChildObjects>
    let search_area = if let Some(child_pos) = content.find("<ChildObjects>") {
        &content[..child_pos]
    } else {
        &content
    };
    let name = Regex::new(r"<Name>([^<\n]{1,200})</Name>")
        .ok()
        .and_then(|re| re.captures(search_area))
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty());

    (name, uuid)
}

/// Build the metadata graph (objects + object_items tables).
///
/// Sources (tried in order):
/// 1. `Configuration.xml` — always present; provides object type + name list
/// 2. `ConfigDumpInfo.xml` — optional; provides attributes, tabular sections, forms, modules
///
/// Returns `(object_count, config_name, onec_uuid)`.
pub fn build_metadata(root: &Path, db_path: &Path) -> Result<(usize, Option<String>, Option<String>), String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Ошибка открытия БД: {}", e))?;

    // Clear existing metadata
    conn.execute("DELETE FROM object_items", []).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM objects", []).map_err(|e| e.to_string())?;

    let mut object_ids: std::collections::HashMap<String, i64> = std::collections::HashMap::new();

    // Read config identity (name + uuid) from Configuration.xml
    let (config_name, onec_uuid) = read_config_identity(root);

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
    } else if !config_dump.exists() && !object_ids.is_empty() {
        // Step 3: No ConfigDumpInfo.xml — parse per-object XML files for attributes/tabular sections
        for (key, &obj_id) in &object_ids {
            let parts: Vec<&str> = key.splitn(2, '.').collect();
            if parts.len() != 2 { continue; }
            let obj_type = parts[0];
            let obj_name = parts[1];
            if let Some(folder) = obj_type_to_folder(obj_type) {
                let xml_path = root.join(folder).join(format!("{}.xml", obj_name));
                if xml_path.exists() {
                    parse_object_xml(&xml_path, &conn, obj_id)
                        .unwrap_or_else(|e| eprintln!("[1c-search] {}.xml: {}", obj_name, e));
                }
            }
        }
    }

    Ok((object_ids.len(), config_name, onec_uuid))
}

/// Parse `<ChildObjects>` section in Configuration.xml.
/// Populates the `objects` table and fills `object_ids` map ("Type.Name" → rowid).
fn parse_configuration_xml(
    path: &Path,
    conn: &Connection,
    object_ids: &mut std::collections::HashMap<String, i64>,
) -> Result<(), String> {
    let content = crate::index::read_file_to_string_lossy(path)
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

/// Map 1C object type → plural folder name used in config dumps.
fn obj_type_to_folder(obj_type: &str) -> Option<&'static str> {
    match obj_type {
        "Catalog"                    => Some("Catalogs"),
        "Document"                   => Some("Documents"),
        "CommonModule"               => Some("CommonModules"),
        "InformationRegister"        => Some("InformationRegisters"),
        "AccumulationRegister"       => Some("AccumulationRegisters"),
        "AccountingRegister"         => Some("AccountingRegisters"),
        "CalculationRegister"        => Some("CalculationRegisters"),
        "ExchangePlan"               => Some("ExchangePlans"),
        "BusinessProcess"            => Some("BusinessProcesses"),
        "Task"                       => Some("Tasks"),
        "ChartOfCharacteristicTypes" => Some("ChartsOfCharacteristicTypes"),
        "ChartOfAccounts"            => Some("ChartsOfAccounts"),
        "ChartOfCalculationTypes"    => Some("ChartsOfCalculationTypes"),
        "DataProcessor"              => Some("DataProcessors"),
        "Report"                     => Some("Reports"),
        "Enum"                       => Some("Enums"),
        "Constant"                   => Some("Constants"),
        "DocumentJournal"            => Some("DocumentJournals"),
        "FilterCriterion"            => Some("FilterCriteria"),
        "ScheduledJob"               => Some("ScheduledJobs"),
        "CommonForm"                 => Some("CommonForms"),
        "CommonAttribute"            => Some("CommonAttributes"),
        "CommonCommand"              => Some("CommonCommands"),
        "Role"                       => Some("Roles"),
        _ => None,
    }
}

/// Parse ConfigDumpInfo.xml: extract `<Metadata name="...">` entries and
/// populate `object_items` (attributes, tabular sections, forms, commands, modules).
fn parse_config_dump_info(
    path: &Path,
    conn: &Connection,
    object_ids: &std::collections::HashMap<String, i64>,
) -> Result<(), String> {
    let content = crate::index::read_file_to_string_lossy(path)
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

/// Parse a per-object XML file (e.g. `Catalogs/Валюты.xml`) and populate
/// `object_items` with attributes, tabular sections and their columns.
///
/// Structure inside top-level `<ChildObjects>`:
/// ```xml
/// <Attribute uuid="..."><Properties><Name>AttrName</Name>...</Properties></Attribute>
/// <TabularSection uuid="...">
///   <Properties><Name>TSName</Name></Properties>
///   <ChildObjects>
///     <Attribute uuid="..."><Properties><Name>ColName</Name>...</Properties></Attribute>
///   </ChildObjects>
/// </TabularSection>
/// ```
fn parse_object_xml(path: &Path, conn: &Connection, obj_id: i64) -> Result<(), String> {
    let content = crate::index::read_file_to_string_lossy(path)
        .map_err(|e| format!("Чтение {}: {}", path.display(), e))?;

    // Find the top-level <ChildObjects> block (after object <Properties>)
    let child_start = match content.find("<ChildObjects>") {
        Some(p) => p + "<ChildObjects>".len(),
        None => return Ok(()),
    };
    let section = &content[child_start..];

    let name_re = Regex::new(r"<Name>([^<\n]{1,200})</Name>").map_err(|e| e.to_string())?;

    conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;

    let mut pos = 0;
    let slen = section.len();

    while pos < slen {
        let rest = &section[pos..];

        if rest.starts_with("</ChildObjects>") {
            break;
        }

        if rest.starts_with("<Attribute ") || rest.starts_with("<Attribute\t") || rest.starts_with("<Attribute\n") {
            if let Some(close_rel) = rest.find("</Attribute>") {
                let block = &rest[..close_rel];
                if let Some(cap) = name_re.captures(block) {
                    let attr_name = cap[1].trim().to_string();
                    if !attr_name.is_empty() {
                        let _ = conn.execute(
                            "INSERT INTO object_items (object_id, item_type, item_name, parent_section) \
                             VALUES (?1, 'Attribute', ?2, NULL)",
                            params![obj_id, attr_name],
                        );
                    }
                }
                pos += close_rel + "</Attribute>".len();
                continue;
            }
        } else if rest.starts_with("<TabularSection ") || rest.starts_with("<TabularSection\t") || rest.starts_with("<TabularSection\n") {
            if let Some(close_rel) = rest.find("</TabularSection>") {
                let ts_block = &rest[..close_rel];

                // Extract TS name from <Properties> (before nested <ChildObjects>)
                let props_end = ts_block.find("<ChildObjects>").unwrap_or(ts_block.len().min(600));
                let ts_name = name_re.captures(&ts_block[..props_end])
                    .map(|c| c[1].trim().to_string())
                    .unwrap_or_default();

                if !ts_name.is_empty() {
                    let _ = conn.execute(
                        "INSERT INTO object_items (object_id, item_type, item_name, parent_section) \
                         VALUES (?1, 'TabularSection', ?2, NULL)",
                        params![obj_id, &ts_name],
                    );

                    // Parse column attributes inside TS's <ChildObjects>
                    if let Some(ts_child_start) = ts_block.find("<ChildObjects>") {
                        let ts_children = &ts_block[ts_child_start + "<ChildObjects>".len()..];
                        let mut ts_pos = 0;
                        while ts_pos < ts_children.len() {
                            let ts_rest = &ts_children[ts_pos..];
                            if ts_rest.starts_with("</ChildObjects>") { break; }
                            if ts_rest.starts_with("<Attribute ") || ts_rest.starts_with("<Attribute\t") {
                                if let Some(col_close) = ts_rest.find("</Attribute>") {
                                    let col_block = &ts_rest[..col_close];
                                    if let Some(col_cap) = name_re.captures(col_block) {
                                        let col_name = col_cap[1].trim().to_string();
                                        if !col_name.is_empty() {
                                            let _ = conn.execute(
                                                "INSERT INTO object_items (object_id, item_type, item_name, parent_section) \
                                                 VALUES (?1, 'Attribute', ?2, ?3)",
                                                params![obj_id, col_name, &ts_name],
                                            );
                                        }
                                    }
                                    ts_pos += col_close + "</Attribute>".len();
                                    continue;
                                }
                            }
                            ts_pos += 1;
                        }
                    }
                }
                pos += close_rel + "</TabularSection>".len();
                continue;
            }
        }

        // Advance past current '<'
        if let Some(next) = rest[1..].find('<') {
            pos += next + 1;
        } else {
            break;
        }
    }

    conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
    Ok(())
}

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigExtensionProfile {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSearchProfile {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub main_path: String,
    #[serde(default)]
    pub extensions: Vec<ConfigExtensionProfile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchRoot {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SearchWorkspace {
    pub active_profile_id: Option<String>,
    pub roots: Vec<SearchRoot>,
}

impl SearchWorkspace {
    pub fn from_env() -> Self {
        let legacy_path = std::env::var("ONEC_CONFIG_PATH").unwrap_or_default();
        let active_profile_id = std::env::var("ONEC_CONFIG_ACTIVE_PROFILE_ID")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let profiles_json = std::env::var("ONEC_CONFIG_PROFILES_JSON").unwrap_or_default();

        if let Ok(profiles) = serde_json::from_str::<Vec<ConfigSearchProfile>>(&profiles_json) {
            if let Some(workspace) = Self::from_profiles(profiles, active_profile_id.clone()) {
                return workspace;
            }
        }

        Self::from_legacy_path(&legacy_path)
    }

    pub fn from_legacy_path(path: &str) -> Self {
        let path = path.trim();
        let roots = if path.is_empty() {
            Vec::new()
        } else {
            vec![SearchRoot {
                id: "main".to_string(),
                name: "Основная конфигурация".to_string(),
                kind: "main".to_string(),
                path: PathBuf::from(path),
            }]
        };

        Self {
            active_profile_id: None,
            roots,
        }
    }

    fn from_profiles(
        profiles: Vec<ConfigSearchProfile>,
        active_profile_id: Option<String>,
    ) -> Option<Self> {
        let selected = active_profile_id
            .as_ref()
            .and_then(|id| profiles.iter().find(|p| p.id == *id))
            .or_else(|| profiles.iter().find(|p| !p.main_path.trim().is_empty()))?;

        let profile_id = stable_id(&selected.id, &selected.name, &selected.main_path, "profile");
        let profile_name = if selected.name.trim().is_empty() {
            "Активная конфигурация".to_string()
        } else {
            selected.name.trim().to_string()
        };

        let mut roots = Vec::new();
        if !selected.main_path.trim().is_empty() {
            roots.push(SearchRoot {
                id: format!("{}:main", profile_id),
                name: profile_name.clone(),
                kind: "main".to_string(),
                path: PathBuf::from(selected.main_path.trim()),
            });
        }

        for (idx, ext) in selected.extensions.iter().enumerate() {
            if ext.path.trim().is_empty() {
                continue;
            }
            let ext_id = stable_id(
                &ext.id,
                &ext.name,
                &ext.path,
                &format!("ext{}", idx + 1),
            );
            let ext_name = if ext.name.trim().is_empty() {
                format!("Расширение {}", idx + 1)
            } else {
                ext.name.trim().to_string()
            };
            roots.push(SearchRoot {
                id: format!("{}:{}", profile_id, ext_id),
                name: ext_name,
                kind: "extension".to_string(),
                path: PathBuf::from(ext.path.trim()),
            });
        }

        if roots.is_empty() {
            return None;
        }

        Some(Self {
            active_profile_id: Some(profile_id),
            roots,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    pub fn primary_root(&self) -> Option<&SearchRoot> {
        self.roots.first()
    }

    pub fn root_by_id(&self, source_id: &str) -> Option<&SearchRoot> {
        self.roots.iter().find(|root| root.id == source_id)
    }

    pub fn find_root_for_file(&self, rel_file: &str) -> Option<&SearchRoot> {
        let normalized = rel_file.replace('\\', "/");
        self.roots.iter().find(|root| {
            root.path
                .join(normalized.replace('/', std::path::MAIN_SEPARATOR_STR))
                .exists()
        })
    }

    pub fn root_for_args(&self, args: &serde_json::Value) -> Option<&SearchRoot> {
        args["source_id"]
            .as_str()
            .and_then(|id| self.root_by_id(id))
            .or_else(|| args["file"].as_str().and_then(|file| self.find_root_for_file(file)))
            .or_else(|| self.primary_root())
    }
}

pub fn root_db_path(root: &SearchRoot) -> PathBuf {
    crate::index::get_db_path(&root.path)
}

pub fn root_exists(root: &SearchRoot) -> bool {
    root.path.exists() && root.path.is_dir()
}

pub fn source_json(root: &SearchRoot) -> serde_json::Value {
    serde_json::json!({
        "source_id": root.id,
        "source_name": root.name,
        "source_kind": root.kind,
        "source_path": root.path.to_string_lossy()
    })
}

fn stable_id(id: &str, name: &str, path: &str, fallback: &str) -> String {
    let raw = [id.trim(), name.trim(), path.trim()]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or(fallback);
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == '_' {
            out.push(ch);
        }
    }
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_path_creates_single_main_root() {
        let ws = SearchWorkspace::from_legacy_path("D:\\cfg\\erp");
        assert_eq!(ws.roots.len(), 1);
        assert_eq!(ws.roots[0].id, "main");
        assert_eq!(ws.roots[0].kind, "main");
    }

    #[test]
    fn profiles_json_creates_main_and_extensions() {
        let profiles = vec![ConfigSearchProfile {
            id: "erp".to_string(),
            name: "ERP".to_string(),
            main_path: "D:\\cfg\\erp".to_string(),
            extensions: vec![ConfigExtensionProfile {
                id: "sales".to_string(),
                name: "Sales".to_string(),
                path: "D:\\cfg\\erp-sales".to_string(),
            }],
        }];
        let ws = SearchWorkspace::from_profiles(profiles, Some("erp".to_string())).unwrap();
        assert_eq!(ws.roots.len(), 2);
        assert_eq!(ws.roots[0].id, "erp:main");
        assert_eq!(ws.roots[1].id, "erp:sales");
    }
}

use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};

pub const WIFI_ENV_KEYS: [&str; 7] = [
    "MAINS_AEGIS_WIFI_SSID",
    "MAINS_AEGIS_WIFI_PSK",
    "MAINS_AEGIS_WIFI_HOSTNAME",
    "MAINS_AEGIS_WIFI_STATIC_IP",
    "MAINS_AEGIS_WIFI_NETMASK",
    "MAINS_AEGIS_WIFI_GATEWAY",
    "MAINS_AEGIS_WIFI_DNS",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiEnvConfig {
    pub values: HashMap<String, String>,
}

impl WifiEnvConfig {
    pub fn from_sources(repo_root: Option<&Path>) -> Self {
        let mut values = HashMap::new();
        if let Some(repo_root) = repo_root {
            for path in candidate_env_paths(repo_root) {
                if path.exists() {
                    values.extend(load_env_file(&path));
                }
            }
        }

        for key in WIFI_ENV_KEYS {
            if let Ok(value) = env::var(key) {
                if !value.trim().is_empty() {
                    values.insert(key.to_string(), value);
                }
            }
        }

        Self { values }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(|value| value.as_str())
    }

    pub fn require_station_credentials(&self) -> Result<(&str, &str), String> {
        let ssid = self
            .get("MAINS_AEGIS_WIFI_SSID")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                "missing MAINS_AEGIS_WIFI_SSID (set it in .env or environment)".to_string()
            })?;
        let psk = self
            .get("MAINS_AEGIS_WIFI_PSK")
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                "missing MAINS_AEGIS_WIFI_PSK (set it in .env or environment)".to_string()
            })?;
        Ok((ssid, psk))
    }
}

pub fn candidate_env_paths(repo_root: &Path) -> [PathBuf; 2] {
    [repo_root.join(".env"), repo_root.join("firmware/.env")]
}

pub fn load_env_file(path: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(contents) = fs::read_to_string(path) else {
        return out;
    };

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };

        let key = raw_key.trim();
        if key.is_empty() {
            continue;
        }

        let mut value = raw_value.trim().to_string();
        if let Some(stripped) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            value = stripped.to_string();
        } else if let Some(stripped) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
            value = stripped.to_string();
        }

        out.insert(key.to_string(), value);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{candidate_env_paths, load_env_file, WifiEnvConfig};
    use std::{collections::HashMap, fs, path::PathBuf};

    fn temp_env_root() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mains-aegis-wifi-env-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn candidate_env_paths_cover_repo_and_firmware() {
        let root = PathBuf::from("/tmp/mains-aegis");
        let paths = candidate_env_paths(&root);
        assert_eq!(paths[0], PathBuf::from("/tmp/mains-aegis/.env"));
        assert_eq!(paths[1], PathBuf::from("/tmp/mains-aegis/firmware/.env"));
    }

    #[test]
    fn load_env_file_strips_quotes_and_comments() {
        let root = temp_env_root();
        let env_path = root.join(".env");
        fs::write(
            &env_path,
            r#"
# ignored
MAINS_AEGIS_WIFI_SSID="lab-ap"
MAINS_AEGIS_WIFI_PSK='test-psk'
MAINS_AEGIS_WIFI_DNS=1.1.1.1
INVALID_LINE
"#,
        )
        .unwrap();

        let values = load_env_file(&env_path);
        assert_eq!(
            values.get("MAINS_AEGIS_WIFI_SSID"),
            Some(&"lab-ap".to_string())
        );
        assert_eq!(
            values.get("MAINS_AEGIS_WIFI_PSK"),
            Some(&"test-psk".to_string())
        );
        assert_eq!(
            values.get("MAINS_AEGIS_WIFI_DNS"),
            Some(&"1.1.1.1".to_string())
        );
        assert!(!values.contains_key("INVALID_LINE"));
    }

    #[test]
    fn require_station_credentials_needs_ssid_and_psk() {
        let mut values = HashMap::new();
        values.insert("MAINS_AEGIS_WIFI_SSID".to_string(), "ups-lab".to_string());
        let cfg = WifiEnvConfig { values };
        assert!(cfg.require_station_credentials().is_err());

        let mut values = HashMap::new();
        values.insert("MAINS_AEGIS_WIFI_SSID".to_string(), "ups-lab".to_string());
        values.insert("MAINS_AEGIS_WIFI_PSK".to_string(), "secret".to_string());
        let cfg = WifiEnvConfig { values };
        assert_eq!(cfg.require_station_credentials(), Ok(("ups-lab", "secret")));
    }
}

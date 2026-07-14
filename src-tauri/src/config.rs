//! Versioned `config.json` with atomic writes, validation, and unknown-field retention.

use crate::paths;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("config parse error: {0}")]
    Parse(String),
    #[error("config validation error: {0}")]
    Validation(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LanguagePref {
    #[default]
    System,
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en")]
    En,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TrayMode {
    #[default]
    Off,
    Active,
    Always,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeneralConfig {
    pub language: LanguagePref,
    pub theme: ThemePref,
    pub tray_mode: TrayMode,
    pub history_limit: u32,
    pub max_concurrent_tasks: u32,
    pub task_timeout_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grok_executable: Option<String>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            language: LanguagePref::System,
            theme: ThemePref::System,
            tray_mode: TrayMode::Off,
            history_limit: 200,
            max_concurrent_tasks: 3,
            task_timeout_seconds: 14_400,
            grok_executable: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiConfig {
    pub popover_width: u32,
    pub popover_height: u32,
    pub show_diagnostics: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            popover_width: 420,
            popover_height: 620,
            show_diagnostics: false,
        }
    }
}

/// Fully validated application config (known fields only).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub schema_version: u32,
    pub general: GeneralConfig,
    pub ui: UiConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            general: GeneralConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

/// Loaded config with unknown top-level / nested fields preserved for round-trip.
#[derive(Debug, Clone)]
pub struct ConfigDocument {
    pub config: AppConfig,
    /// Full JSON object as last successfully parsed (includes unknowns).
    raw: Map<String, Value>,
}

impl Default for ConfigDocument {
    fn default() -> Self {
        let config = AppConfig::default();
        let raw = serde_json::to_value(&config)
            .expect("default config serializes")
            .as_object()
            .cloned()
            .unwrap_or_default();
        Self { config, raw }
    }
}

impl ConfigDocument {
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from(&paths::config_file())
    }

    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut file = File::open(path)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        Self::parse_str(&buf)
    }

    pub fn parse_str(text: &str) -> Result<Self, ConfigError> {
        let value: Value = serde_json::from_str(text)
            .map_err(|e| ConfigError::Parse(format!("invalid JSON: {e}")))?;
        let obj = value
            .as_object()
            .cloned()
            .ok_or_else(|| ConfigError::Parse("config root must be a JSON object".into()))?;

        // Merge known fields with defaults; unknown keys stay in `raw`.
        let mut merged = serde_json::to_value(AppConfig::default())
            .map_err(|e| ConfigError::Parse(e.to_string()))?;
        deep_merge(&mut merged, &Value::Object(obj.clone()));

        // serde ignores unknown fields by default; nested unknowns remain only in `raw`.
        let config: AppConfig = serde_json::from_value(merged)
            .map_err(|e| ConfigError::Parse(format!("schema error: {e}")))?;
        config.validate()?;

        // Rebuild raw: start from file object (top-level + nested unknowns retained),
        // then deep-merge validated known fields so canonical values win without
        // clobbering nested unknown keys (e.g. general.experimentalFlag).
        let mut raw_value = Value::Object(obj);
        let known = serde_json::to_value(&config).map_err(|e| ConfigError::Parse(e.to_string()))?;
        deep_merge(&mut raw_value, &known);
        let raw = raw_value
            .as_object()
            .cloned()
            .ok_or_else(|| ConfigError::Parse("config raw must be object".into()))?;

        Ok(Self { config, raw })
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        self.save_to(&paths::config_file())
    }

    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        self.config.validate()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
            }
        }

        // Deep-merge known config into preserved raw so nested unknowns survive.
        let mut out_value = Value::Object(self.raw.clone());
        let known =
            serde_json::to_value(&self.config).map_err(|e| ConfigError::Parse(e.to_string()))?;
        deep_merge(&mut out_value, &known);
        let pretty = serde_json::to_string_pretty(&out_value)
            .map_err(|e| ConfigError::Parse(e.to_string()))?;
        atomic_write(path, pretty.as_bytes())?;
        Ok(())
    }

    /// Replace known config while retaining previously preserved unknown keys.
    pub fn update_config(&mut self, config: AppConfig) -> Result<(), ConfigError> {
        config.validate()?;
        self.config = config;
        Ok(())
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version == 0 {
            return Err(ConfigError::Validation("schemaVersion must be >= 1".into()));
        }
        if self.general.history_limit > 5000 {
            return Err(ConfigError::Validation(
                "historyLimit must be 0–5000".into(),
            ));
        }
        if !(1..=8).contains(&self.general.max_concurrent_tasks) {
            return Err(ConfigError::Validation(
                "maxConcurrentTasks must be 1–8".into(),
            ));
        }
        if !(300..=86_400).contains(&self.general.task_timeout_seconds) {
            return Err(ConfigError::Validation(
                "taskTimeoutSeconds must be 300–86400".into(),
            ));
        }
        if let Some(ref path) = self.general.grok_executable {
            let p = PathBuf::from(path);
            if !p.is_absolute() {
                return Err(ConfigError::Validation(
                    "grokExecutable must be null or an absolute path".into(),
                ));
            }
            if !p.is_file() {
                return Err(ConfigError::Validation(format!(
                    "grokExecutable does not exist: {path}"
                )));
            }
        }
        if !(360..=520).contains(&self.ui.popover_width) {
            return Err(ConfigError::Validation(
                "popoverWidth must be 360–520".into(),
            ));
        }
        if !(420..=760).contains(&self.ui.popover_height) {
            return Err(ConfigError::Validation(
                "popoverHeight must be 420–760".into(),
            ));
        }
        Ok(())
    }
}

/// Near-atomic write: temp file in same directory → fsync → replace destination.
///
/// - **Unix:** `rename(2)` replaces the destination atomically.
/// - **Windows:** `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` replaces without
///   deleting the destination first. On failure the original file is left intact
///   (temp is cleaned up best-effort). Not a full transactional NTFS replace, but
///   never leaves the destination missing due to a failed mid-replace.
pub fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("config"),
        std::process::id()
    ));
    {
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
        fs::rename(&tmp, path)?;
        // Best-effort directory fsync.
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(windows)]
    {
        replace_file_windows(&tmp, path)?;
    }
    #[cfg(not(any(unix, windows)))]
    {
        // Best-effort: do not delete destination first (may fail if exists).
        fs::rename(&tmp, path)?;
    }
    Ok(())
}

/// Platform replace strategy label (for tests / diagnostics).
pub fn atomic_write_strategy() -> &'static str {
    #[cfg(unix)]
    {
        "unix-rename"
    }
    #[cfg(windows)]
    {
        "windows-movefileex-replace"
    }
    #[cfg(not(any(unix, windows)))]
    {
        "fallback-rename"
    }
}

#[cfg(windows)]
fn replace_file_windows(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    fn wide(p: &Path) -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    // Fast path when destination is absent — plain rename is fine.
    if !dest.exists() {
        return fs::rename(tmp, dest);
    }

    let from = wide(tmp);
    let to = wide(dest);
    // REPLACE without prior delete: on failure the destination remains.
    let ok = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        let err = std::io::Error::last_os_error();
        let _ = fs::remove_file(tmp);
        return Err(err);
    }
    Ok(())
}

/// Deep-merge `src` into `dst`. Object keys from `src` overwrite matching leaves
/// in `dst`, but keys only present in `dst` are preserved (unknown-field retention).
fn deep_merge(dst: &mut Value, src: &Value) {
    match (dst, src) {
        (Value::Object(d), Value::Object(s)) => {
            for (k, v) in s {
                deep_merge(d.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
        (dst, src) => {
            *dst = src.clone();
        }
    }
}

/// Shared in-memory config handle with last-known-good semantics for reload.
#[derive(Debug, Clone)]
pub struct ConfigHandle {
    inner: std::sync::Arc<parking_lot::RwLock<ConfigDocument>>,
}

impl ConfigHandle {
    pub fn new(doc: ConfigDocument) -> Self {
        Self {
            inner: std::sync::Arc::new(parking_lot::RwLock::new(doc)),
        }
    }

    pub fn load_default() -> Result<Self, ConfigError> {
        Ok(Self::new(ConfigDocument::load()?))
    }

    pub fn snapshot(&self) -> AppConfig {
        self.inner.read().config.clone()
    }

    pub fn document(&self) -> ConfigDocument {
        self.inner.read().clone()
    }

    /// Attempt reload from disk. Invalid configs leave last-known-good untouched.
    pub fn reload_from(&self, path: &Path) -> Result<AppConfig, ConfigError> {
        let doc = ConfigDocument::load_from(path)?;
        let cfg = doc.config.clone();
        *self.inner.write() = doc;
        Ok(cfg)
    }

    pub fn replace(&self, doc: ConfigDocument) {
        *self.inner.write() = doc;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn defaults_validate() {
        AppConfig::default().validate().unwrap();
    }

    #[test]
    fn missing_file_yields_defaults() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("missing.json");
        let doc = ConfigDocument::load_from(&path).unwrap();
        assert_eq!(doc.config.general.history_limit, 200);
        assert_eq!(doc.config.general.tray_mode, TrayMode::Off);
    }

    #[test]
    fn unknown_fields_retained() {
        let text = r#"{
            "schemaVersion": 1,
            "general": { "historyLimit": 100, "experimentalFlag": true },
            "ui": { "popoverWidth": 420, "mysteryUiKnob": 7 },
            "futureTop": { "a": 1 }
        }"#;
        let doc = ConfigDocument::parse_str(text).unwrap();
        assert_eq!(doc.config.general.history_limit, 100);
        assert!(doc.raw.contains_key("futureTop"));
        // Nested unknowns under known sections must survive parse.
        let general = doc.raw.get("general").and_then(|v| v.as_object()).unwrap();
        assert_eq!(general.get("experimentalFlag"), Some(&Value::Bool(true)));
        let ui = doc.raw.get("ui").and_then(|v| v.as_object()).unwrap();
        assert_eq!(ui.get("mysteryUiKnob"), Some(&Value::Number(7.into())));

        // Nested unknowns must survive save → reload.
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.json");
        doc.save_to(&path).unwrap();
        let reloaded_text = fs::read_to_string(&path).unwrap();
        assert!(reloaded_text.contains("futureTop"));
        assert!(reloaded_text.contains("experimentalFlag"));
        assert!(reloaded_text.contains("mysteryUiKnob"));

        let reloaded = ConfigDocument::load_from(&path).unwrap();
        assert_eq!(reloaded.config.general.history_limit, 100);
        let general = reloaded
            .raw
            .get("general")
            .and_then(|v| v.as_object())
            .unwrap();
        assert_eq!(general.get("experimentalFlag"), Some(&Value::Bool(true)));
        let ui = reloaded.raw.get("ui").and_then(|v| v.as_object()).unwrap();
        assert_eq!(ui.get("mysteryUiKnob"), Some(&Value::Number(7.into())));
        assert!(reloaded.raw.contains_key("futureTop"));
    }

    #[test]
    fn invalid_json_errors_without_writing() {
        let err = ConfigDocument::parse_str("{not json").unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)));
    }

    #[test]
    fn invalid_history_limit() {
        let mut cfg = AppConfig::default();
        cfg.general.history_limit = 9000;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn invalid_max_concurrent() {
        let mut cfg = AppConfig::default();
        cfg.general.max_concurrent_tasks = 0;
        assert!(cfg.validate().is_err());
        cfg.general.max_concurrent_tasks = 9;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn invalid_timeout() {
        let mut cfg = AppConfig::default();
        cfg.general.task_timeout_seconds = 10;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn atomic_write_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.json");
        let mut doc = ConfigDocument::default();
        doc.config.general.history_limit = 50;
        doc.save_to(&path).unwrap();
        let loaded = ConfigDocument::load_from(&path).unwrap();
        assert_eq!(loaded.config.general.history_limit, 50);
    }

    #[test]
    fn atomic_write_replaces_without_gap() {
        // Strategy is platform-specific; both must leave a valid dest after replace.
        assert!(matches!(
            atomic_write_strategy(),
            "unix-rename" | "windows-movefileex-replace" | "fallback-rename"
        ));
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.json");
        atomic_write(&path, b"first").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first");
        // Overwrite existing: destination must still exist after success.
        atomic_write(&path, b"second").unwrap();
        assert!(path.exists(), "destination must exist after replace");
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
        // Temp siblings must not be left behind on success.
        let leftovers: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with(".config.json.tmp-")
            })
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files left after atomic_write: {leftovers:?}"
        );
    }

    #[test]
    fn reload_rejects_invalid_keeps_last_good() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.json");
        let doc = ConfigDocument::default();
        doc.save_to(&path).unwrap();
        let handle = ConfigHandle::new(doc);
        fs::write(&path, b"{bad").unwrap();
        assert!(handle.reload_from(&path).is_err());
        assert_eq!(handle.snapshot().general.history_limit, 200);
    }

    #[test]
    fn grok_executable_must_be_absolute_existing() {
        let mut cfg = AppConfig::default();
        cfg.general.grok_executable = Some("relative/path".into());
        assert!(cfg.validate().is_err());
        cfg.general.grok_executable = Some("/no/such/grok-binary-xyz".into());
        assert!(cfg.validate().is_err());
    }
}

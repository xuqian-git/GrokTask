//! Unified version and product identity.

/// Application display name / executable product name.
pub const PRODUCT_NAME: &str = "GrokTask";

/// Reverse-DNS bundle identifier (must match tauri.conf.json).
pub const APP_IDENTIFIER: &str = "ai.x.groktask";

/// Semantic version — keep in sync with package.json and tauri.conf.json / Cargo.toml.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// IPC protocol major version negotiated in hello handshake.
pub const PROTOCOL_VERSION: u32 = 1;

/// Window labels used by the GUI host.
pub mod window_label {
    pub const MAIN: &str = "main";
    pub const POPOVER: &str = "popover";
    pub const SETTINGS: &str = "settings";
    pub const HISTORY: &str = "history";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_semver_like() {
        let parts: Vec<_> = APP_VERSION.split('.').collect();
        assert!(parts.len() >= 2);
        assert!(parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())));
    }
}

//! MCP stdio role skeleton (Phase 3 implements rmcp tools).
//!
//! Critical Phase 0–1 invariant: this path never initializes Tauri/WebView.

use crate::cli::eprint_line;
use crate::version::{APP_VERSION, PRODUCT_NAME};

/// Run MCP server on stdio. Phase 0–1: announce readiness on stderr and exit 0
/// after a clean JSON-RPC style no-op so role dispatch is verifiable without Tauri.
pub fn run_stdio() -> ! {
    // Logs only on stderr — stdout reserved for MCP framing in Phase 3.
    eprint_line(&format!(
        "{PRODUCT_NAME} mcp {APP_VERSION}: MCP tools land in Phase 3; role is active without GUI"
    ));
    // Exit success so smoke tests can assert no GUI init. Real MCP handshake is Phase 3.
    std::process::exit(0);
}

//! GrokTask multi-role binary entry.
//!
//! Roles (same executable):
//! - CLI / help / version — no Tauri
//! - `mcp` — no Tauri
//! - `daemon run` — no Tauri
//! - `--task-supervisor` — no Tauri
//! - `--gui-host` — Tauri event loop only
//!
//! Intentionally no `windows_subsystem = "windows"` so CLI/MCP keep a console.

#![allow(dead_code)] // Phase 0–1 foundations expose APIs used fully in later phases.

mod acp;
mod app;
mod cli;
mod config;
mod daemon;
mod dto;
mod fingerprint;
mod ipc;
mod mcp;
mod paths;
mod storage;
mod supervisor;
mod version;

fn main() {
    // Role dispatch happens before any Tauri / GUI initialization.
    cli::dispatch();
}

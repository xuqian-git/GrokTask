//! CLI role dispatch. GUI/Tauri is only entered for explicit GUI roles.

pub mod help;

use std::io::Write;
use std::process::exit;

/// Print a line to stdout; ignore BrokenPipe so `… | head` exits cleanly.
pub fn print_line(text: &str) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{text}").and_then(|_| out.flush());
}

/// Print to stderr (diagnostics / warnings). Never pollute stdout for MCP/JSON roles.
pub fn eprint_line(text: &str) {
    let mut err = std::io::stderr();
    let _ = writeln!(err, "{text}").and_then(|_| err.flush());
}

/// Top-level argv dispatch. Called from `main` before any Tauri initialization.
pub fn dispatch() {
    let argv: Vec<String> = std::env::args().collect();

    if argv.len() < 2 {
        eprint_line("error: missing command");
        eprint_line(&format!("see `{} --help`", help::program_name()));
        print_line(&help::help_text());
        exit(1);
    }

    match argv[1].as_str() {
        "--help" | "-h" | "help" => {
            print_line(&help::help_text());
            exit(0);
        }
        "--version" | "-v" | "version" => {
            print_line(&help::version_text());
            exit(0);
        }
        "mcp" => {
            // Phase 3 implements rmcp tools. Phase 0–1: role exists, never starts Tauri.
            crate::mcp::run_stdio();
        }
        "daemon" => {
            daemon_dispatch(&argv[2..]);
        }
        "--gui-host" => {
            crate::app::gui_host::run();
        }
        "--task-supervisor" => {
            crate::supervisor::run(&argv[2..]);
        }
        "app" | "setup" | "doctor" | "run" | "start" | "status" | "wait" | "cancel" | "tasks"
        | "agents" => {
            // Phase 3+ implements full CLI. Phase 0–1: validate role isolation only.
            eprint_line(&format!(
                "{}: command `{}` is reserved; full CLI lands in Phase 3",
                crate::version::PRODUCT_NAME,
                argv[1]
            ));
            // Still ensure we never init Tauri for these roles in this phase.
            exit(1);
        }
        other => {
            eprint_line(&format!("error: unknown command `{other}`"));
            eprint_line(&format!("see `{} --help`", help::program_name()));
            exit(1);
        }
    }
}

fn daemon_dispatch(args: &[String]) {
    if args.is_empty() {
        eprint_line("error: daemon requires a subcommand (run|start|stop|restart|status|logs)");
        exit(1);
    }
    match args[0].as_str() {
        "run" => {
            if let Err(e) = crate::daemon::run_foreground() {
                eprint_line(&format!("daemon error: {e:#}"));
                exit(3);
            }
        }
        "start" => {
            if let Err(e) = crate::daemon::start_detached() {
                eprint_line(&format!("daemon start error: {e:#}"));
                exit(3);
            }
            print_line("daemon starting");
            exit(0);
        }
        "stop" => {
            if let Err(e) = crate::daemon::stop() {
                eprint_line(&format!("daemon stop error: {e:#}"));
                exit(3);
            }
            print_line("daemon stopped");
            exit(0);
        }
        "restart" => {
            let force = args.iter().any(|a| a == "--force");
            if let Err(e) = crate::daemon::restart(force) {
                eprint_line(&format!("daemon restart error: {e:#}"));
                exit(3);
            }
            print_line("daemon restart requested");
            exit(0);
        }
        "status" => match crate::daemon::status_text() {
            Ok(text) => {
                print_line(&text);
                exit(0);
            }
            Err(e) => {
                eprint_line(&format!("daemon status error: {e:#}"));
                exit(3);
            }
        },
        "logs" => {
            let path = crate::paths::daemon_log();
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    print_line(&content);
                    exit(0);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    print_line("(no daemon log yet)");
                    exit(0);
                }
                Err(e) => {
                    eprint_line(&format!("daemon logs error: {e}"));
                    exit(1);
                }
            }
        }
        other => {
            eprint_line(&format!("error: unknown daemon subcommand `{other}`"));
            exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_mentions_mcp_and_daemon() {
        let h = help::help_text();
        assert!(h.contains("mcp"));
        assert!(h.contains("daemon run"));
        assert!(!h.contains("--gui-host")); // hidden internal entry
    }

    #[test]
    fn version_non_empty() {
        assert!(!help::version_text().is_empty());
    }
}

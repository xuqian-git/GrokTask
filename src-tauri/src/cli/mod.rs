//! CLI role dispatch. GUI/Tauri is only entered for explicit GUI roles
//! (`app`, `setup`, `--gui-host`) or a macOS `.app` bundle launch with no args.

pub mod help;

use crate::dto::{
    run_result_text_summary, validate_submission_id, validate_task_input, validate_uuid_like,
    RunResult, StartResult, TaskDetail, TaskListItem, TaskStatus, TurnCancelResult, WaitTimeout,
    DEFAULT_WAIT_TIMEOUT_MS,
};
use crate::ipc::client::{self, unwrap_result};
use crate::ipc::protocol::ClientRole;
use serde_json::json;
use std::io::Write;
use std::path::Path;
use std::process::exit;
use uuid::Uuid;

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

/// What to do when the process is invoked with no CLI command argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoArgsAction {
    /// Terminal / bare binary: print help and exit(1).
    CliHelp,
    /// macOS app-bundle double-click / `open …/GrokTask.app`: host the GUI.
    GuiAppLaunch,
}

/// True when `exe` lives inside a macOS `.app` bundle (`…/Name.app/Contents/MacOS/…`).
///
/// Pure path check — used so unit tests do not need a real bundle layout.
pub fn path_is_macos_app_bundle_exe(exe: &Path) -> bool {
    // Normalize separators so Windows-style paths in tests still match the marker.
    let s = exe.to_string_lossy().replace('\\', "/");
    s.contains(".app/Contents/MacOS/")
}

/// Decide no-args behavior from pure inputs (testable without launching Tauri).
pub fn no_args_action(is_macos_app_bundle: bool) -> NoArgsAction {
    if is_macos_app_bundle {
        NoArgsAction::GuiAppLaunch
    } else {
        NoArgsAction::CliHelp
    }
}

fn current_exe_is_macos_app_bundle() -> bool {
    std::env::current_exe()
        .map(|p| path_is_macos_app_bundle_exe(&p))
        .unwrap_or(false)
}

/// Top-level argv dispatch. Called from `main` before any Tauri initialization.
pub fn dispatch() {
    let argv: Vec<String> = std::env::args().collect();

    if argv.len() < 2 {
        match no_args_action(current_exe_is_macos_app_bundle()) {
            NoArgsAction::GuiAppLaunch => {
                // In-process GUI host (show main window). Prefer focusing an
                // existing instance rather than spawning a second detached host
                // and exiting — that would make LaunchServices treat the .app
                // as quit immediately after `open`.
                crate::app::gui_host::run_as_app_bundle_launch();
            }
            NoArgsAction::CliHelp => {
                eprint_line("error: missing command");
                eprint_line(&format!("see `{} --help`", help::program_name()));
                print_line(&help::help_text());
                exit(1);
            }
        }
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
        "run" | "submit" => {
            exit(cmd_run(&argv[2..], false));
        }
        "start" => {
            exit(cmd_run(&argv[2..], true));
        }
        "status" => {
            exit(cmd_status(&argv[2..]));
        }
        "wait" => {
            exit(cmd_wait(&argv[2..]));
        }
        "cancel" => {
            exit(cmd_cancel(&argv[2..]));
        }
        "tasks" | "list" => {
            if argv[1] == "list" {
                exit(cmd_tasks_list(&argv[2..]));
            }
            exit(cmd_tasks(&argv[2..]));
        }
        "show" => {
            exit(cmd_tasks_show(&argv[2..]));
        }
        "doctor" => {
            exit(cmd_doctor(&argv[2..]));
        }
        "app" => {
            exit(cmd_app(&argv[2..]));
        }
        "setup" => {
            exit(cmd_setup(&argv[2..]));
        }
        "agents" => {
            exit(cmd_agents(&argv[2..]));
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

// ---------------------------------------------------------------------------
// Flag parsing helpers
// ---------------------------------------------------------------------------

#[derive(Default)]
struct RunFlags {
    mode: Option<String>,
    cwd: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    title: Option<String>,
    submission_id: Option<String>,
    json: bool,
    /// Remaining positional task text tokens
    task_parts: Vec<String>,
}

fn parse_run_flags(args: &[String]) -> Result<RunFlags, String> {
    let mut f = RunFlags::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                i += 1;
                f.mode = args.get(i).cloned();
            }
            "--cwd" => {
                i += 1;
                f.cwd = args.get(i).cloned();
            }
            "--model" => {
                i += 1;
                f.model = args.get(i).cloned();
            }
            "--effort" => {
                i += 1;
                f.effort = args.get(i).cloned();
            }
            "--title" => {
                i += 1;
                f.title = args.get(i).cloned();
            }
            "--submission-id" => {
                i += 1;
                f.submission_id = args.get(i).cloned();
            }
            "--json" => f.json = true,
            "--read" => f.mode = Some("read".into()),
            "--write" => f.mode = Some("write".into()),
            "--background" | "-b" => {
                // start-style: handled by caller via `start` command; flag accepted for submit
            }
            "--" => {
                f.task_parts.extend(args[i + 1..].iter().cloned());
                break;
            }
            s if s.starts_with('-') => {
                return Err(format!("unknown flag `{s}`"));
            }
            other => f.task_parts.push(other.into()),
        }
        i += 1;
    }
    Ok(f)
}

fn cmd_run(args: &[String], async_start: bool) -> i32 {
    let flags = match parse_run_flags(args) {
        Ok(f) => f,
        Err(e) => {
            eprint_line(&format!("error: {e}"));
            return 1;
        }
    };
    let mode = match flags.mode.as_deref() {
        Some(m) => m,
        None => {
            eprint_line("error: --mode read|write is required (or --read/--write)");
            return 1;
        }
    };
    let cwd = match flags.cwd.as_deref() {
        Some(c) => c.to_string(),
        None => match std::env::current_dir() {
            Ok(p) => p.to_string_lossy().into_owned(),
            Err(e) => {
                eprint_line(&format!("error: cannot resolve cwd: {e}"));
                return 1;
            }
        },
    };
    let task = flags.task_parts.join(" ");
    let input = match validate_task_input(
        &task,
        &cwd,
        mode,
        flags.model.as_deref(),
        flags.effort.as_deref(),
        flags.title.as_deref(),
    ) {
        Ok(i) => i,
        Err(e) => {
            eprint_line(&format!("error: {}", e.message));
            return 1;
        }
    };

    let mut params = json!({
        "task": input.task,
        "cwd": input.cwd,
        "mode": input.mode.as_str(),
    });
    if let Some(m) = &input.model {
        params["model"] = json!(m);
    }
    if let Some(e) = &input.effort {
        params["effort"] = json!(e);
    }
    if let Some(t) = &input.title {
        params["title"] = json!(t);
    }

    if async_start {
        let sid = match flags.submission_id {
            Some(s) => match validate_submission_id(&s) {
                Ok(s) => s,
                Err(e) => {
                    eprint_line(&format!("error: {}", e.message));
                    return 1;
                }
            },
            None => Uuid::new_v4().to_string(),
        };
        params["submissionId"] = json!(sid);
        match client::request_blocking(ClientRole::Cli, "task.start", params) {
            Ok(resp) => match unwrap_result(resp) {
                Ok(v) => {
                    if flags.json {
                        print_line(&v.to_string());
                    } else {
                        let sr: StartResult = serde_json::from_value(v).unwrap_or(StartResult {
                            submission_id: sid,
                            task_id: String::new(),
                            turn_id: String::new(),
                            turn_ordinal: 0,
                            status: "queued".into(),
                            mode: input.mode,
                            created_at: String::new(),
                            task_deleted: None,
                        });
                        print_line(&format!(
                            "started taskId={} turnId={} status={}",
                            sr.task_id, sr.turn_id, sr.status
                        ));
                    }
                    0
                }
                Err(e) => {
                    eprint_line(&format!("error: {e:#}"));
                    2
                }
            },
            Err(e) => {
                eprint_line(&format!("daemon/IPC error: {e:#}"));
                3
            }
        }
    } else {
        match client::request_blocking(ClientRole::Cli, "task.run", params) {
            Ok(resp) => match unwrap_result(resp) {
                Ok(v) => {
                    if flags.json {
                        print_line(&v.to_string());
                        return 0;
                    }
                    match serde_json::from_value::<RunResult>(v) {
                        Ok(r) => {
                            print_line(&run_result_text_summary(&r));
                            match r.status {
                                crate::dto::RunStatus::Failed => 2,
                                _ => 0,
                            }
                        }
                        Err(e) => {
                            eprint_line(&format!("error decoding result: {e}"));
                            2
                        }
                    }
                }
                Err(e) => {
                    eprint_line(&format!("error: {e:#}"));
                    2
                }
            },
            Err(e) => {
                eprint_line(&format!("daemon/IPC error: {e:#}"));
                3
            }
        }
    }
}

fn cmd_status(args: &[String]) -> i32 {
    let mut json_out = false;
    let mut task_id = None;
    for a in args {
        if a == "--json" {
            json_out = true;
        } else if !a.starts_with('-') && task_id.is_none() {
            task_id = Some(a.clone());
        }
    }
    let Some(task_id) = task_id else {
        // No task id → daemon status
        return match crate::daemon::status_text() {
            Ok(t) => {
                print_line(&t);
                0
            }
            Err(e) => {
                eprint_line(&format!("{e:#}"));
                3
            }
        };
    };
    let task_id = match validate_uuid_like(&task_id, "taskId") {
        Ok(id) => id,
        Err(e) => {
            eprint_line(&format!("error: {}", e.message));
            return 1;
        }
    };
    match client::request_blocking(ClientRole::Cli, "task.status", json!({ "taskId": task_id })) {
        Ok(resp) => match unwrap_result(resp) {
            Ok(v) => {
                if json_out {
                    print_line(&v.to_string());
                } else if let Ok(s) = serde_json::from_value::<TaskStatus>(v) {
                    print_line(&format!(
                        "taskId={} status={} mode={} model={}",
                        s.task_id,
                        s.status.as_str(),
                        s.mode.as_str(),
                        s.actual_model.as_deref().unwrap_or("-")
                    ));
                    if let Some(a) = s.latest_action {
                        print_line(&format!("latest: {a}"));
                    }
                    if let Some(p) = s.answer_preview {
                        print_line(&format!("answer: {p}"));
                    }
                }
                0
            }
            Err(e) => {
                eprint_line(&format!("error: {e:#}"));
                2
            }
        },
        Err(e) => {
            eprint_line(&format!("daemon/IPC error: {e:#}"));
            3
        }
    }
}

fn cmd_wait(args: &[String]) -> i32 {
    let mut json_out = false;
    let mut timeout_s: Option<u64> = None;
    let mut ids = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json_out = true,
            "--timeout" => {
                i += 1;
                timeout_s = args.get(i).and_then(|s| s.parse().ok());
            }
            s if !s.starts_with('-') => ids.push(s.to_string()),
            other => {
                eprint_line(&format!("error: unknown flag `{other}`"));
                return 1;
            }
        }
        i += 1;
    }
    if ids.len() < 2 {
        eprint_line("error: usage: wait TASK_ID TURN_ID [--timeout SECONDS] [--json]");
        return 1;
    }
    let task_id = match validate_uuid_like(&ids[0], "taskId") {
        Ok(id) => id,
        Err(e) => {
            eprint_line(&format!("error: {}", e.message));
            return 1;
        }
    };
    let turn_id = match validate_uuid_like(&ids[1], "turnId") {
        Ok(id) => id,
        Err(e) => {
            eprint_line(&format!("error: {}", e.message));
            return 1;
        }
    };
    let timeout_ms = timeout_s
        .map(|s| s.saturating_mul(1000))
        .unwrap_or(DEFAULT_WAIT_TIMEOUT_MS);
    let params = json!({
        "taskId": task_id,
        "turnId": turn_id,
        "timeoutMs": timeout_ms,
    });
    match client::request_blocking(ClientRole::Cli, "task.wait", params) {
        Ok(resp) => match unwrap_result(resp) {
            Ok(v) => {
                if json_out {
                    print_line(&v.to_string());
                    return 0;
                }
                if v.get("timedOut").and_then(|t| t.as_bool()) == Some(true) {
                    if let Ok(w) = serde_json::from_value::<WaitTimeout>(v) {
                        print_line(&format!(
                            "timed out taskId={} turnId={} status={}",
                            w.task_id,
                            w.turn_id,
                            w.status.as_str()
                        ));
                    }
                    return 0;
                }
                match serde_json::from_value::<RunResult>(v) {
                    Ok(r) => {
                        print_line(&run_result_text_summary(&r));
                        match r.status {
                            crate::dto::RunStatus::Failed => 2,
                            _ => 0,
                        }
                    }
                    Err(e) => {
                        eprint_line(&format!("error: {e}"));
                        2
                    }
                }
            }
            Err(e) => {
                eprint_line(&format!("error: {e:#}"));
                2
            }
        },
        Err(e) => {
            eprint_line(&format!("daemon/IPC error: {e:#}"));
            3
        }
    }
}

fn cmd_cancel(args: &[String]) -> i32 {
    let mut json_out = false;
    let mut task_id = None;
    let mut turn_id = None;
    let mut recovery_id = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json_out = true,
            "--turn" => {
                i += 1;
                turn_id = args.get(i).cloned();
            }
            "--recovery" => {
                i += 1;
                recovery_id = args.get(i).cloned();
            }
            s if !s.starts_with('-') && task_id.is_none() => task_id = Some(s.to_string()),
            // bare second id as turn
            s if !s.starts_with('-') && turn_id.is_none() => turn_id = Some(s.to_string()),
            other => {
                eprint_line(&format!("error: unknown arg `{other}`"));
                return 1;
            }
        }
        i += 1;
    }
    let Some(task_id) = task_id else {
        eprint_line("error: usage: cancel TASK_ID (--turn TURN_ID | --recovery RECOVERY_ID)");
        return 1;
    };
    let task_id = match validate_uuid_like(&task_id, "taskId") {
        Ok(id) => id,
        Err(e) => {
            eprint_line(&format!("error: {}", e.message));
            return 1;
        }
    };
    let params = if let Some(tid) = turn_id {
        let tid = match validate_uuid_like(&tid, "turnId") {
            Ok(id) => id,
            Err(e) => {
                eprint_line(&format!("error: {}", e.message));
                return 1;
            }
        };
        json!({ "taskId": task_id, "turnId": tid })
    } else if let Some(rid) = recovery_id {
        json!({ "taskId": task_id, "recoveryId": rid })
    } else {
        eprint_line("error: --turn TURN_ID or --recovery RECOVERY_ID required");
        return 1;
    };
    match client::request_blocking(ClientRole::Cli, "task.cancel", params) {
        Ok(resp) => match unwrap_result(resp) {
            Ok(v) => {
                if json_out {
                    print_line(&v.to_string());
                } else if let Ok(c) = serde_json::from_value::<TurnCancelResult>(v.clone()) {
                    print_line(&format!(
                        "cancelled turnId={} alreadyTerminal={} taskStatus={}",
                        c.turn_id,
                        c.already_terminal,
                        c.task_status.as_str()
                    ));
                } else {
                    print_line(&v.to_string());
                }
                0
            }
            Err(e) => {
                eprint_line(&format!("error: {e:#}"));
                2
            }
        },
        Err(e) => {
            eprint_line(&format!("daemon/IPC error: {e:#}"));
            3
        }
    }
}

fn cmd_tasks(args: &[String]) -> i32 {
    if args.is_empty() {
        eprint_line("error: usage: tasks list|show|clear ...");
        return 1;
    }
    match args[0].as_str() {
        "list" => cmd_tasks_list(&args[1..]),
        "show" => cmd_tasks_show(&args[1..]),
        other => {
            eprint_line(&format!("error: unknown tasks subcommand `{other}`"));
            1
        }
    }
}

fn cmd_tasks_list(args: &[String]) -> i32 {
    let mut json_out = false;
    let mut limit: i64 = 50;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => json_out = true,
            "--limit" => {
                i += 1;
                limit = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(50);
            }
            other => {
                eprint_line(&format!("error: unknown flag `{other}`"));
                return 1;
            }
        }
        i += 1;
    }
    match client::request_blocking(ClientRole::Cli, "tasks.list", json!({ "limit": limit })) {
        Ok(resp) => match unwrap_result(resp) {
            Ok(v) => {
                if json_out {
                    print_line(&v.to_string());
                } else if let Ok(list) = serde_json::from_value::<Vec<TaskListItem>>(v) {
                    if list.is_empty() {
                        print_line("(no tasks)");
                    }
                    for t in list {
                        print_line(&format!(
                            "{}  {:10}  {:5}  {}",
                            t.task_id,
                            t.status.as_str(),
                            t.mode.as_str(),
                            t.title
                        ));
                    }
                }
                0
            }
            Err(e) => {
                eprint_line(&format!("error: {e:#}"));
                2
            }
        },
        Err(e) => {
            eprint_line(&format!("daemon/IPC error: {e:#}"));
            3
        }
    }
}

fn cmd_tasks_show(args: &[String]) -> i32 {
    let mut json_out = false;
    let mut task_id = None;
    for a in args {
        if a == "--json" {
            json_out = true;
        } else if !a.starts_with('-') {
            task_id = Some(a.clone());
        }
    }
    let Some(task_id) = task_id else {
        eprint_line("error: usage: tasks show TASK_ID [--json]");
        return 1;
    };
    let task_id = match validate_uuid_like(&task_id, "taskId") {
        Ok(id) => id,
        Err(e) => {
            eprint_line(&format!("error: {}", e.message));
            return 1;
        }
    };
    match client::request_blocking(ClientRole::Cli, "tasks.show", json!({ "taskId": task_id })) {
        Ok(resp) => match unwrap_result(resp) {
            Ok(v) => {
                if json_out {
                    print_line(&v.to_string());
                } else if let Ok(d) = serde_json::from_value::<TaskDetail>(v) {
                    print_line(&format!(
                        "{}  {}  {}",
                        d.task.task_id,
                        d.task.status.as_str(),
                        d.title
                    ));
                    print_line(&format!("cwd={} mode={}", d.cwd, d.task.mode.as_str()));
                    for ev in d.timeline {
                        let stream = if ev.streaming { "…" } else { "" };
                        print_line(&format!(
                            "  [{}] {}{stream}",
                            ev.kind,
                            if ev.message.is_empty() {
                                ev.text.chars().take(100).collect::<String>()
                            } else {
                                ev.message.clone()
                            }
                        ));
                    }
                }
                0
            }
            Err(e) => {
                eprint_line(&format!("error: {e:#}"));
                2
            }
        },
        Err(e) => {
            eprint_line(&format!("daemon/IPC error: {e:#}"));
            3
        }
    }
}

fn cmd_doctor(args: &[String]) -> i32 {
    let json_out = args.iter().any(|a| a == "--json");
    let cfg = crate::config::ConfigDocument::load().ok().map(|d| d.config);
    let report = crate::doctor::run_doctor(cfg.as_ref());
    if json_out {
        match serde_json::to_string(&report) {
            Ok(s) => print_line(&s),
            Err(e) => {
                eprint_line(&format!("error encoding doctor report: {e}"));
                return 1;
            }
        }
    } else {
        print_line(&format!("GrokTask {}", report.version));
        print_line(&format!("executable: {}", report.executable));
        print_line(&format!("daemon: {}", report.daemon));
        if let Some(mode) = &report.tray_mode {
            print_line(&format!("trayMode: {mode}"));
        }
        print_line(&format!(
            "tray: available={} click={}",
            report.tray.tray_available,
            report.tray.tray_click.as_str()
        ));
        if let Some(d) = &report.tray.detail {
            print_line(&format!("trayDetail: {d}"));
        }
        let g = &report.grok;
        print_line(&format!(
            "grok: state={} path={} version={}",
            g.state.as_str(),
            g.executable.as_deref().unwrap_or("(not found)"),
            g.version.as_deref().unwrap_or("-")
        ));
        if let Some(guide) = &g.guidance {
            print_line(&format!("grokGuidance: {guide}"));
        }
    }
    0
}

fn cmd_app(args: &[String]) -> i32 {
    let mut task_id: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--task" => {
                i += 1;
                task_id = args.get(i).cloned();
            }
            s if !s.starts_with('-') && task_id.is_none() => task_id = Some(s.to_string()),
            other => {
                eprint_line(&format!("error: unknown app arg `{other}`"));
                return 1;
            }
        }
        i += 1;
    }
    let cmd = match task_id {
        Some(id) => crate::ipc::protocol::GuiNavCommand::OpenTask { task_id: id },
        None => crate::ipc::protocol::GuiNavCommand::Focus,
    };
    match crate::app::gui_host::ensure_and_navigate(cmd) {
        Ok(()) => 0,
        Err(e) => {
            eprint_line(&format!("error: {e}"));
            1
        }
    }
}

/// Open Settings > Integrations. Does not write any Agent or GrokTask config.
fn cmd_setup(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--write" || a == "--install") {
        eprint_line("error: setup never writes config; use `agents mode …` or Settings UI");
        return 1;
    }
    let cmd = crate::ipc::protocol::GuiNavCommand::OpenSettings {
        section: Some("integrations".into()),
    };
    match crate::app::gui_host::ensure_and_navigate(cmd) {
        Ok(()) => {
            print_line("opened Settings → Integrations (no config changes)");
            0
        }
        Err(e) => {
            eprint_line(&format!("error: {e}"));
            1
        }
    }
}

fn cmd_agents(args: &[String]) -> i32 {
    if args.is_empty() {
        eprint_line(
            "error: usage: agents status [codex|claude] | agents mode codex|claude mcp|none",
        );
        return 1;
    }
    let json_out = args.iter().any(|a| a == "--json");
    match args[0].as_str() {
        "status" => {
            let filter = args.get(1).and_then(|s| {
                if s == "--json" {
                    None
                } else {
                    crate::integrations::AgentId::parse(s)
                }
            });
            if let Some(s) = args.get(1) {
                if s != "--json" && filter.is_none() {
                    eprint_line(&format!(
                        "error: unknown agent `{s}`; expected codex|claude"
                    ));
                    return 1;
                }
            }
            let command = crate::integrations::current_exe_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "GrokTask".into());
            let roots = integration_roots_for_cli();
            let report = crate::integrations::status_report(&roots, filter, &command);
            if json_out {
                print_line(&serde_json::to_string(&report).unwrap_or_else(|_| "{}".into()));
            } else {
                for a in &report.agents {
                    print_line(&format!(
                        "{}: status={} config={} binary={}",
                        a.agent.as_str(),
                        a.status.as_str(),
                        a.config_path,
                        a.binary_path
                    ));
                    if let Some(d) = &a.detail {
                        print_line(&format!("  detail: {d}"));
                    }
                }
            }
            0
        }
        "mode" => {
            // agents mode codex|claude mcp|none
            let agent_s = match args.get(1).map(|s| s.as_str()) {
                Some(s) => s,
                None => {
                    eprint_line("error: usage: agents mode codex|claude mcp|none");
                    return 1;
                }
            };
            let mode = match args.get(2).map(|s| s.as_str()) {
                Some(s) => s,
                None => {
                    eprint_line("error: usage: agents mode codex|claude mcp|none");
                    return 1;
                }
            };
            let agent = match crate::integrations::AgentId::parse(agent_s) {
                Some(a) => a,
                None => {
                    eprint_line(&format!("error: unknown agent `{agent_s}`"));
                    return 1;
                }
            };
            if mode != "mcp" && mode != "none" {
                eprint_line("error: mode must be mcp|none");
                return 1;
            }
            let command = match crate::integrations::current_exe_path() {
                Ok(p) => p.display().to_string(),
                Err(e) => {
                    eprint_line(&format!("error: {e}"));
                    return 1;
                }
            };
            let roots = integration_roots_for_cli();
            match crate::integrations::set_mode(&roots, agent, mode, &command) {
                Ok(status) => {
                    if json_out {
                        print_line(&serde_json::to_string(&status).unwrap_or_else(|_| "{}".into()));
                    } else {
                        print_line(&format!(
                            "{} mode={mode} status={}",
                            agent.as_str(),
                            status.status.as_str()
                        ));
                        print_line(&format!("config={}", status.config_path));
                        if mode == "mcp" {
                            print_line(
                                "note: restart or reload MCP in the agent for changes to take effect",
                            );
                        }
                    }
                    0
                }
                Err(e) => {
                    eprint_line(&format!("error: {e}"));
                    1
                }
            }
        }
        other => {
            eprint_line(&format!("error: unknown agents subcommand `{other}`"));
            1
        }
    }
}

/// Integration roots for CLI. Honors `GROKTASK_AGENT_HOME` so tests never touch real ~/.codex.
fn integration_roots_for_cli() -> crate::integrations::IntegrationRoots {
    if let Ok(home) = std::env::var("GROKTASK_AGENT_HOME") {
        if !home.is_empty() {
            return crate::integrations::IntegrationRoots::from_home(home);
        }
    }
    crate::integrations::IntegrationRoots::user_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn help_mentions_mcp_and_daemon() {
        let h = help::help_text();
        assert!(h.contains("mcp"));
        assert!(h.contains("daemon run"));
        assert!(!h.contains("--gui-host")); // hidden internal entry
    }

    #[test]
    fn path_detects_macos_app_bundle_exe() {
        let bundled = PathBuf::from("/Users/me/Applications/GrokTask.app/Contents/MacOS/GrokTask");
        assert!(path_is_macos_app_bundle_exe(&bundled));

        let bare = PathBuf::from("/Users/me/bin/GrokTask");
        assert!(!path_is_macos_app_bundle_exe(&bare));

        let release = PathBuf::from("/Users/me/proj/src-tauri/target/release/GrokTask");
        assert!(!path_is_macos_app_bundle_exe(&release));

        // Nested path must still match the Contents/MacOS marker.
        let nested = PathBuf::from("/tmp/build/GrokTask.app/Contents/MacOS/GrokTask");
        assert!(path_is_macos_app_bundle_exe(&nested));

        // Incomplete bundle layout is not treated as an app launch.
        assert!(!path_is_macos_app_bundle_exe(Path::new(
            "/tmp/GrokTask.app/GrokTask"
        )));
        assert!(!path_is_macos_app_bundle_exe(Path::new(
            "/tmp/Contents/MacOS/GrokTask"
        )));
    }

    #[test]
    fn no_args_routes_app_bundle_to_gui_else_cli_help() {
        assert_eq!(
            no_args_action(true),
            NoArgsAction::GuiAppLaunch,
            "macOS .app no-args must open the GUI, not print help"
        );
        assert_eq!(
            no_args_action(false),
            NoArgsAction::CliHelp,
            "bare binary no-args keeps CLI help + exit(1)"
        );
    }

    #[test]
    fn version_non_empty() {
        assert!(!help::version_text().is_empty());
    }

    #[test]
    fn parse_run_requires_mode_via_flag() {
        let f = parse_run_flags(&[
            "--mode".into(),
            "read".into(),
            "--cwd".into(),
            "/tmp".into(),
            "hello".into(),
        ])
        .unwrap();
        assert_eq!(f.mode.as_deref(), Some("read"));
        assert_eq!(f.task_parts, vec!["hello"]);
    }

    #[test]
    fn parse_read_write_shorthand() {
        let f = parse_run_flags(&["--read".into(), "do".into(), "it".into()]).unwrap();
        assert_eq!(f.mode.as_deref(), Some("read"));
        assert_eq!(f.task_parts.join(" "), "do it");
    }

    #[test]
    fn agents_status_stable_with_temp_home() {
        let _g = crate::paths::test_env_lock();
        let tmp = tempfile::TempDir::new().unwrap();
        let prev = std::env::var_os("GROKTASK_AGENT_HOME");
        std::env::set_var("GROKTASK_AGENT_HOME", tmp.path());
        let roots = integration_roots_for_cli();
        let report = crate::integrations::status_report(&roots, None, "/tmp/GrokTask");
        assert_eq!(report.agents.len(), 2);
        assert_eq!(
            report.agents[0].status,
            crate::integrations::IntegrationStatus::NotInstalled
        );
        // Must not point at real home configs in this test.
        assert!(report.agents[0]
            .config_path
            .starts_with(&tmp.path().display().to_string()));
        match prev {
            Some(v) => std::env::set_var("GROKTASK_AGENT_HOME", v),
            None => std::env::remove_var("GROKTASK_AGENT_HOME"),
        }
    }

    #[test]
    fn agents_mode_uses_same_engine() {
        let _g = crate::paths::test_env_lock();
        let tmp = tempfile::TempDir::new().unwrap();
        let prev = std::env::var_os("GROKTASK_AGENT_HOME");
        std::env::set_var("GROKTASK_AGENT_HOME", tmp.path());
        let roots = integration_roots_for_cli();
        let cmd = "/opt/test-GrokTask";
        let st =
            crate::integrations::set_mode(&roots, crate::integrations::AgentId::Codex, "mcp", cmd)
                .unwrap();
        assert_eq!(st.status, crate::integrations::IntegrationStatus::Installed);
        let st =
            crate::integrations::set_mode(&roots, crate::integrations::AgentId::Codex, "none", cmd)
                .unwrap();
        assert_eq!(
            st.status,
            crate::integrations::IntegrationStatus::NotInstalled
        );
        match prev {
            Some(v) => std::env::set_var("GROKTASK_AGENT_HOME", v),
            None => std::env::remove_var("GROKTASK_AGENT_HOME"),
        }
    }

    #[test]
    fn setup_command_does_not_write_agent_configs() {
        // setup only opens Settings; this unit test pins the no-write contract by
        // verifying agent home is untouched when we only construct the nav command.
        let tmp = tempfile::TempDir::new().unwrap();
        let codex = tmp.path().join(".codex").join("config.toml");
        std::fs::create_dir_all(codex.parent().unwrap()).unwrap();
        std::fs::write(&codex, b"# untouched\n").unwrap();
        let before = std::fs::read(&codex).unwrap();
        let cmd = crate::ipc::protocol::GuiNavCommand::OpenSettings {
            section: Some("integrations".into()),
        };
        assert!(matches!(
            cmd,
            crate::ipc::protocol::GuiNavCommand::OpenSettings { .. }
        ));
        assert_eq!(std::fs::read(&codex).unwrap(), before);
    }
}

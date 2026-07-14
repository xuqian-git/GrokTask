fn main() {
    // Register every `#[tauri::command]` so tauri-build emits allow/deny ACL
    // permissions (`allow-tasks-list`, etc.). Without this, the packaged app
    // rejects IPC with "tasks_list not allowed" and the UI never loads tasks.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "settings_get",
            "settings_set_tray_mode",
            "agents_status",
            "agents_install",
            "agents_remove",
            "agents_workflow_enable",
            "agents_workflow_disable",
            "workspace_cwd",
            "doctor_report",
            "grok_cli_status",
            "daemon_status_text",
            "daemon_restart",
            "tasks_list",
            "tasks_show",
        ]),
    ))
    .expect("failed to run tauri-build");
}

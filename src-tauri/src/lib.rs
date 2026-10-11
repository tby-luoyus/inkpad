//! 便签桌面应用 —— 库入口
//!
//! 业务模块都在这里导出，`main.rs` 只负责调用 `run()`。
//! 这样拆分的好处：便于写单元测试，也便于未来把核心逻辑复用到 CLI 或移动端。

pub mod api;
pub mod commands;
pub mod hub;
pub mod models;
pub mod service;
pub mod store;
pub mod vault;
pub mod window;

pub use hub::{AppEvent, EventBus, Hub, SharedHub};
pub use models::{Doc, Note, NotePatch, NoteQuery, Settings};
pub use service::AppService;

use std::path::PathBuf;
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, RunEvent, WindowEvent,
};

/// 数据目录：%APPDATA%/com.stickynotes.desktop（各平台自动适配）
pub fn resolve_data_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("STICKY_NOTES_HOME") {
        if !custom.trim().is_empty() {
            return PathBuf::from(custom);
        }
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.stickynotes.desktop")
}

/// 装配并运行应用
pub fn run() {
    let data_dir = resolve_data_dir();

    // release 下没有控制台，日志只写 stderr 等于丢失。
    // 同时写一份到 <数据目录>/app.log，方便排查启动与运行时问题。
    let log_path = data_dir.join("app.log");
    std::fs::create_dir_all(&data_dir).ok();
    // 追加模式：多次启动 / 多实例不互相覆盖日志
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok();

    let builder = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sticky_notes_lib=debug".into()),
        )
        .with_target(false);

    match log_file {
        Some(f) => builder.with_writer(std::sync::Mutex::new(f)).init(),
        None => builder.init(),
    }

    tracing::info!("数据目录: {}", data_dir.display());

    let svc = match AppService::new(data_dir) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            tracing::error!("初始化数据层失败: {e:#}");
            std::process::exit(1);
        }
    };
    if let Err(e) = svc.scaffold_demo_assets() {
        tracing::warn!("初始化示例资源失败: {e:#}");
    }

    let hub: SharedHub = Arc::new(Hub::new());
    if let Err(e) = svc.load_plugins(&hub, None) {
        tracing::warn!("加载插件失败: {e:#}");
    }

    let svc_setup = svc.clone();
    let hub_setup = hub.clone();

    tauri::Builder::default()
        // 单实例保护（官方要求最先注册）：重复双击 exe 时聚焦已有主窗口，
        // 而不是再起一个进程。两个实例会抢同一个 SQLite、同一批端口、
        // 同一份日志，行为不可预测。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(svc)
        .manage(hub)
        .invoke_handler(tauri::generate_handler![
            commands::notes_list,
            commands::notes_get,
            commands::notes_count,
            commands::notes_create,
            commands::notes_update,
            commands::notes_append,
            commands::notes_trash,
            commands::notes_restore,
            commands::notes_purge,
            commands::notes_purge_trash,
            commands::notes_duplicate,
            commands::notes_set_pinned,
            commands::notes_set_color,
            commands::notes_set_hidden,
            commands::notes_bring_to_front,
            commands::window_open_note,
            commands::window_close_note,
            commands::window_show_main,
            commands::window_sync_geometry,
            commands::window_note_closing,
            commands::settings_get,
            commands::settings_save,
            commands::backup_export,
            commands::backup_import,
            commands::backup_export_path,
            commands::backup_import_path,
            commands::data_export_json,
            commands::ext_commands,
            commands::ext_themes,
            commands::ext_reload_themes,
            commands::ext_plugins,
            commands::ext_reload_plugins,
            commands::ext_set_plugin_enabled,
            commands::ext_plugin_start,
            commands::ext_plugin_stop,
            commands::ext_plugin_ping,
            commands::ext_plugin_log,
            commands::ext_events_recent,
            commands::ext_webhooks,
            commands::ext_webhook_upsert,
            commands::ext_webhook_remove,
            commands::ext_emit_event,
            commands::ext_execute,
            commands::ext_api_status,
            commands::ext_api_set_enabled,
            commands::ext_paths,
            commands::ext_open_dir,
            commands::docs_list,
            commands::docs_tree,
            commands::docs_info,
            commands::docs_tags,
            commands::docs_get,
            commands::docs_search,
            commands::docs_create,
            commands::docs_save,
            commands::docs_rename,
            commands::docs_delete,
            commands::docs_backlinks,
            commands::docs_outlinks,
            commands::docs_reindex,
            commands::docs_from_notes,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            // 注入 AppHandle：事件需要经 Tauri 原生通道转发给前端 webview
            hub_setup.attach_app(handle.clone());
            if let Err(e) = svc_setup.load_plugins(&hub_setup, Some(&handle)) {
                tracing::warn!("重新加载插件失败: {e:#}");
            }
            setup_tray(&handle)?;
            restore_open_notes(&handle, &svc_setup);
            purge_expired_trash(&svc_setup);
            maybe_start_http(&handle, &svc_setup, &hub_setup);
            hub_setup.broadcast("app.ready", serde_json::json!({}), "system");
            Ok(())
        })
        .on_window_event(|w, event| {
            let app = w.app_handle().clone();
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    let label = w.label().to_string();
                    let svc = app.state::<Arc<AppService>>().inner().clone();

                    if label == "main" {
                        let settings = svc.settings().unwrap_or_default();
                        if settings.minimize_to_tray {
                            api.prevent_close();
                            let _ = w.hide();
                        } else {
                            window::close_all_note_windows(&app);
                        }
                    } else if let Some(id) = window::note_id_from_label(&label) {
                        window::sync_geometry_now(&app, &svc, &id);
                        svc.set_open(&id, false).ok();
                        let hub = app.state::<SharedHub>().inner().clone();
                        hub.broadcast("note.closed", serde_json::json!({ "id": id }), "ui");
                    }
                }
                WindowEvent::Focused(true) => {
                    if let Some(id) = window::note_id_from_label(w.label()) {
                        let svc = app.state::<Arc<AppService>>().inner().clone();
                        let hub = app.state::<SharedHub>().inner().clone();
                        if let Ok(Some(note)) = svc.bring_to_front_and_get(&id) {
                            hub.broadcast(
                                "note.focused",
                                serde_json::json!({ "id": note.id, "zIndex": note.z_index }),
                                "ui",
                            );
                        }
                    }
                }
                WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
                    if let Some(id) = window::note_id_from_label(w.label()) {
                        let svc = app.state::<Arc<AppService>>().inner().clone();
                        window::sync_geometry_now(&app, &svc, &id);
                    }
                }
                _ => {}
            }
        })
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(|app, event| {
            match event {
                RunEvent::ExitRequested { api, .. } => {
                    let svc = app.state::<Arc<AppService>>().inner().clone();
                    let settings = svc.settings().unwrap_or_default();
                    if settings.minimize_to_tray && app.get_webview_window("main").is_some() {
                        api.prevent_exit();
                    }
                }
                RunEvent::Exit => {
                    // 退出时收掉所有插件子进程，避免留下孤儿进程
                    let svc = app.state::<Arc<AppService>>().inner().clone();
                    let hub = app.state::<SharedHub>().inner().clone();
                    svc.stop_all_plugin_processes(&hub);
                }
                _ => {}
            }
        });
}

/// 系统托盘：快速新建便签 / 显示主窗口 / 隐藏全部 / 退出
fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let new_note = MenuItem::with_id(app, "new_note", "新建便签", true, Some("Ctrl+N"))?;
    let show = MenuItem::with_id(app, "show_main", "显示便签面板", true, None::<&str>)?;
    let hide_all = MenuItem::with_id(app, "hide_all", "隐藏全部便签", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&new_note, &show, &hide_all, &sep, &quit])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or(tauri::Error::AssetNotFound("icon".into()))?;

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("便签")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id().as_ref() {
            "new_note" => {
                let handle = app.clone();
                let svc = handle.state::<Arc<AppService>>().inner().clone();
                let hub = handle.state::<SharedHub>().inner().clone();
                let _ = service::execute_command(
                    &handle, &svc, &hub, "notes.create",
                    serde_json::json!({ "open": true }), "system",
                );
            }
            "show_main" => window::show_main(app),
            "hide_all" => {
                for (label, w) in app.webview_windows() {
                    if label.starts_with("note-") {
                        let _ = w.hide();
                    }
                }
            }
            "quit" => {
                window::close_all_note_windows(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// 启动时恢复上次打开的便签窗口
fn restore_open_notes(app: &AppHandle, svc: &Arc<AppService>) {
    let settings = svc.settings().unwrap_or_default();
    if !settings.restore_on_start {
        return;
    }
    match svc.list_notes(&NoteQuery::default()) {
        Ok(notes) => {
            for n in notes.iter().filter(|n| n.is_open) {
                if let Err(e) = window::open_note_window(app, n) {
                    tracing::warn!("恢复便签窗口 {} 失败: {e:#}", n.id);
                }
            }
        }
        Err(e) => tracing::warn!("恢复便签失败: {e:#}"),
    }
}

fn purge_expired_trash(svc: &Arc<AppService>) {
    let days = svc.settings().map(|s| s.trash_retention_days).unwrap_or(30);
    if let Ok(n) = svc.store.purge_expired_trash(days) {
        if n > 0 {
            tracing::info!("已清理 {n} 条过期回收站便签");
        }
    }
}

fn maybe_start_http(app: &AppHandle, svc: &Arc<AppService>, hub: &SharedHub) {
    let settings = svc.settings().unwrap_or_default();
    if settings.http_api_enabled {
        if let Err(e) = api::set_enabled(app, svc, hub, true, Some(settings.http_api_port)) {
            tracing::warn!("启动本地 API 失败: {e:#}");
        }
    }
}

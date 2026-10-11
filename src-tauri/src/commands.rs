//! Tauri 命令层
//!
//! 所有命令都遵循同一模式：调用 `AppService` 的业务逻辑 → 广播事件 → 返回结果。
//! 这样无论调用来自 UI、HTTP API 还是插件，行为完全一致。

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

use crate::hub::{AppEvent, PluginRuntime, SharedHub, ThemePack};
use crate::models::{
    Backlink, BackupFile, Doc, DocNode, DocPatch, Note, NotePatch, NoteQuery, OpResult, Settings,
    VaultInfo,
};
use crate::service::AppService;
use crate::window;

// ---------------------------------------------------------------- 便签查询

#[tauri::command]
pub fn notes_list(svc: State<'_, std::sync::Arc<AppService>>, query: Option<NoteQuery>) -> Result<Vec<Note>, String> {
    svc.list_notes(&query.unwrap_or_default()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn notes_get(svc: State<'_, std::sync::Arc<AppService>>, id: String) -> Result<Option<Note>, String> {
    svc.get_note(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn notes_count(svc: State<'_, std::sync::Arc<AppService>>) -> Result<(usize, usize, usize), String> {
    svc.counts().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- 便签写入

#[tauri::command]
pub fn notes_create(
    app: AppHandle,
    svc: State<'_, std::sync::Arc<AppService>>,
    hub: State<'_, SharedHub>,
    patch: Option<NotePatch>,
    open: Option<bool>,
) -> Result<Note, String> {
    let mut p = patch.unwrap_or_default();
    // 未指定位置时做阶梯式偏移，避免新便签完全重叠
    if p.x.is_none() || p.y.is_none() {
        let (cx, cy) = svc.suggest_position().map_err(|e| e.to_string())?;
        p.x.get_or_insert(cx);
        p.y.get_or_insert(cy);
    }
    let should_open = open.unwrap_or(true);
    let note = svc.create_note(p, should_open).map_err(|e| e.to_string())?;
    if should_open {
        // 只投递、不等待：窗口任务必须在事件循环里执行，
        // 就地同步建窗会死锁（详见 window.rs 顶部说明）。
        window::open_note_window(&app, &note).map_err(|e| e.to_string())?;
    }
    hub.broadcast_note("note.created", &note, "ui");
    Ok(note)
}

#[tauri::command]
pub fn notes_update(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
    patch: NotePatch,
) -> Result<Option<Note>, String> {
    let updated = svc.update_note(&id, &patch).map_err(|e| e.to_string())?;
    if let Some(n) = &updated {
        hub.broadcast_note("note.updated", n, "ui");
    }
    Ok(updated)
}

#[tauri::command]
pub fn notes_append(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
    text: String,
) -> Result<Option<Note>, String> {
    let n = svc.append_to_note(&id, &text).map_err(|e| e.to_string())?;
    if let Some(note) = &n {
        hub.broadcast_note("note.updated", note, "ui");
    }
    Ok(n)
}

#[tauri::command]
pub fn notes_trash(
    app: AppHandle,
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
) -> Result<bool, String> {
    let n = svc.get_note(&id).map_err(|e| e.to_string())?;
    let ok = svc.trash_note(&id).map_err(|e| e.to_string())?;
    window::close_note_window(&app, &id);
    if let Some(note) = n {
        hub.broadcast_note("note.trashed", &note, "ui");
    }
    Ok(ok)
}

#[tauri::command]
pub fn notes_restore(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
) -> Result<bool, String> {
    let ok = svc.restore_note(&id).map_err(|e| e.to_string())?;
    hub.emit(AppEvent::new("note.restored", json!({ "id": id }), "ui"));
    Ok(ok)
}

#[tauri::command]
pub fn notes_purge(
    app: AppHandle,
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
) -> Result<bool, String> {
    let ok = svc.purge_note(&id).map_err(|e| e.to_string())?;
    window::close_note_window(&app, &id);
    hub.emit(AppEvent::new("note.purged", json!({ "id": id }), "ui"));
    Ok(ok)
}

#[tauri::command]
pub fn notes_purge_trash(hub: State<'_, SharedHub>, svc: State<'_, std::sync::Arc<AppService>>) -> Result<usize, String> {
    let n = svc.purge_trash().map_err(|e| e.to_string())?;
    hub.emit(AppEvent::new("trash.emptied", json!({ "count": n }), "ui"));
    Ok(n)
}

#[tauri::command]
pub fn notes_duplicate(
    app: AppHandle,
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
) -> Result<Option<Note>, String> {
    let n = svc.duplicate_note(&id).map_err(|e| e.to_string())?;
    if let Some(note) = &n {
        // 副本默认打开，方便用户立刻看到
        let _ = window::open_note_window(&app, note);
        hub.broadcast_note("note.created", note, "ui");
    }
    Ok(n)
}

#[tauri::command]
pub fn notes_set_pinned(
    app: AppHandle,
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
    pinned: bool,
) -> Result<bool, String> {
    let ok = svc.set_pinned(&id, pinned).map_err(|e| e.to_string())?;
    if let Some(w) = app.get_webview_window(&window::note_label(&id)) {
        let _ = w.set_always_on_top(pinned);
    }
    hub.emit(
        AppEvent::new("note.pinnedChanged", json!({ "id": id, "pinned": pinned }), "ui"),
    );
    Ok(ok)
}

#[tauri::command]
pub fn notes_set_color(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
    color: String,
) -> Result<bool, String> {
    let ok = svc.set_color(&id, &color).map_err(|e| e.to_string())?;
    hub.emit(AppEvent::new("note.colorChanged", json!({ "id": id, "color": color }), "ui"));
    Ok(ok)
}

#[tauri::command]
pub fn notes_set_hidden(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
    hidden: bool,
) -> Result<bool, String> {
    let ok = svc.set_hidden(&id, hidden).map_err(|e| e.to_string())?;
    hub.emit(AppEvent::new("note.hiddenChanged", json!({ "id": id, "hidden": hidden }), "ui"));
    Ok(ok)
}

#[tauri::command]
pub fn notes_bring_to_front(svc: State<'_, std::sync::Arc<AppService>>, id: String) -> Result<i64, String> {
    svc.bring_to_front(&id).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- 窗口控制

#[tauri::command]
pub fn window_open_note(app: AppHandle, svc: State<'_, std::sync::Arc<AppService>>, id: String) -> Result<(), String> {
    let note = svc
        .get_note(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "便签不存在".to_string())?;
    svc.set_open(&id, true).ok();
    window::open_note_window(&app, &note).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn window_close_note(app: AppHandle, svc: State<'_, std::sync::Arc<AppService>>, id: String) -> Result<(), String> {
    svc.set_open(&id, false).ok();
    window::close_note_window(&app, &id);
    Ok(())
}

#[tauri::command]
pub fn window_show_main(app: AppHandle) -> Result<(), String> {
    window::show_main(&app);
    Ok(())
}

/// 便签窗口在前端完成渲染后调用，用于同步几何尺寸
#[tauri::command]
pub fn window_sync_geometry(app: AppHandle, svc: State<'_, std::sync::Arc<AppService>>, id: String) -> Result<(), String> {
    // 读窗口几何是一次阻塞式的跨线程往返，必须放到主线程上做，
    // 否则会占住 Tauri 命令线程池（表现为所有 invoke 无响应）。
    window::sync_geometry_with(&app, svc.inner(), &id, NotePatch::default());
    Ok(())
}

/// 便签窗口即将关闭：把窗口位置写回数据库
#[tauri::command]
pub fn window_note_closing(app: AppHandle, svc: State<'_, std::sync::Arc<AppService>>, id: String) -> Result<(), String> {
    window::sync_geometry_with(
        &app,
        svc.inner(),
        &id,
        NotePatch { is_open: Some(false), ..Default::default() },
    );
    svc.set_open(&id, false).ok();
    Ok(())
}

// ------------------------------------------------------------------ 设置

#[tauri::command]
pub fn settings_get(svc: State<'_, std::sync::Arc<AppService>>) -> Result<Settings, String> {
    svc.settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn settings_save(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    settings: Settings,
) -> Result<Settings, String> {
    let s = svc.save_settings(&settings).map_err(|e| e.to_string())?;
    hub.emit(AppEvent::new("settings.changed", serde_json::to_value(&s).unwrap_or(json!({})), "ui"));
    Ok(s)
}

// ------------------------------------------------------- 备份 / 导入导出

#[tauri::command]
pub fn backup_export(app: AppHandle, svc: State<'_, std::sync::Arc<AppService>>) -> Result<String, String> {
    svc.export_to_dialog(&app).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn backup_import(
    app: AppHandle,
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
) -> Result<OpResult, String> {
    let r = svc.import_from_dialog(&app).map_err(|e| e.to_string())?;
    hub.emit(AppEvent::new("data.imported", json!({ "affected": r.affected }), "ui"));
    Ok(r)
}

#[tauri::command]
pub fn backup_export_path(svc: State<'_, std::sync::Arc<AppService>>, path: String) -> Result<OpResult, String> {
    let n = svc.export_to_path(&path).map_err(|e| e.to_string())?;
    Ok(OpResult { ok: true, affected: n, message: format!("已导出 {n} 条便签") })
}

#[tauri::command]
pub fn backup_import_path(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    path: String,
    overwrite: Option<bool>,
) -> Result<OpResult, String> {
    let n = svc
        .import_from_path(&path, overwrite.unwrap_or(false))
        .map_err(|e| e.to_string())?;
    hub.emit(AppEvent::new("data.imported", json!({ "affected": n }), "http"));
    Ok(OpResult { ok: true, affected: n, message: format!("已导入 {n} 条便签") })
}

#[tauri::command]
pub fn data_export_json(svc: State<'_, std::sync::Arc<AppService>>) -> Result<BackupFile, String> {
    svc.export_backup().map_err(|e| e.to_string())
}

// ------------------------------------------------------------ 拓展接口

#[tauri::command]
pub async fn ext_commands(hub: State<'_, SharedHub>) -> Result<Value, String> {
    Ok(serde_json::to_value(hub.commands.all()).unwrap_or(json!([])))
}

#[tauri::command]
pub async fn ext_themes(svc: State<'_, std::sync::Arc<AppService>>) -> Result<Vec<ThemePack>, String> {
    Ok(svc.themes())
}

#[tauri::command]
pub async fn ext_reload_themes(svc: State<'_, std::sync::Arc<AppService>>) -> Result<Vec<ThemePack>, String> {
    svc.reload_themes();
    Ok(svc.themes())
}

#[tauri::command]
pub async fn ext_plugins(hub: State<'_, SharedHub>, svc: State<'_, std::sync::Arc<AppService>>) -> Result<Vec<PluginRuntime>, String> {
    Ok(svc.plugin_runtimes(hub.inner()))
}

#[tauri::command]
pub async fn ext_reload_plugins(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
) -> Result<Vec<PluginRuntime>, String> {
    svc.reload_plugins(hub.inner()).map_err(|e| e.to_string())?;
    Ok(svc.plugin_runtimes(hub.inner()))
}

#[tauri::command]
pub async fn ext_set_plugin_enabled(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
    enabled: bool,
) -> Result<bool, String> {
    let ok = svc.set_plugin_enabled(hub.inner(), &id, enabled).map_err(|e| e.to_string())?;
    hub.emit(AppEvent::new("plugin.toggled", json!({ "id": id, "enabled": enabled }), "ui"));
    Ok(ok)
}

/// 手动拉起插件的自启动进程
#[tauri::command]
pub async fn ext_plugin_start(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
) -> Result<bool, String> {
    svc.start_plugin_process(hub.inner(), &id).map_err(|e| e.to_string())
}

/// 停止插件的自启动进程
#[tauri::command]
pub async fn ext_plugin_stop(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
) -> Result<bool, String> {
    Ok(svc.stop_plugin_process(hub.inner(), &id))
}

/// 探测插件后端是否可达（返回响应内容或 null）
#[tauri::command]
pub async fn ext_plugin_ping(
    hub: State<'_, SharedHub>,
    id: String,
) -> Result<Value, String> {
    let endpoint = {
        let map = hub.plugins.read();
        match map.get(&id).and_then(|p| p.endpoint.clone()) {
            Some(e) => e,
            None => return Ok(json!({ "reachable": false, "reason": "未配置 endpoint" })),
        }
    };
    let client = crate::hub::shared_http_client();
    let url = endpoint.trim_end_matches("/sticky").to_string();
    match client
        .get(format!("{url}/health"))
        .timeout(std::time::Duration::from_millis(1500))
        .send()
        .await
    {
        Ok(r) => {
            let status = r.status().as_u16();
            let text = r.text().await.unwrap_or_default();
            Ok(json!({ "reachable": true, "status": status, "body": text }))
        }
        Err(e) => Ok(json!({ "reachable": false, "reason": e.to_string() })),
    }
}

#[tauri::command]
pub async fn ext_events_recent(hub: State<'_, SharedHub>, limit: Option<usize>) -> Result<Vec<AppEvent>, String> {
    Ok(hub.events.recent(limit.unwrap_or(50)))
}

/// 读取插件后端进程的日志（末尾 `limit` 行），用于在界面上排查插件启动失败。
#[tauri::command]
pub async fn ext_plugin_log(
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
    limit: Option<usize>,
) -> Result<Value, String> {
    let path = svc.logs_dir().join(format!("{id}.log"));
    if !path.exists() {
        return Ok(json!({
            "path": path.to_string_lossy(),
            "exists": false,
            "text": "",
        }));
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("读取插件日志失败: {e}"))?;
    let n = limit.unwrap_or(200);
    let lines: Vec<&str> = raw.lines().collect();
    let text = if lines.len() > n {
        lines[lines.len() - n..].join("\n")
    } else {
        raw
    };
    Ok(json!({
        "path": path.to_string_lossy(),
        "exists": true,
        "text": text,
    }))
}

#[tauri::command]
pub async fn ext_webhooks(hub: State<'_, SharedHub>) -> Result<Value, String> {
    Ok(serde_json::to_value(hub.events.webhooks()).unwrap_or(json!([])))
}

#[tauri::command]
pub async fn ext_webhook_upsert(hub: State<'_, SharedHub>, config: crate::hub::WebhookConfig) -> Result<bool, String> {
    hub.events.upsert_webhook(config);
    Ok(true)
}

#[tauri::command]
pub async fn ext_webhook_remove(hub: State<'_, SharedHub>, id: String) -> Result<bool, String> {
    Ok(hub.events.remove_webhook(&id))
}

/// 广播一个自定义事件（供 UI 中的扩展面板 / 自动化使用）
#[tauri::command]
pub async fn ext_emit_event(hub: State<'_, SharedHub>, name: String, payload: Option<Value>) -> Result<(), String> {
    hub.emit(AppEvent::new(&name, payload.unwrap_or(json!({})), "ui"));
    Ok(())
}

/// 统一命令执行入口 —— Agent / 插件 / 自动化都走这里
#[tauri::command]
pub async fn ext_execute(
    app: AppHandle,
    svc: State<'_, std::sync::Arc<AppService>>,
    hub: State<'_, SharedHub>,
    command: String,
    args: Option<Value>,
) -> Result<Value, String> {
    crate::service::execute_command(
        &app,
        svc.inner(),
        hub.inner(),
        &command,
        args.unwrap_or(json!({})),
        "ui",
    )
    .map_err(|e| e.to_string())
}

/// 本地 HTTP 服务状态
#[tauri::command]
pub async fn ext_api_status(svc: State<'_, std::sync::Arc<AppService>>) -> Result<Value, String> {
    Ok(svc.api_status())
}

#[tauri::command]
pub async fn ext_api_set_enabled(
    app: AppHandle,
    svc: State<'_, std::sync::Arc<AppService>>,
    enabled: bool,
    port: Option<u16>,
) -> Result<Value, String> {
    let svc_arc = app.state::<std::sync::Arc<AppService>>().inner().clone();
    let hub_arc = app.state::<SharedHub>().inner().clone();
    crate::api::set_enabled(&app, &svc_arc, &hub_arc, enabled, port).map_err(|e| e.to_string())?;
    Ok(svc.api_status())
}

/// 插件目录（便于用户放置自己的插件）
#[tauri::command]
pub async fn ext_paths(svc: State<'_, std::sync::Arc<AppService>>) -> Result<Value, String> {
    Ok(svc.paths_info())
}

/// 打开数据/插件/主题目录
#[tauri::command]
pub fn ext_open_dir(app: AppHandle, kind: String, svc: State<'_, std::sync::Arc<AppService>>) -> Result<(), String> {
    let p = svc.dir_for(&kind).ok_or_else(|| "未知目录类型".to_string())?;
    tauri_plugin_opener::open_path(p.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
    .map(|_| ())
    .or_else(|_: String| {
        let _ = &app;
        Ok(())
    })
}

// ============================================================== 笔记（Vault）

#[tauri::command]
pub async fn docs_list(
    svc: State<'_, std::sync::Arc<AppService>>,
    folder: Option<String>,
    tag: Option<String>,
) -> Result<Vec<Doc>, String> {
    svc.vault_list(folder, tag).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn docs_tree(svc: State<'_, std::sync::Arc<AppService>>) -> Result<Vec<DocNode>, String> {
    svc.vault_tree().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn docs_info(svc: State<'_, std::sync::Arc<AppService>>) -> Result<VaultInfo, String> {
    svc.vault_info().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn docs_tags(svc: State<'_, std::sync::Arc<AppService>>) -> Result<Vec<(String, usize)>, String> {
    svc.vault_tags().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn docs_get(
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
) -> Result<Option<Doc>, String> {
    svc.vault_read(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn docs_search(
    svc: State<'_, std::sync::Arc<AppService>>,
    keyword: String,
) -> Result<Vec<Doc>, String> {
    svc.vault_search(&keyword).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn docs_create(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    patch: DocPatch,
) -> Result<Doc, String> {
    let d = svc.vault_create(patch).map_err(|e| e.to_string())?;
    hub.emit(AppEvent::new(
        "doc.created",
        json!({ "id": d.id, "path": d.path, "title": d.title }),
        "ui",
    ));
    Ok(d)
}

#[tauri::command]
pub async fn docs_save(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
    patch: DocPatch,
) -> Result<Option<Doc>, String> {
    let d = svc.vault_save(&id, patch).map_err(|e| e.to_string())?;
    if let Some(doc) = &d {
        hub.emit(AppEvent::new(
            "doc.saved",
            json!({ "id": doc.id, "path": doc.path, "title": doc.title, "wordCount": doc.word_count }),
            "ui",
        ));
    }
    Ok(d)
}

#[tauri::command]
pub async fn docs_rename(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
    path: String,
) -> Result<Option<Doc>, String> {
    let d = svc.vault_rename(&id, &path).map_err(|e| e.to_string())?;
    hub.emit(AppEvent::new("doc.renamed", json!({ "id": id, "path": path }), "ui"));
    Ok(d)
}

#[tauri::command]
pub async fn docs_delete(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
) -> Result<bool, String> {
    let ok = svc.vault_delete(&id).map_err(|e| e.to_string())?;
    hub.emit(AppEvent::new("doc.deleted", json!({ "id": id }), "ui"));
    Ok(ok)
}

#[tauri::command]
pub async fn docs_backlinks(
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
) -> Result<Vec<Backlink>, String> {
    svc.vault_backlinks(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn docs_outlinks(
    svc: State<'_, std::sync::Arc<AppService>>,
    id: String,
) -> Result<Vec<Doc>, String> {
    svc.vault_outlinks(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn docs_reindex(
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
) -> Result<usize, String> {
    let n = svc.vault_reindex().map_err(|e| e.to_string())?;
    hub.emit(AppEvent::new("doc.reindexed", json!({ "count": n }), "ui"));
    Ok(n)
}

/// 便签汇总生成笔记（内置实现，插件可覆盖）
#[tauri::command]
pub async fn docs_from_notes(
    app: AppHandle,
    hub: State<'_, SharedHub>,
    svc: State<'_, std::sync::Arc<AppService>>,
    ids: Option<Vec<String>>,
    title: Option<String>,
    folder: Option<String>,
) -> Result<Value, String> {
    let mut args = json!({ "ids": ids.unwrap_or_default() });
    if let Some(t) = title {
        args["title"] = json!(t);
    }
    if let Some(f) = folder {
        args["folder"] = json!(f);
    }
    crate::service::execute_command(&app, svc.inner(), hub.inner(), "doc.fromNotes", args, "ui")
        .map_err(|e| e.to_string())
}

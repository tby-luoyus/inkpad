//! 本地 HTTP API —— 多 App 联动的主要通道
//!
//! 任何外部程序（Python 脚本、浏览器扩展、其他桌面应用、Agent 运行时）
//! 都可以通过 `http://127.0.0.1:<port>/api/v1/...` 操作便签。
//!
//! 鉴权：请求头 `X-Api-Token: <token>`，token 保存在数据目录的 `api.token`。
//! 因为只监听 127.0.0.1，默认不强制校验；如果提供了 token 则必须匹配。

use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{sse::{Event, KeepAlive, Sse}, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tauri::Manager;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::hub::{Hub, SharedHub};
use crate::models::{NotePatch, NoteQuery};
use crate::service::{execute_command, AppService};

#[derive(Clone)]
pub struct ApiState {
    pub app: tauri::AppHandle,
    pub svc: Arc<AppService>,
    pub hub: SharedHub,
}

/// 启动服务；如果已在运行则先停掉旧的
pub fn set_enabled(
    app: &tauri::AppHandle,
    svc: &Arc<AppService>,
    hub: &Arc<Hub>,
    enabled: bool,
    port: Option<u16>,
) -> Result<()> {
    if !enabled {
        *svc.http_running.write() = None;
        tracing::info!("本地 HTTP API 已关闭");
        return Ok(());
    }
    let p = port.unwrap_or_else(|| svc.settings().map(|s| s.http_api_port).unwrap_or(43919));
    *svc.http_running.write() = Some(p);

    let state = ApiState {
        app: app.clone(),
        svc: svc.clone(),
        hub: hub.clone(),
    };
    spawn_server(state, p);
    Ok(())
}

/// 供内部命令总线调用的版本：只要引用即可，内部自行完成共享化
pub fn apply_enabled(
    app: &tauri::AppHandle,
    svc: &Arc<AppService>,
    hub: &Arc<Hub>,
    enabled: bool,
    port: Option<u16>,
) -> Result<()> {
    if !enabled {
        *svc.http_running.write() = None;
        tracing::info!("本地 HTTP API 已关闭");
        return Ok(());
    }
    let p = port.unwrap_or_else(|| svc.settings().map(|s| s.http_api_port).unwrap_or(43919));
    *svc.http_running.write() = Some(p);
    let state = ApiState {
        app: app.clone(),
        svc: svc.clone(),
        hub: hub.clone(),
    };
    spawn_server(state, p);
    Ok(())
}

/// 无法直接拿到 Arc 时的封装：内部持有的是同一份共享状态，
/// 这里用一个只用于监听的轻量句柄即可（AxumState 需要 Arc 以满足 Send + 'static）。
pub fn spawn_server_shared(
    app: &tauri::AppHandle,
    svc: &AppService,
    hub: &Hub,
    port: u16,
) {
    let _ = (app, svc, hub, port);
    tracing::warn!("命令总线启动 HTTP 服务需通过 UI 设置面板操作（缺少共享句柄）");
}

/// 真正拉起 axum 服务：独立线程 + 多线程运行时，随应用生命周期常驻
///
/// 必须是多线程：单线程运行时里任何一个 handler 的同步阻塞（哪怕是几十毫秒的
/// 锁等待）都会把 accept 循环一起卡住，表现为整个 HTTP 接口无响应 ——
/// 连不依赖任何状态的 `/health` 都超时。
fn spawn_server(state: ApiState, p: u16) {
    let token = state.svc.api_token();
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("sticky-http")
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("创建 HTTP 运行时失败: {e}");
                return;
            }
        };
        rt.block_on(async move {
            let router = build_router(state.clone());
            let addr = SocketAddr::from(([127, 0, 0, 1], p));
            match tokio::net::TcpListener::bind(addr).await {
                Ok(listener) => {
                    tracing::info!("本地 HTTP API 已启动: http://127.0.0.1:{p}/api/v1  token={token}");
                    state.hub.broadcast(
                        "api.started",
                        json!({ "port": p, "baseUrl": format!("http://127.0.0.1:{p}/api/v1") }),
                        "system",
                    );
                    if let Err(e) = axum::serve(listener, router).await {
                        tracing::error!("HTTP 服务异常退出: {e}");
                    }
                }
                Err(e) => {
                    tracing::error!("HTTP API 监听 {addr} 失败: {e}");
                    *state.svc.http_running.write() = None;
                    state.hub.broadcast("api.error", json!({ "error": e.to_string() }), "system");
                }
            }
        });
    });
}

fn build_router(state: ApiState) -> Router {
    Router::new()
        // 元信息
        .route("/api/v1/health", get(health))
        .route("/api/v1/commands", get(list_commands))
        .route("/api/v1/events", get(recent_events))
        .route("/api/v1/events/subscribe", get(subscribe_sse))
        .route("/api/v1/capabilities", get(capabilities))
        // 便签
        .route("/api/v1/notes", get(list_notes).post(create_note))
        .route(
            "/api/v1/notes/:id",
            get(get_note).patch(patch_note).delete(trash_note),
        )
        .route("/api/v1/notes/:id/content", post(set_content))
        .route("/api/v1/notes/:id/append", post(append_content))
        .route("/api/v1/notes/:id/pin", post(set_pin))
        .route("/api/v1/notes/:id/color", post(set_color))
        .route("/api/v1/notes/:id/restore", post(restore_note))
        .route("/api/v1/notes/:id/purge", post(purge_note))
        .route("/api/v1/notes/:id/open", post(open_note))
        .route("/api/v1/notes/:id/close", post(close_note))
        // 批量
        .route("/api/v1/search", get(search))
        .route("/api/v1/trash", get(list_trash).delete(empty_trash))
        // 通用命令入口（Agent 首选）
        .route("/api/v1/execute", post(execute))
        // 备份
        .route("/api/v1/backup/export", post(backup_export))
        .route("/api/v1/backup/import", post(backup_import))
        // 设置 / 主题
        .route("/api/v1/settings", get(get_settings).patch(patch_settings))
        .route("/api/v1/themes", get(list_themes))
        // 笔记（Vault）—— 对标 Obsidian
        .route("/api/v1/docs", get(list_docs).post(create_doc))
        .route("/api/v1/docs/tree", get(docs_tree))
        .route("/api/v1/docs/info", get(docs_info))
        .route("/api/v1/docs/tags", get(docs_tags))
        .route("/api/v1/docs/search", get(docs_search_http))
        .route("/api/v1/docs/reindex", post(docs_reindex_http))
        .route("/api/v1/docs/from-notes", post(docs_from_notes_http))
        .route(
            "/api/v1/docs/:id",
            get(get_doc).patch(save_doc).delete(delete_doc),
        )
        .route("/api/v1/docs/:id/backlinks", get(doc_backlinks_http))
        .route("/api/v1/docs/:id/outlinks", get(doc_outlinks_http))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state)
}

type ApiResult = Result<Json<Value>, ApiError>;

/// 统一的 API 错误类型，把 anyhow / serde 错误都收敛成 400 JSON 响应
pub struct ApiError(StatusCode, Json<Value>);

impl ApiError {
    fn new(code: StatusCode, msg: impl std::fmt::Display) -> Self {
        Self(code, Json(json!({ "ok": false, "error": msg.to_string() })))
    }
    fn bad(msg: impl std::fmt::Display) -> Self {
        Self::new(StatusCode::BAD_REQUEST, msg)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::bad(format!("{e:#}"))
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError::bad(format!("序列化失败: {e}"))
    }
}

impl From<tauri::Error> for ApiError {
    fn from(e: tauri::Error) -> Self {
        ApiError::bad(format!("窗口操作失败: {e}"))
    }
}

fn ok(v: Value) -> Json<Value> {
    Json(json!({ "ok": true, "data": v }))
}

/// 鉴权失败时直接返回错误响应
fn require_auth(st: &ApiState, headers: &HeaderMap) -> Result<(), ApiError> {
    let expected = st.svc.api_token();
    match headers.get("x-api-token").and_then(|v| v.to_str().ok()) {
        // 带了 token 但不匹配 -> 拒绝
        Some(t) if t != expected => Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "token 不匹配",
        )),
        // 未带 token：本机回环允许，便于快速调试
        _ => Ok(()),
    }
}

// --------------------------------------------------------------- 处理器

async fn health() -> Json<Value> {
    Json(json!({
        "ok": true,
        "app": "sticky-notes",
        "version": env!("CARGO_PKG_VERSION"),
        "time": chrono::Utc::now().to_rfc3339(),
    }))
}

async fn capabilities(State(st): State<ApiState>) -> ApiResult {
    Ok(ok(json!({
        "commands": st.hub.commands.all(),
        "themes": st.svc.themes().iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
        "plugins": st.hub.plugins.read().values().map(|p| json!({
            "id": p.id, "name": p.name, "enabled": p.enabled
        })).collect::<Vec<_>>(),
        "endpoints": [
            "GET  /api/v1/notes",
            "POST /api/v1/notes",
            "GET  /api/v1/notes/:id",
            "PATCH /api/v1/notes/:id",
            "DELETE /api/v1/notes/:id",
            "POST /api/v1/notes/:id/append",
            "POST /api/v1/notes/:id/pin",
            "POST /api/v1/notes/:id/color",
            "POST /api/v1/notes/:id/open",
            "GET  /api/v1/search?q=",
            "GET  /api/v1/trash",
            "DELETE /api/v1/trash",
            "POST /api/v1/execute",
            "POST /api/v1/backup/export",
            "POST /api/v1/backup/import",
            "GET  /api/v1/events",
            "GET  /api/v1/events/subscribe",
            "GET  /api/v1/commands",
            "GET  /api/v1/settings",
            "PATCH /api/v1/settings",
            "GET  /api/v1/themes",
        ],
    })))
}

async fn list_commands(State(st): State<ApiState>) -> ApiResult {
    Ok(ok(serde_json::to_value(st.hub.commands.all()).unwrap_or(json!([]))))
}

#[derive(Deserialize)]
struct EventsQuery {
    limit: Option<usize>,
}

async fn recent_events(State(st): State<ApiState>, Query(q): Query<EventsQuery>) -> ApiResult {
    Ok(ok(serde_json::to_value(st.hub.events.recent(q.limit.unwrap_or(50)))?))
}

/// SSE 实时事件流 —— 外部 App 订阅便签变更的最简单方式
async fn subscribe_sse(State(st): State<ApiState>) -> Response {
    let rx = st.hub.events.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|item| match item {
        Ok(ev) => {
            let data = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into());
            Some(Ok::<Event, Infallible>(
                Event::default().event(ev.name.clone()).data(data),
            ))
        }
        // 落后的订阅者会被跳过，不中断连接
        Err(_) => None,
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

async fn list_notes(State(st): State<ApiState>, Query(q): Query<HashMap<String, String>>) -> ApiResult {
    let query = NoteQuery {
        keyword: q.get("q").cloned(),
        include_deleted: q.get("includeDeleted").map(|v| v == "true"),
        only_deleted: q.get("onlyDeleted").map(|v| v == "true"),
        sort: q.get("sort").cloned(),
        colors: q
            .get("colors")
            .map(|c| c.split(',').map(|s| s.to_string()).collect::<Vec<String>>()),
    };
    Ok(ok(serde_json::to_value(st.svc.list_notes(&query)?)?))
}

async fn search(State(st): State<ApiState>, Query(q): Query<HashMap<String, String>>) -> ApiResult {
    let query = NoteQuery {
        keyword: q.get("q").cloned(),
        ..Default::default()
    };
    Ok(ok(serde_json::to_value(st.svc.list_notes(&query)?)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateBody {
    content: Option<String>,
    color: Option<String>,
    #[serde(default)]
    open: Option<bool>,
    meta: Option<Value>,
}

async fn create_note(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Json(body): Json<CreateBody>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let v = execute_command(
        &st.app,
        &st.svc,
        &st.hub,
        "notes.create",
        json!({
            "content": body.content.unwrap_or_default(),
            "color": body.color,
            "meta": body.meta,
            "open": body.open.unwrap_or(true),
        }),
        "http",
    )?;
    Ok(ok(v))
}

async fn get_note(State(st): State<ApiState>, Path(id): Path<String>) -> ApiResult {
    let note = st.svc.get_note(&id)?;
    match note {
        Some(n) => Ok(ok(serde_json::to_value(n)?)),
        None => Ok(Json(json!({ "ok": false, "error": "便签不存在" }))),
    }
}

async fn patch_note(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(patch): Json<NotePatch>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let n = st.svc.update_note(&id, &patch)?;
    if let Some(note) = &n {
        st.hub.broadcast_note("note.updated", note, "http");
    }
    Ok(ok(serde_json::to_value(n)?))
}

async fn trash_note(State(st): State<ApiState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult {
    require_auth(&st, &headers)?;
    let note = st.svc.get_note(&id)?;
    let ok_ = st.svc.trash_note(&id)?;
    crate::window::close_note_window(&st.app, &id);
    if let Some(n) = note {
        st.hub.broadcast_note("note.trashed", &n, "http");
    }
    Ok(ok(json!({ "ok": ok_ })))
}

#[derive(Deserialize)]
struct ContentBody {
    content: Option<String>,
    text: Option<String>,
}

async fn set_content(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<ContentBody>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let content = b.content.or(b.text).unwrap_or_default();
    let n = st.svc.update_note(&id, &NotePatch { content: Some(content), ..Default::default() })?;
    if let Some(note) = &n {
        st.hub.broadcast_note("note.updated", note, "http");
    }
    Ok(ok(serde_json::to_value(n)?))
}

async fn append_content(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<ContentBody>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let text = b.text.or(b.content).unwrap_or_default();
    let n = st.svc.append_to_note(&id, &text)?;
    if let Some(note) = &n {
        st.hub.broadcast_note("note.updated", note, "http");
    }
    Ok(ok(serde_json::to_value(n)?))
}

#[derive(Deserialize)]
struct PinBody {
    pinned: Option<bool>,
}

async fn set_pin(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<PinBody>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let pinned = b.pinned.unwrap_or(true);
    let ok_ = st.svc.set_pinned(&id, pinned)?;
    if let Some(w) = st.app.get_webview_window(&crate::window::note_label(&id)) {
        let _ = w.set_always_on_top(pinned);
    }
    st.hub.broadcast("note.pinnedChanged", json!({ "id": id, "pinned": pinned }), "http");
    Ok(ok(json!({ "ok": ok_ })))
}

#[derive(Deserialize)]
struct ColorBody {
    color: String,
}

async fn set_color(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<ColorBody>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let ok_ = st.svc.set_color(&id, &b.color)?;
    st.hub.broadcast("note.colorChanged", json!({ "id": id, "color": b.color }), "http");
    Ok(ok(json!({ "ok": ok_ })))
}

async fn restore_note(State(st): State<ApiState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult {
    require_auth(&st, &headers)?;
    let ok_ = st.svc.restore_note(&id)?;
    st.hub.broadcast("note.restored", json!({ "id": id }), "http");
    Ok(ok(json!({ "ok": ok_ })))
}

async fn purge_note(State(st): State<ApiState>, headers: HeaderMap, Path(id): Path<String>) -> ApiResult {
    require_auth(&st, &headers)?;
    let ok_ = st.svc.purge_note(&id)?;
    crate::window::close_note_window(&st.app, &id);
    st.hub.broadcast("note.purged", json!({ "id": id }), "http");
    Ok(ok(json!({ "ok": ok_ })))
}

async fn open_note(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let v = execute_command(&st.app, &st.svc, &st.hub, "window.openNote", json!({ "id": id }), "http")?;
    Ok(ok(v))
}

async fn close_note(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let v = execute_command(&st.app, &st.svc, &st.hub, "window.closeNote", json!({ "id": id }), "http")?;
    Ok(ok(v))
}

async fn list_trash(State(st): State<ApiState>) -> ApiResult {
    let q = NoteQuery {
        only_deleted: Some(true),
        sort: Some("updated".into()),
        ..Default::default()
    };
    Ok(ok(serde_json::to_value(st.svc.list_notes(&q)?)?))
}

async fn empty_trash(State(st): State<ApiState>, headers: HeaderMap) -> ApiResult {
    require_auth(&st, &headers)?;
    let n = st.svc.purge_trash()?;
    st.hub.broadcast("trash.emptied", json!({ "count": n }), "http");
    Ok(ok(json!({ "affected": n })))
}

#[derive(Deserialize)]
struct ExecuteBody {
    command: String,
    #[serde(default)]
    args: Value,
}

/// 通用命令入口 —— Agent / 自动化脚本首选
async fn execute(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Json(b): Json<ExecuteBody>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let v = execute_command(&st.app, &st.svc, &st.hub, &b.command, b.args, "http")?;
    Ok(ok(v))
}

#[derive(Deserialize)]
struct ExportBody {
    path: Option<String>,
    /// 是否连设置一起导出（仅在直接返回备份体时生效）
    #[serde(default)]
    #[allow(dead_code)]
    with_settings: bool,
}

async fn backup_export(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Json(b): Json<ExportBody>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    match b.path {
        Some(p) => {
            let n = st.svc.export_to_path(&p)?;
            Ok(ok(json!({ "affected": n, "path": p })))
        }
        None => {
            let backup = st.svc.export_backup()?;
            Ok(ok(json!({ "affected": backup.notes.len(), "backup": backup })))
        }
    }
}

#[derive(Deserialize)]
struct ImportBody {
    path: String,
    #[serde(default)]
    overwrite: bool,
}

async fn backup_import(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Json(b): Json<ImportBody>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let n = st.svc.import_from_path(&b.path, b.overwrite)?;
    st.hub.broadcast("data.imported", json!({ "affected": n }), "http");
    Ok(ok(json!({ "affected": n })))
}

async fn get_settings(State(st): State<ApiState>) -> ApiResult {
    Ok(ok(serde_json::to_value(st.svc.settings()?)?))
}

async fn patch_settings(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Json(patch): Json<Value>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let cur = serde_json::to_value(st.svc.settings()?)?;
    let merged = crate::service::merge_json(cur, patch);
    let s: crate::models::Settings = serde_json::from_value(merged)?;
    let saved = st.svc.save_settings(&s)?;
    st.hub.broadcast("settings.changed", serde_json::to_value(&saved)?, "http");
    Ok(ok(serde_json::to_value(saved)?))
}

async fn list_themes(State(st): State<ApiState>) -> ApiResult {
    Ok(ok(serde_json::to_value(st.svc.themes())?))
}

// ================================================================ 笔记（Vault）

async fn list_docs(State(st): State<ApiState>, Query(q): Query<HashMap<String, String>>) -> ApiResult {
    let folder = q.get("folder").cloned();
    let tag = q.get("tag").cloned();
    Ok(ok(serde_json::to_value(st.svc.vault_list(folder, tag)?)?))
}

async fn docs_tree(State(st): State<ApiState>) -> ApiResult {
    Ok(ok(serde_json::to_value(st.svc.vault_tree()?)?))
}

async fn docs_info(State(st): State<ApiState>) -> ApiResult {
    Ok(ok(serde_json::to_value(st.svc.vault_info()?)?))
}

async fn docs_tags(State(st): State<ApiState>) -> ApiResult {
    Ok(ok(serde_json::to_value(st.svc.vault_tags()?)?))
}

async fn docs_search_http(
    State(st): State<ApiState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult {
    let kw = q.get("q").cloned().unwrap_or_default();
    Ok(ok(serde_json::to_value(st.svc.vault_search(&kw)?)?))
}

async fn get_doc(State(st): State<ApiState>, Path(id): Path<String>) -> ApiResult {
    match st.svc.vault_read(&id)? {
        Some(d) => Ok(ok(serde_json::to_value(d)?)),
        None => Ok(Json(json!({ "ok": false, "error": "笔记不存在" }))),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateDocBody {
    path: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    frontmatter: Option<Value>,
}

async fn create_doc(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Json(b): Json<CreateDocBody>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let v = execute_command(
        &st.app,
        &st.svc,
        &st.hub,
        "doc.create",
        json!({ "path": b.path, "content": b.content, "frontmatter": b.frontmatter }),
        "http",
    )?;
    Ok(ok(v))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveDocBody {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    frontmatter: Option<Value>,
    #[serde(default)]
    path: Option<String>,
}

async fn save_doc(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<SaveDocBody>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    // 带 path 视为重命名/移动
    if let Some(p) = b.path.filter(|p| !p.is_empty()) {
        let v = execute_command(
            &st.app, &st.svc, &st.hub, "doc.rename",
            json!({ "id": id, "path": p }), "http",
        )?;
        return Ok(ok(v));
    }
    let v = execute_command(
        &st.app,
        &st.svc,
        &st.hub,
        "doc.save",
        json!({ "id": id, "content": b.content, "frontmatter": b.frontmatter }),
        "http",
    )?;
    Ok(ok(v))
}

async fn delete_doc(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let v = execute_command(
        &st.app, &st.svc, &st.hub, "doc.delete", json!({ "id": id }), "http",
    )?;
    Ok(ok(v))
}

async fn doc_backlinks_http(State(st): State<ApiState>, Path(id): Path<String>) -> ApiResult {
    Ok(ok(serde_json::to_value(st.svc.vault_backlinks(&id)?)?))
}

async fn doc_outlinks_http(State(st): State<ApiState>, Path(id): Path<String>) -> ApiResult {
    Ok(ok(serde_json::to_value(st.svc.vault_outlinks(&id)?)?))
}

async fn docs_reindex_http(State(st): State<ApiState>, headers: HeaderMap) -> ApiResult {
    require_auth(&st, &headers)?;
    let v = execute_command(
        &st.app, &st.svc, &st.hub, "doc.reindex", json!({}), "http",
    )?;
    Ok(ok(v))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FromNotesBody {
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    folder: Option<String>,
}

/// 便签汇总生成笔记 —— 外部 App / Agent 也能直接调
async fn docs_from_notes_http(
    State(st): State<ApiState>,
    headers: HeaderMap,
    Json(b): Json<FromNotesBody>,
) -> ApiResult {
    require_auth(&st, &headers)?;
    let v = execute_command(
        &st.app,
        &st.svc,
        &st.hub,
        "doc.fromNotes",
        json!({ "ids": b.ids, "title": b.title, "folder": b.folder }),
        "http",
    )?;
    Ok(ok(v))
}

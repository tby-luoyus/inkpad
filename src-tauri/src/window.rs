//! 窗口管理：主窗口 + 每张便签一个独立无边框窗口
//!
//! 便签窗口的创建/关闭/位置同步都在这里。窗口 label 约定为 `note-<id>`。
//!
//! ## 为什么窗口任务必须「投递」而不是「执行」
//!
//! Tauri 的窗口 API（`WebviewWindowBuilder::build`、`outer_position`、`inner_size`、
//! `show`、`destroy`……）底层都是：
//!
//! ```text
//! send_user_message(...)   // 把请求塞进事件循环
//! rx.recv()                // 同步阻塞，等主线程处理完
//! ```
//!
//! `send_user_message` 只在**调用方正好是主线程**时就地同步执行，否则走
//! `proxy.send_event` 交给事件循环。于是有两个陷阱：
//!
//! 1. **在别的线程上调用** → 调用方阻塞。主线程事件循环一旦没能及时消费
//!    （批量建窗、WebView2 忙……），调用线程就永久卡住：Tauri 命令线程池
//!    和 HTTP 线程被耗尽，表现为「点击新建便签没反应」。
//! 2. **在主线程上调用** → 看起来很安全，其实更危险：`run_on_main_thread`
//!    会就地执行，于是建窗发生在**事件循环回调内部**。实测（见 app.log 的
//!    `[diag]`）`notes_create` 命令本身就跑在 `main` 线程上，
//!    `WebviewWindowBuilder::build()` 一旦在 IPC 回调里同步初始化 WebView2，
//!    WebView2 又反过来等消息循环泵消息 —— **直接死锁，整个应用冻住**。
//!
//! 结论：**窗口任务永远不能在调用线程上就地执行，必须投递到事件循环。**
//! 这里用一个常驻投递线程承接任务，再从该线程调 `run_on_main_thread`，
//! 由于该线程不是主线程，`send_user_message` 必然走 `proxy.send_event`
//! 分支 —— 既保证任务在主线程执行，又保证不在当前调用栈上执行。

use anyhow::Result;
use std::sync::{mpsc::Sender, Arc, OnceLock};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

use crate::models::Note;

pub fn note_label(id: &str) -> String {
    format!("note-{id}")
}

/// 一个待在主线程上执行的窗口任务
type WindowJob = Box<dyn FnOnce(&AppHandle) + Send + 'static>;

/// 窗口任务队列 + 常驻投递线程（单例）
static DISPATCH_TX: OnceLock<Sender<(AppHandle, WindowJob)>> = OnceLock::new();

fn dispatcher() -> &'static Sender<(AppHandle, WindowJob)> {
    DISPATCH_TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<(AppHandle, WindowJob)>();
        std::thread::Builder::new()
            .name("sticky-window-dispatch".into())
            .spawn(move || {
                // 这个线程不是主线程 → run_on_main_thread 一定走事件循环投递，
                // 绝不会就地执行，从而避免上面的死锁 #2。
                while let Ok((app, job)) = rx.recv() {
                    let owned = app.clone();
                    if let Err(e) = app.run_on_main_thread(move || job(&owned)) {
                        tracing::warn!("派发主线程任务失败: {e:#}");
                    }
                }
            })
            .expect("启动窗口任务投递线程失败");
        tx
    })
}

/// 把一个窗口任务投递到主线程执行，**不等待结果**。
///
/// 这是本模块唯一的跨线程入口：调用方（主线程 / 命令线程 / HTTP 线程）
/// 永远不会因为窗口操作而阻塞或死锁。
fn dispatch(app: &AppHandle, job: WindowJob) -> Result<()> {
    // 注意：不能用 `.context()` —— `SendError<WindowJob>` 不满足 `Sync`
    // （`dyn FnOnce` 不是 Sync），anyhow 的 Context 要求 Error + Send + Sync。
    dispatcher()
        .send((app.clone(), job))
        .map_err(|_| anyhow::anyhow!("投递窗口任务失败：投递线程已退出"))
}

// ------------------------------------------------------------ 打开 / 关闭

/// 打开（或聚焦）一张便签的独立窗口。异步执行，立即返回。
pub fn open_note_window(app: &AppHandle, note: &Note) -> Result<()> {
    let note = note.clone();
    dispatch(
        app,
        Box::new(move |app| {
            if let Err(e) = open_note_window_on_main(app, &note) {
                tracing::warn!("打开便签窗口 {} 失败: {e:#}", note.id);
            }
        }),
    )
}

/// 真正的建窗逻辑 —— 只在主线程（事件循环）上被调用
fn open_note_window_on_main(app: &AppHandle, note: &Note) -> Result<()> {
    let label = note_label(&note.id);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }

    let url = WebviewUrl::App(format!("note.html?id={}", note.id).into());
    let win = WebviewWindowBuilder::new(app, &label, url)
        .title("便签")
        .inner_size(note.width as f64, note.height as f64)
        .min_inner_size(180.0, 140.0)
        .position(note.x as f64, note.y as f64)
        .decorations(false)
        .resizable(true)
        .transparent(false)
        .shadow(true)
        .skip_taskbar(false)
        .always_on_top(note.is_pinned)
        .visible(false) // 等前端渲染完再显示，避免白屏闪烁
        .build()?;

    let _ = win.show();
    Ok(())
}

/// 关闭一张便签的窗口。异步执行，立即返回。
pub fn close_note_window(app: &AppHandle, id: &str) {
    let id = id.to_string();
    if let Err(e) = dispatch(
        app,
        Box::new(move |app| {
            if let Some(w) = app.get_webview_window(&note_label(&id)) {
                let _ = w.destroy();
            }
        }),
    ) {
        tracing::warn!("投递关闭窗口任务失败: {e:#}");
    }
}

/// 显示主窗口
pub fn show_main(app: &AppHandle) {
    if let Err(e) = dispatch(
        app,
        Box::new(move |app| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }),
    ) {
        tracing::warn!("投递显示主窗口任务失败: {e:#}");
    }
}

// ------------------------------------------------------------ 几何同步

/// 读取便签窗口的当前几何信息 —— 只在主线程上调用
fn read_geometry_on_main(app: &AppHandle, id: &str) -> Option<(i32, i32, i32, i32)> {
    let w = app.get_webview_window(&note_label(id))?;
    let pos: PhysicalPosition<i32> = w.outer_position().ok()?;
    let size: PhysicalSize<u32> = w.inner_size().ok()?;
    Some((pos.x, pos.y, size.width as i32, size.height as i32))
}

/// 供命令层使用：读取几何并落库，可附加额外字段（如 `is_open`）。
/// 投递到主线程执行，立即返回。
pub fn sync_geometry_with(
    app: &AppHandle,
    svc: &Arc<crate::service::AppService>,
    id: &str,
    extra: crate::models::NotePatch,
) {
    let svc = svc.clone();
    let id = id.to_string();
    let is_open = extra.is_open;
    if let Err(e) = dispatch(
        app,
        Box::new(move |app| {
            let mut patch = match read_geometry_on_main(app, &id) {
                Some((x, y, w, h)) => crate::models::NotePatch {
                    x: Some(x),
                    y: Some(y),
                    width: Some(w),
                    height: Some(h),
                    ..Default::default()
                },
                None => crate::models::NotePatch::default(),
            };
            patch.is_open = is_open;
            if let Err(e) = svc.update_note_silent(&id, &patch) {
                tracing::warn!("同步便签 {id} 几何信息失败: {e:#}");
            }
        }),
    ) {
        tracing::warn!("投递几何同步任务失败: {e:#}");
    }
}

/// 立即读取窗口几何并落库（关闭 / 拖动结束时调用）
pub fn sync_geometry_now(app: &AppHandle, svc: &Arc<crate::service::AppService>, id: &str) {
    sync_geometry_with(app, svc, id, crate::models::NotePatch::default())
}

/// 关闭所有便签窗口（退出时用）
pub fn close_all_note_windows(app: &AppHandle) {
    if let Err(e) = dispatch(
        app,
        Box::new(move |app| {
            for (_label, w) in app.webview_windows() {
                if w.label().starts_with("note-") {
                    let _ = w.destroy();
                }
            }
        }),
    ) {
        tracing::warn!("投递关闭全部窗口任务失败: {e:#}");
    }
}

/// 判断 label 是否是便签窗口，并取出便签 id
pub fn note_id_from_label(label: &str) -> Option<String> {
    label.strip_prefix("note-").map(|s| s.to_string())
}

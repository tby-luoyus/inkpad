//! 拓展中枢（Extension Hub）
//!
//! 这是整个应用「可扩展」的根基。四类拓展接口都挂在这里：
//!
//! 1. **事件总线** —— 所有数据变更都会广播出去，Webhook / 插件 / Agent 都能订阅
//! 2. **命令注册表** —— 插件与 Agent 通过统一的 `Command` 抽象操作应用
//! 3. **动作（Action）系统** —— 便签内的 `@动作` 语法，供 Agent 与自动化调用
//! 4. **主题包 / 插件清单** —— 从磁盘加载，热插拔
//!
//! 设计原则：核心不依赖任何具体插件。插件只依赖契约（JSON 结构），
//! 因此插件可以用任意语言编写（HTTP 协议通信）。

use chrono::Utc;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tokio::sync::broadcast;

use crate::models::Note;

// ============================================================ 事件总线

/// 应用内广播的事件类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppEvent {
    /// 事件名，如 `note.created`
    pub name: String,
    /// 事件负载
    pub payload: Value,
    /// 事件时间
    pub at: String,
    /// 事件来源：ui | http | plugin:<id> | agent | system
    pub source: String,
}

impl AppEvent {
    pub fn new(name: &str, payload: Value, source: &str) -> Self {
        Self {
            name: name.to_string(),
            payload,
            at: Utc::now().to_rfc3339(),
            source: source.to_string(),
        }
    }
}

/// 事件总线：内部广播 + 外发 Webhook
pub struct EventBus {
    tx: broadcast::Sender<AppEvent>,
    /// 已注册的 Webhook：id -> 配置
    webhooks: RwLock<HashMap<String, WebhookConfig>>,
    /// 事件历史环形缓冲，便于 Agent 回溯
    history: Mutex<Vec<AppEvent>>,
    client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    /// 订阅的事件名，支持前缀通配，如 `note.*`；`*` 表示全部
    pub events: Vec<String>,
    pub enabled: bool,
    /// 可选的自定义请求头
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(512);
        Self {
            tx,
            webhooks: RwLock::new(HashMap::new()),
            history: Mutex::new(Vec::with_capacity(256)),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(8))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<AppEvent> {
        self.tx.subscribe()
    }

    /// 广播事件：写入历史 → 推给内部订阅者 → 异步派发 Webhook
    pub fn emit(&self, event: AppEvent) {
        {
            let mut h = self.history.lock();
            h.push(event.clone());
            if h.len() > 500 {
                h.remove(0);
            }
        }
        let _ = self.tx.send(event.clone());
        self.dispatch_webhooks(event);
    }

    pub fn emit_note(&self, name: &str, note: &Note, source: &str) {
        self.emit(AppEvent::new(
            name,
            json!({
                "id": note.id,
                "title": note.title(),
                "content": note.content,
                "color": note.color,
                "pinned": note.is_pinned,
                "meta": note.meta,
            }),
            source,
        ));
    }

    pub fn recent(&self, limit: usize) -> Vec<AppEvent> {
        let h = self.history.lock();
        h.iter().rev().take(limit).cloned().collect::<Vec<_>>().into_iter().rev().collect()
    }

    pub fn webhooks(&self) -> Vec<WebhookConfig> {
        self.webhooks.read().values().cloned().collect()
    }

    pub fn upsert_webhook(&self, cfg: WebhookConfig) {
        self.webhooks.write().insert(cfg.id.clone(), cfg);
    }

    pub fn remove_webhook(&self, id: &str) -> bool {
        self.webhooks.write().remove(id).is_some()
    }

    fn dispatch_webhooks(&self, event: AppEvent) {
        let targets: Vec<WebhookConfig> = self
            .webhooks
            .read()
            .values()
            .filter(|w| w.enabled && w.events.iter().any(|p| matches_pattern(p, &event.name)))
            .cloned()
            .collect();
        if targets.is_empty() {
            return;
        }
        let client = self.client.clone();
        spawn_async(async move {
            for w in targets {
                let mut req = client.post(&w.url).json(&event);
                for (k, v) in &w.headers {
                    req = req.header(k, v);
                }
                if let Err(e) = req.send().await {
                    tracing::warn!("webhook {} 投递失败: {e}", w.id);
                }
            }
        });
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

/// 通配匹配：`note.*` / `*`
pub fn matches_pattern(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return name.starts_with(prefix);
    }
    pattern == name
}

// ======================================================== 命令注册表

/// 拓展可以调用的命令签名（供插件自描述 / 文档生成）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSpec {
    pub name: String,
    pub description: String,
    /// 参数 JSON Schema（简化描述）
    pub params: Value,
    pub category: String,
}

/// 命令注册表：核心命令 + 插件注册的命令
pub struct CommandRegistry {
    specs: RwLock<HashMap<String, CommandSpec>>,
    /// 插件注册的扩展命令处理器：name -> plugin_id
    plugin_handlers: RwLock<HashMap<String, String>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        let me = Self {
            specs: RwLock::new(HashMap::new()),
            plugin_handlers: RwLock::new(HashMap::new()),
        };
        me.register_core_specs();
        me
    }

    fn register_core_specs(&self) {
        let core = vec![
            ("notes.list", "列出便签", "notes", json!({"keyword":"string?","includeDeleted":"bool?"})),
            ("notes.get", "获取单张便签", "notes", json!({"id":"string"})),
            ("notes.create", "新建便签", "notes", json!({"content":"string?","color":"string?","open":"bool?"})),
            ("notes.update", "更新便签", "notes", json!({"id":"string","patch":"NotePatch"})),
            ("notes.append", "在便签末尾追加内容（Agent 常用）", "notes", json!({"id":"string","text":"string"})),
            ("notes.replace", "替换便签全文", "notes", json!({"id":"string","content":"string"})),
            ("notes.trash", "移入回收站", "notes", json!({"id":"string"})),
            ("notes.restore", "从回收站恢复", "notes", json!({"id":"string"})),
            ("notes.purge", "彻底删除", "notes", json!({"id":"string"})),
            ("notes.pin", "设置置顶", "notes", json!({"id":"string","pinned":"bool"})),
            ("notes.setColor", "设置颜色", "notes", json!({"id":"string","color":"string"})),
            ("notes.duplicate", "复制便签", "notes", json!({"id":"string"})),
            ("notes.search", "搜索便签", "notes", json!({"keyword":"string"})),
            ("window.openNote", "打开便签为独立窗口", "window", json!({"id":"string"})),
            ("window.closeNote", "关闭便签窗口", "window", json!({"id":"string"})),
            ("window.openMain", "显示主窗口", "window", json!({})),
            ("app.emit", "广播自定义事件", "system", json!({"name":"string","payload":"object?"})),
            ("app.settings", "读取设置", "system", json!({})),
            ("app.updateSettings", "更新设置", "system", json!({"patch":"SettingsPatch"})),
            ("app.export", "导出全部便签", "system", json!({"path":"string?"})),
            ("app.import", "导入便签", "system", json!({"path":"string","overwrite":"bool?"})),
            ("theme.list", "列出主题包", "theme", json!({})),
            ("theme.set", "切换主题包", "theme", json!({"id":"string"})),
            ("plugin.list", "列出插件", "plugin", json!({})),
            ("plugin.enable", "启用/停用插件", "plugin", json!({"id":"string","enabled":"bool"})),
            ("plugin.startProcess", "启动插件后端进程", "plugin", json!({"id":"string"})),
            ("plugin.stopProcess", "停止插件后端进程", "plugin", json!({"id":"string"})),
            ("events.recent", "读取最近事件", "system", json!({"limit":"number?"})),
            ("webhook.list", "列出 Webhook", "system", json!({})),
            ("webhook.upsert", "添加/更新 Webhook", "system", json!({"config":"WebhookConfig"})),
            ("webhook.remove", "删除 Webhook", "system", json!({"id":"string"})),
            // ---------------------------------------------------- 笔记（Vault）
            ("doc.list", "列出笔记", "doc", json!({"folder":"string?","tag":"string?"})),
            ("doc.tree", "读取笔记目录树", "doc", json!({})),
            ("doc.info", "笔记库概览（数量/标签/孤立笔记）", "doc", json!({})),
            ("doc.tags", "全部标签及计数", "doc", json!({})),
            ("doc.get", "读取一篇笔记", "doc", json!({"id":"string"})),
            ("doc.search", "全文检索笔记", "doc", json!({"keyword":"string"})),
            ("doc.create", "新建笔记", "doc", json!({"path":"string","content":"string?","frontmatter":"object?"})),
            ("doc.save", "保存笔记正文", "doc", json!({"id":"string","content":"string"})),
            ("doc.rename", "重命名 / 移动笔记", "doc", json!({"id":"string","path":"string"})),
            ("doc.delete", "删除笔记", "doc", json!({"id":"string"})),
            ("doc.backlinks", "查询反向链接", "doc", json!({"id":"string"})),
            ("doc.outlinks", "查询出链", "doc", json!({"id":"string"})),
            ("doc.reindex", "重建笔记索引", "doc", json!({})),
            (
                "doc.fromNotes",
                "把便签汇总成一篇笔记",
                "doc",
                json!({"ids":"string[]?","title":"string?","folder":"string?"}),
            ),
        ];
        let mut map = self.specs.write();
        for (name, desc, cat, params) in core {
            map.insert(
                name.to_string(),
                CommandSpec {
                    name: name.to_string(),
                    description: desc.to_string(),
                    category: cat.to_string(),
                    params,
                },
            );
        }
    }

    /// 注册一条插件命令
    pub fn register_plugin_command(&self, spec: CommandSpec, plugin_id: &str) {
        self.plugin_handlers
            .write()
            .insert(spec.name.clone(), plugin_id.to_string());
        self.specs.write().insert(spec.name.clone(), spec);
    }

    pub fn owner_of(&self, name: &str) -> Option<String> {
        self.plugin_handlers.read().get(name).cloned()
    }

    pub fn unregister_plugin(&self, plugin_id: &str) {
        let dead: Vec<String> = self
            .plugin_handlers
            .read()
            .iter()
            .filter(|(_, v)| v.as_str() == plugin_id)
            .map(|(k, _)| k.clone())
            .collect();
        for name in dead {
            self.plugin_handlers.write().remove(&name);
            self.specs.write().remove(&name);
        }
    }

    pub fn all(&self) -> Vec<CommandSpec> {
        let mut v: Vec<CommandSpec> = self.specs.read().values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// =========================================================== 主题 / 插件

/// 主题包清单（`themes/<id>/theme.json`）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemePack {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    /// 亮色 / 暗色
    #[serde(default = "default_appearance")]
    pub appearance: String,
    /// 便签配色：色 id -> 十六进制
    pub colors: HashMap<String, String>,
    /// 全局 CSS 变量覆盖
    #[serde(default)]
    pub css_vars: HashMap<String, String>,
    /// 额外 CSS（相对主题目录的路径）
    pub css_file: Option<String>,
    /// 便签卡片自定义渲染器（相对主题目录的 JS 路径）
    pub renderer: Option<String>,
    /// 内置主题标志（内置的不可删除）
    #[serde(default)]
    pub builtin: bool,
    /// 主题目录
    #[serde(skip)]
    pub dir: Option<PathBuf>,
}

fn default_version() -> String {
    "1.0.0".into()
}
fn default_appearance() -> String {
    "light".into()
}

impl ThemePack {
    /// 内置默认主题：与微软便笺一致的六色
    pub fn builtin_default() -> Self {
        let mut colors = HashMap::new();
        colors.insert("yellow".into(), "#FFF8B8".into());
        colors.insert("green".into(), "#DDF0C8".into());
        colors.insert("pink".into(), "#FDD3E1".into());
        colors.insert("purple".into(), "#E6DCF8".into());
        colors.insert("blue".into(), "#CDE6F7".into());
        colors.insert("gray".into(), "#E6E6E6".into());

        let mut css_vars = HashMap::new();
        css_vars.insert("--sn-font".into(), "'Segoe UI Variable Text', 'Segoe UI', system-ui, -apple-system, 'Microsoft YaHei UI', sans-serif".into());
        css_vars.insert("--sn-radius".into(), "8px".into());

        Self {
            id: "default".into(),
            name: "默认（微软便笺风格）".into(),
            author: "Built-in".into(),
            description: "对齐 Windows 便笺的经典六色配色".into(),
            version: "1.0.0".into(),
            appearance: "system".into(),
            colors,
            css_vars,
            css_file: None,
            renderer: None,
            builtin: true,
            dir: None,
        }
    }

    pub fn builtin_midnight() -> Self {
        let mut colors = HashMap::new();
        colors.insert("yellow".into(), "#4A4327".into());
        colors.insert("green".into(), "#2F4029".into());
        colors.insert("pink".into(), "#4A2B38".into());
        colors.insert("purple".into(), "#3C3350".into());
        colors.insert("blue".into(), "#28394A".into());
        colors.insert("gray".into(), "#333333".into());

        let mut css_vars = HashMap::new();
        css_vars.insert("--sn-note-text".into(), "#F3F3F3".into());
        css_vars.insert("--sn-radius".into(), "8px".into());

        Self {
            id: "midnight".into(),
            name: "午夜".into(),
            author: "Built-in".into(),
            description: "暗色便签，适合夜间使用".into(),
            version: "1.0.0".into(),
            appearance: "dark".into(),
            colors,
            css_vars,
            css_file: None,
            renderer: None,
            builtin: true,
            dir: None,
        }
    }

    pub fn builtins() -> Vec<Self> {
        vec![Self::builtin_default(), Self::builtin_midnight()]
    }
}

/// 插件清单（`plugins/<id>/plugin.json`）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    /// 权限声明，用于 UI 展示风险
    #[serde(default)]
    pub permissions: Vec<String>,
    /// 订阅的事件
    #[serde(default)]
    pub events: Vec<String>,
    /// 注册的命令（转发到 webhook）
    #[serde(default)]
    pub commands: Vec<PluginCommandDecl>,
    /// 插件后端地址：所有事件 POST 到此，命令也由此处理
    pub endpoint: Option<String>,
    /// 纯前端插件：注入的 JS（相对插件目录）
    pub frontend: Option<String>,
    /// 是否默认启用
    #[serde(default)]
    pub enabled: bool,
    /// 可选：随应用启动的插件进程（如 `python server.py`）
    #[serde(default)]
    pub auto_start: Option<AutoStart>,
    #[serde(skip)]
    pub dir: Option<PathBuf>,
}

/// 插件自启动声明：让插件后端无需用户手动拉起
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoStart {
    /// 可执行文件，如 `python`、`node`
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// 相对插件目录的工作目录
    #[serde(default)]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCommandDecl {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub params: Value,
}

/// 插件运行时状态
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRuntime {
    #[serde(flatten)]
    pub manifest: PluginManifest,
    /// 磁盘路径
    pub path: String,
    /// 是否已加载到当前进程
    pub loaded: bool,
    /// 自启动进程是否正在运行
    #[serde(default)]
    pub process_running: bool,
}

/// 拓展中枢：把事件总线、命令注册表、插件表聚在一起，作为全局共享状态
pub struct Hub {
    pub events: EventBus,
    pub commands: CommandRegistry,
    pub plugins: RwLock<HashMap<String, PluginManifest>>,
    /// 由本应用拉起的插件子进程：plugin id -> 句柄
    pub plugin_procs: RwLock<HashMap<String, std::process::Child>>,
    /// Tauri AppHandle：事件需要转发给前端（webview）时用。
    /// 在 setup 阶段注入，之后只读。
    pub app: RwLock<Option<tauri::AppHandle>>,
}

/// 一个便捷的共享状态容器，供 Tauri 命令与 HTTP 服务共用
pub type SharedHub = Arc<Hub>;

/// 把内部事件名转成合法的 Tauri 事件名。
///
/// Tauri 2 只允许字母数字与 `-`、`/`、`:`、`_`，而本项目的事件名沿用
/// `note.created` 这种带点的写法（更符合 Web 习惯，也用于插件/webhook 契约）。
/// 因此在「发给前端」这一层做一次转换：`.` → `:`。
///
/// 前端订阅时用同样的函数转换，业务代码无需改事件名。
pub fn to_tauri_event_name(name: &str) -> String {
    name.replace('.', ":")
}

impl Hub {
    pub fn new() -> Self {
        Self {
            events: EventBus::new(),
            commands: CommandRegistry::new(),
            plugins: RwLock::new(HashMap::new()),
            plugin_procs: RwLock::new(HashMap::new()),
            app: RwLock::new(None),
        }
    }

    /// 注入 AppHandle（setup 阶段调用一次）
    pub fn attach_app(&self, app: tauri::AppHandle) {
        *self.app.write() = Some(app);
    }

    pub fn emit(&self, event: AppEvent) {
        self.forward_to_frontend(&event);
        self.events.emit(event);
    }

    /// 把事件推给前端 webview（Tauri 原生事件通道）
    ///
    /// **必须异步**：`app.emit()` 会同步分发到 webview，如果 webview 正忙，
    /// 同步调用会把调用方一起卡住 —— 而调用方常常是 HTTP 请求线程，
    /// 一卡就是整个接口无响应。这里投递到常驻投递线程，调用方立即返回。
    fn forward_to_frontend(&self, event: &AppEvent) {
        let app = self.app.read().clone();
        let Some(app) = app else { return };
        let name = to_tauri_event_name(&event.name);
        push_frontend_event(app, name, event.clone());
    }

    /// 广播便签事件 —— 内部事件总线 + 订阅它的插件后端，一次到位
    pub fn broadcast_note(&self, name: &str, note: &Note, source: &str) {
        let event = note_event(name, note, source);
        self.forward_to_frontend(&event);
        self.events.emit(event.clone());
        self.dispatch_to_plugins(event);
    }

    /// 把事件推送给订阅的插件后端
    pub fn dispatch_to_plugins(&self, event: AppEvent) {
        let targets: Vec<(String, String)> = self
            .plugins
            .read()
            .values()
            .filter(|p| p.enabled && p.endpoint.is_some())
            .filter(|p| p.events.iter().any(|pat| matches_pattern(pat, &event.name)))
            .filter_map(|p| p.endpoint.clone().map(|e| (p.id.clone(), e)))
            .collect();
        if targets.is_empty() {
            return;
        }
        let client = shared_http_client();
        spawn_async(async move {
            for (id, endpoint) in targets {
                if let Err(err) = client
                    .post(&endpoint)
                    .header("x-plugin-id", &id)
                    .json(&event)
                    .send()
                    .await
                {
                    tracing::warn!("插件 {id} 事件投递失败: {err}");
                }
            }
        });
    }

    /// 通用广播：内部总线 + 插件后端 + 前端 webview
    pub fn broadcast(&self, name: &str, payload: Value, source: &str) {
        let event = AppEvent::new(name, payload, source);
        self.forward_to_frontend(&event);
        self.events.emit(event.clone());
        self.dispatch_to_plugins(event);
    }
}

/// 一次前端投递任务
type EmitJob = (tauri::AppHandle, String, AppEvent);

/// 前端事件投递队列（有界）+ 常驻投递线程。
///
/// 曾经的实现是「一个事件 spawn 一个线程」，事件密集时线程数会失控；
/// 改为单线程 + 有界队列后，最坏情况是丢弃事件并打日志，
/// 而不是把调用方（命令线程 / HTTP 线程）拖死。
static EMITTER_TX: OnceLock<std::sync::mpsc::SyncSender<EmitJob>> = OnceLock::new();

fn emitter_tx() -> &'static std::sync::mpsc::SyncSender<EmitJob> {
    EMITTER_TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::sync_channel::<EmitJob>(512);
        std::thread::Builder::new()
            .name("sticky-emitter".into())
            .spawn(move || {
                use tauri::Emitter;
                while let Ok((app, name, event)) = rx.recv() {
                    // 前端没订阅 / 窗口已关闭属于正常情况
                    if let Err(e) = app.emit(&name, &event) {
                        tracing::debug!("事件 {name} 未送达前端: {e}");
                    }
                }
            })
            .expect("启动前端事件投递线程失败");
        tx
    })
}

/// 投递一个事件到前端；队列满时丢弃并告警（绝不阻塞调用方）
pub fn push_frontend_event(app: tauri::AppHandle, name: String, event: AppEvent) {
    use std::sync::mpsc::TrySendError;
    match emitter_tx().try_send((app, name.clone(), event)) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            tracing::warn!("前端事件队列已满，丢弃事件 {name}（webview 可能已卡住）");
        }
        Err(TrySendError::Disconnected(_)) => {
            tracing::debug!("前端事件投递线程已退出，丢弃事件 {name}");
        }
    }
}

/// 构造便签事件的标准负载形状
pub fn note_event(name: &str, note: &Note, source: &str) -> AppEvent {
    AppEvent::new(
        name,
        serde_json::json!({
            "id": note.id,
            "title": note.title(),
            "content": note.content,
            "color": note.color,
            "pinned": note.is_pinned,
            "meta": note.meta,
        }),
        source,
    )
}

/// 共享的异步 HTTP 客户端（避免每次事件都新建连接池）
pub fn shared_http_client() -> reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default()
        })
        .clone()
}

/// 后台异步运行时句柄。
///
/// 事件派发（Webhook / 插件）可能从任意线程触发——Tauri 命令线程、HTTP 线程池、
/// 甚至插件进程管理线程——这些线程未必处于 Tokio 运行时上下文中。
/// 直接调用 `tokio::spawn` 会 panic："there is no reactor running"。
///
/// 因此这里惰性创建一个常驻后台运行时，所有异步派发都通过它的 `Handle::spawn`
/// 投递，保证在任何调用点都安全。
pub fn runtime_handle() -> &'static tokio::runtime::Handle {
    use std::sync::OnceLock;
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("sticky-async")
            .build()
            .expect("构建后台异步运行时失败")
    })
    .handle()
}

/// 在后台运行时里投递一个异步任务（可从任意线程安全调用）
pub fn spawn_async<F>(fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    runtime_handle().spawn(fut);
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

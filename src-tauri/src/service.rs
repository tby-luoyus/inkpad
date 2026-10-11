//! 应用服务层
//!
//! UI（Tauri 命令）与本地 HTTP API 共用同一套业务逻辑，
//! 避免出现「界面能做的事 API 做不了」这种割裂。

use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

use crate::hub::{self, AppEvent, CommandSpec, PluginManifest, PluginRuntime, SharedHub, ThemePack};
use crate::models::{BackupFile, Doc, DocNode, DocPatch, Note, NotePatch, NoteQuery, OpResult, Settings, VaultInfo};
use crate::store::Store;
use crate::vault::Vault;
use crate::window;

pub struct AppService {
    pub store: Store,
    /// 笔记库（对标 Obsidian 的 Markdown vault）
    pub vault: Vault,
    pub data_dir: PathBuf,
    pub themes: RwLock<Vec<ThemePack>>,
    pub http_running: RwLock<Option<u16>>,
}

impl AppService {
    pub fn new(data_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&data_dir)?;
        let store = Store::open(&data_dir.join("notes.db"))?;
        // 笔记库默认放在数据目录下的 vault/，也可以指向任何你已有的 Obsidian 仓库
        let vault = Vault::open(data_dir.join("vault"))?;
        let svc = Self {
            store,
            vault,
            data_dir,
            themes: RwLock::new(Vec::new()),
            http_running: RwLock::new(None),
        };
        svc.reload_themes();
        Ok(svc)
    }

    // ------------------------------------------------------------ 路径

    pub fn plugins_dir(&self) -> PathBuf {
        self.data_dir.join("plugins")
    }
    pub fn themes_dir(&self) -> PathBuf {
        self.data_dir.join("themes")
    }
    pub fn backups_dir(&self) -> PathBuf {
        self.data_dir.join("backups")
    }
    pub fn vault_dir(&self) -> PathBuf {
        self.vault.root.clone()
    }
    /// 插件后端进程的输出日志目录
    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("plugin-logs")
    }

    pub fn ensure_layout(&self) -> Result<()> {
        for d in [
            self.plugins_dir(),
            self.themes_dir(),
            self.backups_dir(),
            self.vault_dir(),
            self.logs_dir(),
        ] {
            std::fs::create_dir_all(&d)?;
        }
        Ok(())
    }

    pub fn dir_for(&self, kind: &str) -> Option<PathBuf> {
        Some(match kind {
            "data" => self.data_dir.clone(),
            "plugins" => self.plugins_dir(),
            "themes" => self.themes_dir(),
            "backups" => self.backups_dir(),
            "vault" | "notes" => self.vault_dir(),
            "logs" => self.logs_dir(),
            _ => return None,
        })
    }

    pub fn paths_info(&self) -> Value {
        json!({
            "data": self.data_dir.to_string_lossy(),
            "plugins": self.plugins_dir().to_string_lossy(),
            "themes": self.themes_dir().to_string_lossy(),
            "backups": self.backups_dir().to_string_lossy(),
            "vault": self.vault_dir().to_string_lossy(),
            "logs": self.logs_dir().to_string_lossy(),
            "database": self.store.db_path.to_string_lossy(),
            "http": self.api_status(),
        })
    }

    // ---------------------------------------------------------- 笔记库

    pub fn vault_list(&self, folder: Option<String>, tag: Option<String>) -> Result<Vec<Doc>> {
        self.vault.list(folder.as_deref(), tag.as_deref())
    }
    pub fn vault_tree(&self) -> Result<Vec<DocNode>> {
        self.vault.tree()
    }
    pub fn vault_info(&self) -> Result<VaultInfo> {
        self.vault.info()
    }
    pub fn vault_read(&self, id: &str) -> Result<Option<Doc>> {
        self.vault.read(id)
    }
    pub fn vault_create(&self, patch: DocPatch) -> Result<Doc> {
        self.vault.create(patch)
    }
    pub fn vault_save(&self, id: &str, patch: DocPatch) -> Result<Option<Doc>> {
        self.vault.save(id, patch)
    }
    pub fn vault_rename(&self, id: &str, new_path: &str) -> Result<Option<Doc>> {
        self.vault.rename(id, new_path)
    }
    pub fn vault_delete(&self, id: &str) -> Result<bool> {
        self.vault.delete(id)
    }
    pub fn vault_search(&self, keyword: &str) -> Result<Vec<Doc>> {
        self.vault.search(keyword)
    }
    pub fn vault_backlinks(&self, id: &str) -> Result<Vec<crate::models::Backlink>> {
        self.vault.backlinks(id)
    }
    pub fn vault_outlinks(&self, id: &str) -> Result<Vec<Doc>> {
        self.vault.outlinks(id)
    }
    pub fn vault_tags(&self) -> Result<Vec<(String, usize)>> {
        self.vault.tags()
    }
    pub fn vault_reindex(&self) -> Result<usize> {
        self.vault.reindex()
    }

    // ---------------------------------------------------------- 便签读写

    pub fn list_notes(&self, q: &NoteQuery) -> Result<Vec<Note>> {
        self.store.list_notes(q)
    }

    pub fn get_note(&self, id: &str) -> Result<Option<Note>> {
        self.store.get_note(id)
    }

    pub fn counts(&self) -> Result<(usize, usize, usize)> {
        self.store.count_all()
    }

    pub fn create_note(&self, patch: NotePatch, open: bool) -> Result<Note> {
        let settings = self.store.get_settings()?;
        let mut n = self.store.create_note(patch, &settings)?;
        if open {
            n = self
                .store
                .update_note(&n.id, &NotePatch { is_open: Some(true), ..Default::default() })?
                .unwrap_or(n);
        }
        Ok(n)
    }

    pub fn update_note(&self, id: &str, patch: &NotePatch) -> Result<Option<Note>> {
        self.store.update_note(id, patch)
    }

    /// 不广播、不更新时间戳的静默更新（窗口几何同步用）
    pub fn update_note_silent(&self, id: &str, patch: &NotePatch) -> Result<Option<Note>> {
        self.store.update_note(id, patch)
    }

    pub fn append_to_note(&self, id: &str, text: &str) -> Result<Option<Note>> {
        let Some(n) = self.store.get_note(id)? else {
            return Ok(None);
        };
        let mut content = n.content.clone();
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(text);
        self.store.update_note(id, &NotePatch { content: Some(content), ..Default::default() })
    }

    pub fn trash_note(&self, id: &str) -> Result<bool> {
        self.store.trash_note(id)
    }
    pub fn restore_note(&self, id: &str) -> Result<bool> {
        self.store.restore_note(id)
    }
    pub fn purge_note(&self, id: &str) -> Result<bool> {
        self.store.purge_note(id)
    }
    pub fn purge_trash(&self) -> Result<usize> {
        self.store.purge_all_trashed()
    }
    pub fn duplicate_note(&self, id: &str) -> Result<Option<Note>> {
        let settings = self.store.get_settings()?;
        self.store.duplicate_note(id, &settings)
    }
    pub fn bring_to_front(&self, id: &str) -> Result<i64> {
        self.store.bring_to_front(id)
    }

    /// 置前并返回最新便签（窗口聚焦时用）
    pub fn bring_to_front_and_get(&self, id: &str) -> Result<Option<Note>> {
        let z = self.store.bring_to_front(id)?;
        let mut n = self.store.get_note(id)?;
        if let Some(note) = n.as_mut() {
            note.z_index = z;
        }
        Ok(n)
    }

    pub fn set_pinned(&self, id: &str, pinned: bool) -> Result<bool> {
        Ok(self
            .store
            .update_note(id, &NotePatch { is_pinned: Some(pinned), ..Default::default() })?
            .is_some())
    }

    pub fn set_color(&self, id: &str, color: &str) -> Result<bool> {
        Ok(self
            .store
            .update_note(id, &NotePatch { color: Some(color.to_string()), ..Default::default() })?
            .is_some())
    }

    pub fn set_hidden(&self, id: &str, hidden: bool) -> Result<bool> {
        Ok(self
            .store
            .update_note(id, &NotePatch { hidden: Some(hidden), ..Default::default() })?
            .is_some())
    }

    pub fn set_open(&self, id: &str, open: bool) -> Result<bool> {
        Ok(self
            .store
            .update_note(id, &NotePatch { is_open: Some(open), ..Default::default() })?
            .is_some())
    }

    /// 新便签的默认落点：以屏幕中央为基准做阶梯偏移
    pub fn suggest_position(&self) -> Result<(i32, i32)> {
        let base = self.data_dir.clone();
        let _ = base;
        let (total, _, _) = self.store.count_all()?;
        let step = 32i32;
        let (sx, sy) = (520i32, 260i32);
        Ok((sx + (total as i32 % 8) * step, sy + (total as i32 % 8) * step))
    }

    // ------------------------------------------------------------- 设置

    pub fn settings(&self) -> Result<Settings> {
        self.store.get_settings()
    }

    pub fn save_settings(&self, s: &Settings) -> Result<Settings> {
        self.store.save_settings(s)?;
        Ok(s.clone())
    }

    // ------------------------------------------------------- 备份 / 导入

    pub fn export_backup(&self) -> Result<BackupFile> {
        self.store.export_backup(true)
    }

    pub fn export_to_path(&self, path: &str) -> Result<usize> {
        let b = self.store.export_backup(true)?;
        let n = b.notes.len();
        std::fs::write(path, serde_json::to_string_pretty(&b)?)?;
        Ok(n)
    }

    pub fn import_from_path(&self, path: &str, overwrite: bool) -> Result<usize> {
        let raw = std::fs::read_to_string(path)?;
        let b: BackupFile = serde_json::from_str(&raw).map_err(|e| anyhow!("备份文件格式错误: {e}"))?;
        let n = self.store.import_backup(&b, overwrite)?;
        if let Some(s) = &b.settings {
            self.store.save_settings(s).ok();
        }
        Ok(n)
    }

    pub fn export_to_dialog(&self, app: &AppHandle) -> Result<String> {
        let default_name = format!(
            "便签备份-{}.json",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        let dir = self.backups_dir();
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join(default_name);
        let b = self.store.export_backup(true)?;
        std::fs::write(&path, serde_json::to_string_pretty(&b)?)?;
        let _ = app;
        Ok(path.to_string_lossy().to_string())
    }

    pub fn import_from_dialog(&self, app: &AppHandle) -> Result<OpResult> {
        use tauri_plugin_dialog::DialogExt;
        let picked = app
            .dialog()
            .file()
            .add_filter("便签备份", &["json"])
            .blocking_pick_file();
        let Some(p) = picked else {
            return Ok(OpResult { ok: false, affected: 0, message: "已取消".into() });
        };
        let path = p
            .into_path()
            .map_err(|e| anyhow!("路径解析失败: {e}"))?;
        let n = self.import_from_path(&path.to_string_lossy(), true)?;
        Ok(OpResult { ok: true, affected: n, message: format!("已导入 {n} 条便签") })
    }

    // ------------------------------------------------------------ 主题包

    /// 扫描主题目录并合并内置主题
    pub fn reload_themes(&self) {
        let mut list = ThemePack::builtins();
        let dir = self.themes_dir();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                let manifest = if p.is_dir() { p.join("theme.json") } else { p.clone() };
                if !manifest.exists() {
                    continue;
                }
                match std::fs::read_to_string(&manifest) {
                    Ok(raw) => match serde_json::from_str::<ThemePack>(&raw) {
                        Ok(mut t) => {
                            t.dir = Some(manifest.parent().unwrap_or(&p).to_path_buf());
                            // 内置 id 被覆盖时以磁盘为准
                            list.retain(|x| x.id != t.id);
                            list.push(t);
                        }
                        Err(err) => tracing::warn!("主题 {} 解析失败: {err}", manifest.display()),
                    },
                    Err(err) => tracing::warn!("主题 {} 读取失败: {err}", manifest.display()),
                }
            }
        }
        *self.themes.write() = list;
    }

    pub fn themes(&self) -> Vec<ThemePack> {
        self.themes.read().clone()
    }

    pub fn theme_by_id(&self, id: &str) -> Option<ThemePack> {
        self.themes.read().iter().find(|t| t.id == id).cloned()
    }

    /// 读取主题附加 CSS 内容
    pub fn theme_css(&self, id: &str) -> Option<String> {
        let t = self.theme_by_id(id)?;
        let file = t.css_file?;
        let dir = t.dir?;
        std::fs::read_to_string(dir.join(file)).ok()
    }

    // ------------------------------------------------------------- 插件

    pub fn load_plugins(&self, hub: &SharedHub, app: Option<&AppHandle>) -> Result<()> {
        let dir = self.plugins_dir();
        std::fs::create_dir_all(&dir).ok();
        let mut found: Vec<PluginManifest> = Vec::new();

        if let Ok(entries) = std::fs::read_dir(&dir) {
            for e in entries.flatten() {
                let p = e.path();
                let manifest_path = if p.is_dir() { p.join("plugin.json") } else { p.clone() };
                if !manifest_path.exists() {
                    continue;
                }
                match std::fs::read_to_string(&manifest_path) {
                    Ok(raw) => match serde_json::from_str::<PluginManifest>(&raw) {
                        Ok(mut m) => {
                            m.dir = Some(manifest_path.parent().unwrap_or(&p).to_path_buf());
                            found.push(m);
                        }
                        Err(err) => tracing::warn!("插件 {} 解析失败: {err}", manifest_path.display()),
                    },
                    Err(err) => tracing::warn!("插件 {} 读取失败: {err}", manifest_path.display()),
                }
            }
        }

        let mut map = hub.plugins.write();
        map.clear();
        let mut to_start: Vec<String> = Vec::new();
        for m in found {
            if m.enabled {
                for c in &m.commands {
                    hub.commands.register_plugin_command(
                        CommandSpec {
                            name: c.name.clone(),
                            description: if c.description.is_empty() {
                                format!("{} 提供的命令", m.name)
                            } else {
                                c.description.clone()
                            },
                            params: c.params.clone(),
                            category: format!("plugin:{}", m.id),
                        },
                        &m.id,
                    );
                }
                if m.auto_start.is_some() {
                    to_start.push(m.id.clone());
                }
            }
            map.insert(m.id.clone(), m);
        }
        drop(map);
        self.install_frontend_plugins(hub, app);
        // 自启动放在最后：避免持有 plugins 写锁时启动进程
        if !to_start.is_empty() {
            // 插件后端靠本机 HTTP 接口读写便签；若接口没开，插件会「起来了但用不了」。
            // 这里提前给出明确警告，避免用户对着一堆插件报错找不到原因。
            let api_on = self
                .settings()
                .map(|s| s.http_api_enabled)
                .unwrap_or(false);
            if !api_on {
                tracing::warn!(
                    "有 {} 个插件需要本机 HTTP 接口，但「设置 → 开发者 → 启用本地 HTTP API」当前是关闭的，\
                     插件将无法读写便签（可在设置中开启）",
                    to_start.len()
                );
            }
        }
        for id in to_start {
            if let Err(e) = self.start_plugin_process(hub, &id) {
                tracing::warn!("插件 {id} 自启动失败: {e:#}");
            }
        }
        Ok(())
    }

    /// 纯前端插件：把它们的 JS 暴露给前端（前端通过读取路径自行 import）
    fn install_frontend_plugins(&self, hub: &SharedHub, _app: Option<&AppHandle>) {
        let list: Vec<String> = hub
            .plugins
            .read()
            .values()
            .filter(|p| p.enabled && p.frontend.is_some())
            .filter_map(|p| {
                let d = p.dir.clone()?;
                let f = p.frontend.clone().unwrap();
                Some(d.join(f).to_string_lossy().to_string())
            })
            .collect();
        if !list.is_empty() {
            tracing::info!("已加载 {} 个前端插件", list.len());
        }
    }

    pub fn reload_plugins(&self, hub: &SharedHub) -> Result<()> {
        self.load_plugins(hub, None)
    }

    pub fn plugin_runtimes(&self, hub: &SharedHub) -> Vec<PluginRuntime> {
        // 顺带清理已退出的子进程记录，并返回仍存活的插件 id 集合
        let running: std::collections::HashSet<String> = {
            let mut procs = hub.plugin_procs.write();
            procs.retain(|id, c| match c.try_wait() {
                Ok(Some(status)) => {
                    tracing::info!("插件进程 {id} 已退出（{status}），移除记录");
                    false
                }
                Ok(None) => true,   // 仍在运行
                Err(e) => {
                    tracing::warn!("无法查询插件进程 {id} 状态: {e}");
                    false
                }
            });
            procs.keys().cloned().collect()
        };
        hub.plugins
            .read()
            .values()
            .map(|m| PluginRuntime {
                manifest: m.clone(),
                path: m
                    .dir
                    .as_ref()
                    .map(|d| d.to_string_lossy().to_string())
                    .unwrap_or_default(),
                loaded: m.enabled,
                process_running: running.contains(&m.id),
            })
            .collect()
    }

    /// 拉起插件的自启动进程（若 plugin.json 声明了 autoStart）
    pub fn start_plugin_process(&self, hub: &SharedHub, id: &str) -> Result<bool> {
        let (dir, auto, endpoint) = {
            let map = hub.plugins.read();
            let Some(m) = map.get(id) else {
                return Err(anyhow!("插件不存在: {id}"));
            };
            (m.dir.clone(), m.auto_start.clone(), m.endpoint.clone())
        };
        let Some(auto) = auto else {
            return Ok(false);
        };
        {
            // 已在运行则不重复拉起
            let procs = hub.plugin_procs.read();
            if procs.contains_key(id) {
                return Ok(true);
            }
        }

        // 进程可能已由上一次会话遗留（强制退出 / 崩溃时子进程不会被回收），
        // 此时端口已被占用。先探测 endpoint，若已有实例在服务，就直接复用它，
        // 否则新进程会因端口冲突启动失败。
        if let Some(ep) = &endpoint {
            if plugin_endpoint_alive(ep) {
                tracing::info!("插件 {id} 已有实例在运行（{ep}），跳过拉起");
                return Ok(true);
            }
        } else {
            tracing::debug!("插件 {id} 未声明 endpoint，直接拉起进程");
        }

        let cwd = match (&dir, &auto.cwd) {
            (Some(d), Some(rel)) => d.join(rel),
            (Some(d), None) => d.clone(),
            _ => std::env::current_dir()?,
        };

        // 解析可执行文件路径：
        // Windows 上 Command::new 对相对路径的查找规则不稳（不完全依赖 current_dir），
        // 因此这里显式把它拼成绝对路径。插件目录下的相对命令（如 "bin/x.exe"）
        // 会得到正确处理；系统命令（如 "python"）不含路径分隔符，保持原样交给 PATH 查找。
        let program = {
            let raw = std::path::Path::new(&auto.command);
            let has_sep = auto.command.contains('/') || auto.command.contains('\\');
            if raw.is_absolute() {
                auto.command.clone()
            } else if has_sep {
                let joined = cwd.join(&auto.command);
                if joined.exists() {
                    joined.to_string_lossy().to_string()
                } else if let Some(d) = &dir {
                    d.join(&auto.command).to_string_lossy().to_string()
                } else {
                    auto.command.clone()
                }
            } else {
                auto.command.clone()
            }
        };
        if has_sep_hint(&program) && !std::path::Path::new(&program).exists() {
            tracing::warn!("插件 {id} 的可执行文件不存在: {program}");
        }

        let mut cmd = std::process::Command::new(&program);
        cmd.args(&auto.args).current_dir(&cwd);

        // 插件后端的输出要留下来：插件常见的失败（缺依赖、端口冲突、
        // 忘了开 HTTP API）都只在 stderr 里说明，丢掉就完全查不到原因。
        // 写到 <数据目录>/plugin-logs/<id>.log，用户在「拓展 → 插件」可打开查看。
        let log_path = self.logs_dir().join(format!("{id}.log"));
        let (out, err) = match std::fs::File::create(&log_path) {
            Ok(f) => match f.try_clone() {
                Ok(f2) => (std::process::Stdio::from(f), std::process::Stdio::from(f2)),
                Err(_) => (std::process::Stdio::null(), std::process::Stdio::null()),
            },
            Err(e) => {
                tracing::warn!("无法创建插件日志 {log_path:?}: {e}");
                (std::process::Stdio::null(), std::process::Stdio::null())
            }
        };
        cmd.stdout(out).stderr(err);

        // Windows 下避免弹出控制台窗口
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        match cmd.spawn() {
            Ok(child) => {
                tracing::info!(
                    "已启动插件进程 {id} (pid {})，日志: {}",
                    child.id(),
                    log_path.display()
                );
                hub.plugin_procs.write().insert(id.to_string(), child);
                Ok(true)
            }
            Err(e) => Err(anyhow!("启动插件进程失败（{cmd:?}）: {e}")),
        }
    }

    /// 停止插件的自启动进程
    pub fn stop_plugin_process(&self, hub: &SharedHub, id: &str) -> bool {
        let child = hub.plugin_procs.write().remove(id);
        match child {
            Some(mut c) => {
                if !matches!(c.try_wait(), Ok(Some(_))) {
                    let _ = c.kill();
                }
                let _ = c.wait();
                tracing::info!("已停止插件进程 {id}");
                true
            }
            None => false,
        }
    }

    /// 停止全部插件进程（应用退出时调用）
    pub fn stop_all_plugin_processes(&self, hub: &SharedHub) {
        let mut procs = hub.plugin_procs.write();
        for (id, mut c) in procs.drain() {
            // 已经自行退出的进程会有 try_wait=Some，此时不必再 kill
            if let Ok(Some(_)) = c.try_wait() {
                tracing::info!("插件进程 {id} 已自行退出");
                continue;
            }
            let _ = c.kill();
            let _ = c.wait(); // 回收句柄，避免僵尸进程
            tracing::info!("已停止插件进程 {id}");
        }
    }

    /// 启停插件：更新内存状态并回写 plugin.json
    pub fn set_plugin_enabled(&self, hub: &SharedHub, id: &str, enabled: bool) -> Result<bool> {
        let (dir, mut manifest) = {
            let map = hub.plugins.read();
            let Some(m) = map.get(id) else {
                return Err(anyhow!("插件不存在: {id}"));
            };
            (m.dir.clone(), m.clone())
        };
        manifest.enabled = enabled;
        if let Some(d) = &dir {
            let p = d.join("plugin.json");
            let mut raw: Value = serde_json::from_str(&std::fs::read_to_string(&p)?)?;
            raw["enabled"] = json!(enabled);
            std::fs::write(&p, serde_json::to_string_pretty(&raw)?)?;
        }
        if enabled {
            for c in &manifest.commands {
                hub.commands.register_plugin_command(
                    CommandSpec {
                        name: c.name.clone(),
                        description: c.description.clone(),
                        params: c.params.clone(),
                        category: format!("plugin:{id}"),
                    },
                    id,
                );
            }
            if manifest.auto_start.is_some() {
                if let Err(e) = self.start_plugin_process(hub, id) {
                    tracing::warn!("插件 {id} 启动进程失败: {e:#}");
                }
            }
        } else {
            hub.commands.unregister_plugin(id);
            self.stop_plugin_process(hub, id);
        }
        hub.plugins.write().insert(id.to_string(), manifest);
        Ok(true)
    }

    // --------------------------------------------------------- HTTP 状态

    pub fn api_status(&self) -> Value {
        let port = *self.http_running.read();
        json!({
            "running": port.is_some(),
            "port": port,
            "baseUrl": port.map(|p| format!("http://127.0.0.1:{p}/api/v1")),
            "token": self.api_token(),
        })
    }

    pub fn api_token(&self) -> String {
        // 简单稳定的本机 token：写入文件，随数据目录走
        let p = self.data_dir.join("api.token");
        if let Ok(s) = std::fs::read_to_string(&p) {
            if s.trim().len() >= 16 {
                return s.trim().to_string();
            }
        }
        let t = uuid::Uuid::new_v4().simple().to_string();
        std::fs::write(&p, &t).ok();
        t
    }
}

// ==================================================== 统一命令执行器

/// 所有入口（UI / HTTP / 插件 / Agent）最终都汇流到这里。
/// `command` 形如 `notes.create`，`args` 为参数字典。
pub fn execute_command(
    app: &AppHandle,
    svc: &AppService,
    hub: &SharedHub,
    command: &str,
    args: Value,
    source: &str,
) -> Result<Value> {
    let get_str = |k: &str| -> Result<String> {
        args.get(k)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("缺少参数 {k}"))
    };
    let get_bool = |k: &str| args.get(k).and_then(|v| v.as_bool());
    let patch: NotePatch = args
        .get("patch")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()?
        .unwrap_or_default();

    let out = match command {
        "notes.list" => {
            let q: NoteQuery = serde_json::from_value(args.clone()).unwrap_or_default();
            serde_json::to_value(svc.list_notes(&q)?)?
        }
        "notes.search" => {
            let kw = get_str("keyword")?;
            serde_json::to_value(svc.list_notes(&NoteQuery {
                keyword: Some(kw),
                ..Default::default()
            })?)?
        }
        "notes.get" => serde_json::to_value(svc.get_note(&get_str("id")?)?)?,
        "notes.create" => {
            let mut p: NotePatch = args
                .get("patch")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()?
                .unwrap_or_default();
            if p.content.is_none() {
                p.content = args.get("content").and_then(|v| v.as_str()).map(String::from);
            }
            if p.color.is_none() {
                p.color = args.get("color").and_then(|v| v.as_str()).map(String::from);
            }
            if p.meta.is_none() {
                p.meta = args.get("meta").cloned();
            }
            let open = get_bool("open").unwrap_or(true);
            let note = svc.create_note(p, open)?;
            if open {
                window::open_note_window(app, &note)?;
            }
            hub.broadcast_note("note.created", &note, source);
            serde_json::to_value(note)?
        }
        "notes.update" => {
            let id = get_str("id")?;
            let n = svc.update_note(&id, &patch)?;
            if let Some(note) = &n {
                hub.broadcast_note("note.updated", note, source);
            }
            serde_json::to_value(n)?
        }
        "notes.replace" => {
            let id = get_str("id")?;
            let content = get_str("content")?;
            let n = svc.update_note(&id, &NotePatch { content: Some(content), ..Default::default() })?;
            if let Some(note) = &n {
                hub.broadcast_note("note.updated", note, source);
            }
            serde_json::to_value(n)?
        }
        "notes.append" => {
            let id = get_str("id")?;
            let text = get_str("text")?;
            let n = svc.append_to_note(&id, &text)?;
            if let Some(note) = &n {
                hub.broadcast_note("note.updated", note, source);
            }
            serde_json::to_value(n)?
        }
        "notes.trash" => {
            let id = get_str("id")?;
            let note = svc.get_note(&id)?;
            let ok = svc.trash_note(&id)?;
            window::close_note_window(app, &id);
            if let Some(n) = note {
                hub.broadcast_note("note.trashed", &n, source);
            }
            json!({ "ok": ok })
        }
        "notes.restore" => {
            let id = get_str("id")?;
            let ok = svc.restore_note(&id)?;
            hub.emit(AppEvent::new("note.restored", json!({ "id": id }), source));
            json!({ "ok": ok })
        }
        "notes.purge" => {
            let id = get_str("id")?;
            let ok = svc.purge_note(&id)?;
            window::close_note_window(app, &id);
            hub.emit(AppEvent::new("note.purged", json!({ "id": id }), source));
            json!({ "ok": ok })
        }
        "notes.pin" => {
            let id = get_str("id")?;
            let pinned = get_bool("pinned").unwrap_or(true);
            let ok = svc.set_pinned(&id, pinned)?;
            if let Some(w) = app.get_webview_window(&window::note_label(&id)) {
                let _ = w.set_always_on_top(pinned);
            }
            hub.emit(AppEvent::new("note.pinnedChanged", json!({ "id": id, "pinned": pinned }), source));
            json!({ "ok": ok })
        }
        "notes.setColor" => {
            let id = get_str("id")?;
            let color = get_str("color")?;
            let ok = svc.set_color(&id, &color)?;
            hub.emit(AppEvent::new("note.colorChanged", json!({ "id": id, "color": color }), source));
            json!({ "ok": ok })
        }
        "notes.duplicate" => {
            let id = get_str("id")?;
            let n = svc.duplicate_note(&id)?;
            if let Some(note) = &n {
                let _ = window::open_note_window(app, note);
                hub.broadcast_note("note.created", note, source);
            }
            serde_json::to_value(n)?
        }
        "window.openNote" => {
            let id = get_str("id")?;
            let note = svc.get_note(&id)?.ok_or_else(|| anyhow!("便签不存在"))?;
            svc.set_open(&id, true)?;
            window::open_note_window(app, &note)?;
            json!({ "ok": true })
        }
        "window.closeNote" => {
            let id = get_str("id")?;
            svc.set_open(&id, false)?;
            window::close_note_window(app, &id);
            json!({ "ok": true })
        }
        "window.openMain" => {
            window::show_main(app);
            json!({ "ok": true })
        }
        "app.settings" => serde_json::to_value(svc.settings()?)?,
        "app.updateSettings" => {
            let mut s = svc.settings()?;
            if let Some(p) = args.get("patch") {
                let cur = serde_json::to_value(&s)?;
                let merged = merge_json(cur, p.clone());
                s = serde_json::from_value(merged)?;
            }
            let saved = svc.save_settings(&s)?;
            hub.emit(AppEvent::new("settings.changed", serde_json::to_value(&saved)?, source));
            serde_json::to_value(saved)?
        }
        "app.stats" => {
            let (total, active, trashed) = svc.counts()?;
            json!({ "total": total, "active": active, "trashed": trashed })
        }
        "app.export" => {
            let path = args.get("path").and_then(|v| v.as_str()).map(String::from);
            match path {
                Some(p) => {
                    let n = svc.export_to_path(&p)?;
                    json!({ "ok": true, "affected": n, "path": p })
                }
                None => {
                    let p = svc.export_to_dialog(app)?;
                    json!({ "ok": true, "path": p })
                }
            }
        }
        "app.import" => {
            let path = args.get("path").and_then(|v| v.as_str()).map(String::from);
            match path {
                Some(p) => {
                    let n = svc.import_from_path(&p, get_bool("overwrite").unwrap_or(false))?;
                    json!({ "ok": true, "affected": n })
                }
                None => serde_json::to_value(svc.import_from_dialog(app)?)?,
            }
        }
        "trash.empty" => {
            let n = svc.purge_trash()?;
            hub.emit(AppEvent::new("trash.emptied", json!({ "count": n }), source));
            json!({ "ok": true, "affected": n })
        }
        "theme.list" => serde_json::to_value(svc.themes())?,
        "theme.reload" => {
            svc.reload_themes();
            serde_json::to_value(svc.themes())?
        }
        "theme.set" => {
            let id = get_str("id")?;
            let mut s = svc.settings()?;
            s.theme_pack = id.clone();
            svc.save_settings(&s)?;
            hub.emit(AppEvent::new("theme.changed", json!({ "id": id }), source));
            json!({ "ok": true })
        }
        "plugin.list" => serde_json::to_value(svc.plugin_runtimes(hub))?,
        "plugin.reload" => {
            svc.reload_plugins(hub)?;
            serde_json::to_value(svc.plugin_runtimes(hub))?
        }
        "plugin.enable" => {
            let id = get_str("id")?;
            let en = get_bool("enabled").unwrap_or(true);
            let ok = svc.set_plugin_enabled(hub, &id, en)?;
            json!({ "ok": ok })
        }
        "plugin.startProcess" => {
            let id = get_str("id")?;
            let ok = svc.start_plugin_process(hub, &id)?;
            json!({ "ok": ok })
        }
        "plugin.stopProcess" => {
            let id = get_str("id")?;
            let ok = svc.stop_plugin_process(hub, &id);
            json!({ "ok": ok })
        }
        "events.recent" => {
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
            serde_json::to_value(hub.events.recent(limit))?
        }
        "events.emit" => {
            let name = get_str("name")?;
            let payload = args.get("payload").cloned().unwrap_or(json!({}));
            hub.emit(AppEvent::new(&name, payload, source));
            json!({ "ok": true })
        }
        "webhook.list" => serde_json::to_value(hub.events.webhooks())?,
        "webhook.upsert" => {
            let cfg = args
                .get("config")
                .cloned()
                .ok_or_else(|| anyhow!("缺少 config"))?;
            let cfg: hub::WebhookConfig = serde_json::from_value(cfg)?;
            hub.events.upsert_webhook(cfg);
            json!({ "ok": true })
        }
        "webhook.remove" => {
            let id = get_str("id")?;
            json!({ "ok": hub.events.remove_webhook(&id) })
        }
        "ext.commands" => serde_json::to_value(hub.commands.all())?,
        "ext.paths" => svc.paths_info(),
        "api.status" => svc.api_status(),
        "api.enable" => {
            let en = get_bool("enabled").unwrap_or(true);
            let port = args.get("port").and_then(|v| v.as_u64()).map(|v| v as u16);
            // 命令总线里没有 Arc，改为直接操作共享状态字段
            if !en {
                *svc.http_running.write() = None;
                tracing::info!("本地 HTTP API 已关闭");
            } else {
                let p = port
                    .or_else(|| svc.settings().ok().map(|s| s.http_api_port))
                    .unwrap_or(43919);
                *svc.http_running.write() = Some(p);
                crate::api::spawn_server_shared(app, svc, hub, p);
            }
            svc.api_status()
        }
        // ------------------------------------------------------- 笔记（Vault）
        "doc.list" => {
            let folder = args.get("folder").and_then(|v| v.as_str()).map(String::from);
            let tag = args.get("tag").and_then(|v| v.as_str()).map(String::from);
            serde_json::to_value(svc.vault_list(folder, tag)?)?
        }
        "doc.tree" => serde_json::to_value(svc.vault_tree()?)?,
        "doc.info" => serde_json::to_value(svc.vault_info()?)?,
        "doc.tags" => serde_json::to_value(svc.vault_tags()?)?,
        "doc.get" => serde_json::to_value(svc.vault_read(&get_str("id")?)?)?,
        "doc.search" => {
            let kw = get_str("keyword")?;
            serde_json::to_value(svc.vault_search(&kw)?)?
        }
        "doc.create" => {
            let path = get_str("path")?;
            let content = args.get("content").and_then(|v| v.as_str()).map(String::from);
            let frontmatter = args.get("frontmatter").cloned();
            let d = svc.vault_create(DocPatch { path: Some(path), content, frontmatter, tags: None })?;
            hub.emit(AppEvent::new("doc.created", json!({ "id": d.id, "path": d.path, "title": d.title }), source));
            serde_json::to_value(d)?
        }
        "doc.save" => {
            let id = get_str("id")?;
            let content = args.get("content").and_then(|v| v.as_str()).map(String::from);
            let frontmatter = args.get("frontmatter").cloned();
            let d = svc.vault_save(&id, DocPatch { path: None, content, frontmatter, tags: None })?;
            if let Some(doc) = &d {
                hub.emit(
                    AppEvent::new("doc.saved", json!({ "id": doc.id, "path": doc.path, "title": doc.title, "wordCount": doc.word_count }), source),
                );
            }
            serde_json::to_value(d)?
        }
        "doc.rename" => {
            let id = get_str("id")?;
            let path = get_str("path")?;
            let d = svc.vault_rename(&id, &path)?;
            hub.emit(AppEvent::new("doc.renamed", json!({ "id": id, "path": path }), source));
            serde_json::to_value(d)?
        }
        "doc.delete" => {
            let id = get_str("id")?;
            let ok = svc.vault_delete(&id)?;
            hub.emit(AppEvent::new("doc.deleted", json!({ "id": id }), source));
            json!({ "ok": ok })
        }
        "doc.backlinks" => serde_json::to_value(svc.vault_backlinks(&get_str("id")?)?)?,
        "doc.outlinks" => serde_json::to_value(svc.vault_outlinks(&get_str("id")?)?)?,
        "doc.reindex" => {
            let n = svc.vault_reindex()?;
            hub.emit(AppEvent::new("doc.reindexed", json!({ "count": n }), source));
            json!({ "ok": true, "affected": n })
        }
        // 便签 -> 笔记：把若干便签汇总成一篇 Markdown 笔记（插件与内置命令共用）
        "doc.fromNotes" => {
            let ids: Vec<String> = args
                .get("ids")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default();
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| format!("便签汇总 {}", chrono::Local::now().format("%Y-%m-%d %H:%M")));
            let folder = args.get("folder").and_then(|v| v.as_str()).unwrap_or("便签汇总");
            let only_ids: std::collections::HashSet<String> = ids.iter().cloned().collect();
            let notes = svc.list_notes(&NoteQuery { include_deleted: Some(false), ..Default::default() })?;
            let picked: Vec<&Note> = notes
                .iter()
                .filter(|n| only_ids.is_empty() || only_ids.contains(&n.id))
                .collect();
            if picked.is_empty() {
                return Err(anyhow!("没有可汇总的便签"));
            }
            let mut md = String::new();
            md.push_str(&format!("# {title}\n\n"));
            md.push_str(&format!("> 由 {} 张便签汇总生成 · {}\n\n", picked.len(), chrono::Local::now().format("%Y-%m-%d %H:%M")));
            md.push_str("## 目录\n\n");
            for (i, n) in picked.iter().enumerate() {
                let t = if n.title().is_empty() { format!("便签 {}", i + 1) } else { n.title() };
                md.push_str(&format!("{}. {}\n", i + 1, t));
            }
            md.push_str("\n---\n\n");
            for (i, n) in picked.iter().enumerate() {
                let t = if n.title().is_empty() { format!("便签 {}", i + 1) } else { n.title() };
                md.push_str(&format!("## {}\n\n", t));
                md.push_str(&format!("- 颜色：`{}`\n", n.color));
                md.push_str(&format!("- 更新：{}\n\n", n.updated_at));
                let body = n.content.trim();
                md.push_str(if body.is_empty() { "_（空便签）_\n\n" } else { body });
                md.push_str("\n\n");
                let _ = i;
            }
            let rel = format!("{folder}/{}.md", title.replace(['/', '\\', ':'], "-"));
            let d = match svc.vault_create(DocPatch { path: Some(rel), content: Some(md.clone()), frontmatter: None, tags: None }) {
                Ok(d) => d,
                // 同名则追加时间戳，避免覆盖
                Err(_) => {
                    let rel2 = format!("{folder}/{}-{}.md", title.replace(['/', '\\', ':'], "-"), chrono::Local::now().format("%H%M%S"));
                    svc.vault_create(DocPatch { path: Some(rel2), content: Some(md), frontmatter: None, tags: None })?
                }
            };
            hub.emit(AppEvent::new("doc.created", json!({ "id": d.id, "path": d.path, "title": d.title, "fromNotes": picked.len() }), source));
            serde_json::to_value(d)?
        }
        other => {
            // 插件命令：转发到插件后端，并把插件的返回结果透传给调用方
            if let Some(owner) = hub.commands.owner_of(other) {
                let result = forward_to_plugin_result(hub, &owner, other, args)?;
                json!({ "ok": true, "forwardedTo": owner, "result": result })
            } else {
                return Err(anyhow!("未知命令: {other}"));
            }
        }
    };
    Ok(out)
}

/// 探测插件后端是否已在运行（用于避免重复拉起导致端口冲突）。
///
/// endpoint 形如 `http://127.0.0.1:43110/sticky`。这里不做 HTTP 请求，
/// 而是直接对 `host:port` 做一次 **纯 std TCP 连接**探测：
///
/// - 不依赖 Tokio 运行时，因此可以在任何线程、任何生命周期阶段安全调用
///   （在 Tauri setup 早期调用异步运行时可能尚未就绪）；
/// - 只要端口上有监听者，就认为「已有实例在运行」，避免新进程端口冲突。
fn plugin_endpoint_alive(endpoint: &str) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};

    // 从 URL 里取出 host:port
    let rest = endpoint
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(endpoint);
    let hostport = rest.split('/').next().unwrap_or(rest);
    let addr = if hostport.contains(':') {
        hostport.to_string()
    } else {
        format!("{hostport}:80")
    };

    let Ok(mut addrs) = addr.to_socket_addrs() else {
        return false;
    };
    // 逐个尝试解析出的地址（IPv4 / IPv6），任一连通即视为存活
    addrs.any(|a| {
        TcpStream::connect_timeout(&a, std::time::Duration::from_millis(600)).is_ok()
    })
}

/// 转发命令到插件后端，并等待它的响应（用于需要结果的场景，例如「总结生成笔记」）。
///
/// 采用同步等待 + 超时：插件是本地 HTTP 服务，正常在毫秒级返回；
/// 若插件没起，1.5 秒内失败并返回错误，不会把 UI 卡死。
fn forward_to_plugin_result(
    hub: &SharedHub,
    plugin_id: &str,
    command: &str,
    args: Value,
) -> Result<Value> {
    let endpoint = {
        let map = hub.plugins.read();
        map.get(plugin_id)
            .and_then(|p| p.endpoint.clone())
            .ok_or_else(|| anyhow!("插件 {plugin_id} 未提供 endpoint（纯前端插件不能处理命令）"))?
    };
    let body = json!({ "command": command, "args": args });
    let pid = plugin_id.to_string();
    let client = hub::shared_http_client();

    let call = async move {
        let resp = client
            .post(&endpoint)
            .header("x-plugin-id", &pid)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("插件返回 {}: {}", status.as_u16(), text));
        }
        Ok::<Value, anyhow::Error>(parse_plugin_body(&text))
    };

    let timeout = std::time::Duration::from_millis(8000);
    // 统一投递到全局后台运行时，再同步等待结果。
    // 这样既不会嵌套 block_on（在 Tauri 异步命令里会 panic），
    // 也不依赖调用线程是否处于 Tokio 上下文。
    let (tx, rx) = std::sync::mpsc::channel();
    hub::spawn_async(async move {
        let out = tokio::time::timeout(timeout, call)
            .await
            .map_err(|_| anyhow!("插件响应超时（{timeout:?}）"))
            .and_then(|r| r);
        let _ = tx.send(out);
    });
    rx.recv_timeout(timeout + std::time::Duration::from_secs(2))
        .map_err(|_| anyhow!("等待插件响应超时"))?
}

/// 插件可能返回 JSON，也可能返回纯文本；统一成 Value
fn parse_plugin_body(text: &str) -> Value {
    let t = text.trim();
    if t.is_empty() {
        return json!({ "ok": true });
    }
    serde_json::from_str(t).unwrap_or_else(|_| json!({ "text": t }))
}

/// 深度合并 JSON 对象（用于 settings patch）
pub fn merge_json(base: Value, patch: Value) -> Value {
    match (base, patch) {
        (Value::Object(mut a), Value::Object(b)) => {
            for (k, v) in b {
                let merged = match a.remove(&k) {
                    Some(old) => merge_json(old, v),
                    None => v,
                };
                a.insert(k, merged);
            }
            Value::Object(a)
        }
        (_, p) => p,
    }
}

impl AppService {
    /// 便捷：初始化磁盘目录结构，并生成示例插件与主题，降低上手门槛
    pub fn scaffold_demo_assets(&self) -> Result<()> {
        self.ensure_layout()?;
        self.scaffold_paper_theme()?;
        self.scaffold_summarizer_plugin()?;
        self.scaffold_vault_welcome()?;
        Ok(())
    }

    /// 示例主题：纸张
    fn scaffold_paper_theme(&self) -> Result<()> {
        let tdir = self.themes_dir().join("paper");
        if tdir.exists() {
            return Ok(());
        }
        std::fs::create_dir_all(&tdir)?;
        let mut colors = HashMap::new();
        colors.insert("yellow".to_string(), "#FBF3D5".to_string());
        colors.insert("green".to_string(), "#E4EFDC".to_string());
        colors.insert("pink".to_string(), "#F7E3E8".to_string());
        colors.insert("purple".to_string(), "#EAE3F5".to_string());
        colors.insert("blue".to_string(), "#DFEAF3".to_string());
        colors.insert("gray".to_string(), "#EDEDED".to_string());
        let mut css = HashMap::new();
        css.insert("--sn-radius".to_string(), "14px".to_string());
        css.insert(
            "--sn-note-shadow".to_string(),
            "0 1px 2px rgba(0,0,0,.08), 0 8px 24px rgba(0,0,0,.10)".to_string(),
        );
        let t = ThemePack {
            id: "paper".into(),
            name: "纸张".into(),
            author: "示例".into(),
            description: "低饱和纸质色，圆角更大，长阴影".into(),
            version: "1.0.0".into(),
            appearance: "light".into(),
            colors,
            css_vars: css,
            css_file: Some("theme.css".into()),
            renderer: None,
            builtin: false,
            dir: Some(tdir.clone()),
        };
        std::fs::write(tdir.join("theme.json"), serde_json::to_string_pretty(&t)?)?;
        std::fs::write(
            tdir.join("theme.css"),
            "/* 主题包附加样式：覆盖任意 CSS 变量 */\n.note-shell { backdrop-filter: saturate(1.05); }\n",
        )?;
        Ok(())
    }

    /// 内置第一个插件：便签总结生成笔记
    ///
    /// v2 起后端是 **Rust 单文件 exe**，随主程序一起分发、首次启动释放，
    /// 因此不需要目标机器安装 Python，也无需任何额外配置。
    /// 若用户机器上没有 python，命令仍可用（应用内置了 `doc.fromNotes` 兜底实现）。
    fn scaffold_summarizer_plugin(&self) -> Result<()> {
        let pdir = self.plugins_dir().join("sticky-summarizer");

        // 清单与说明：内容变了要覆盖（版本升级时同步新契约）
        std::fs::create_dir_all(&pdir)?;
        std::fs::write(pdir.join("plugin.json"), SUMMARIZER_PLUGIN_JSON)?;
        std::fs::write(pdir.join("README.md"), SUMMARIZER_PLUGIN_README)?;

        // 后端可执行文件：释放到 bin/ 下。
        // 每次启动都校验（大小不一致就重写），保证升级主程序后插件同步更新。
        let bindir = pdir.join("bin");
        std::fs::create_dir_all(&bindir)?;
        let exe = bindir.join(PLUGIN_BIN_NAME);
        let need_write = match std::fs::metadata(&exe) {
            Ok(m) => m.len() != SUMMARIZER_PLUGIN_BIN.len() as u64,
            Err(_) => true,
        };
        if need_write {
            std::fs::write(&exe, SUMMARIZER_PLUGIN_BIN)?;
            tracing::info!("已释放插件后端 {} ({} 字节)", exe.display(), SUMMARIZER_PLUGIN_BIN.len());
        }

        // 清理历史遗留的 Python 版文件（v1 升级上来时）
        let legacy_py = pdir.join("server.py");
        if legacy_py.exists() {
            let _ = std::fs::remove_file(&legacy_py);
            tracing::info!("已移除旧版 Python 插件后端");
        }
        let legacy_cache = pdir.join("__pycache__");
        if legacy_cache.exists() {
            let _ = std::fs::remove_dir_all(&legacy_cache);
        }

        tracing::info!("已安装插件 sticky-summarizer (Rust)");
        Ok(())
    }

    /// 首次启动时给笔记库放一篇欢迎笔记，顺便演示 Obsidian 语法
    fn scaffold_vault_welcome(&self) -> Result<()> {
        let about = self.vault_dir().join("欢迎使用笔记.md");
        if about.exists() {
            return Ok(());
        }
        let content = r#"---
tags: [使用说明, 示例]
created: 自动生成
---

# 欢迎使用笔记

这个笔记库就是一个**普通的 Markdown 文件夹**，你可以用 Obsidian、VS Code、Typora 直接打开它。

## 核心特性

- [x] 实时预览（编辑 / 分栏 / 阅读三种视图，Ctrl+E 切换）
- [x] `[[双向链接]]` 与反向链接面板
- [x] `#标签` 自动索引
- [ ] YAML frontmatter（就是本文顶部那块）
- [ ] 大纲导航

## 双向链接怎么用

用两个方括号包裹笔记名即可建立链接，例如 [[便签使用技巧]]。
如果目标笔记不存在，链接会显示为**虚线**，点击即可创建。

输入 `[[` 会自动弹出笔记补全列表。

## 标签

在正文任意位置写 `#标签名` 就会被索引，左侧栏会列出所有标签。
支持中文标签，例如 #效率 #记录。

## 标注块

> [!tip] 提示
> 用 `> [!tip]` 可以写出这样的标注块，支持 tip / warning / danger / note。

## 代码

```python
def hello():
    print("笔记里可以直接贴代码")
```

## 表格

| 视图 | 快捷键 | 说明 |
| --- | --- | --- |
| 编辑 | Ctrl+E | 只看源码 |
| 分栏 | Ctrl+E | 左边源码右边预览 |
| 阅读 | Ctrl+E | 只看渲染结果 |

---

*把这篇笔记删掉也没关系，它只是用来演示的。*
"#;
        std::fs::write(&about, content)?;

        let tips = self.vault_dir().join("便签使用技巧.md");
        if !tips.exists() {
            let t = r#"---
tags: [技巧]
---

# 便签使用技巧

## 快捷键

| 操作 | 快捷键 |
| --- | --- |
| 新建便签 | `Ctrl+N` |
| 搜索 | `Ctrl+F` |
| 全选 | `Ctrl+A` |
| 删除选中 | `Delete` |
| 保存并关闭便签窗口 | `Esc` |

## 便签转笔记

选中若干便签后，可以用「便签总结」插件一键生成结构化笔记。
详见 [[欢迎使用笔记]]。

## 颜色分类

便签支持六种颜色（黄绿粉紫蓝灰），可以用来做简单的分类：
- 黄色 = 待办
- 蓝色 = 灵感
- 绿色 = 已完成

写完后可以用 `#todo` 标记待办项，插件会自动汇总。
"#;
            std::fs::write(&tips, t)?;
        }
        // 索引刚刚写入的文件
        self.vault.reindex().ok();
        Ok(())
    }
}

// ============================================ 内置插件资源（编译期嵌入）
//
// 插件在项目 `plugins/sticky-summarizer/` 下维护，编译时嵌入二进制，
// 首次启动自动释放到数据目录。这样安装包不需要额外携带资源文件。
//
// v2 起插件后端改为 **Rust 实现**（单个 exe，零运行时依赖），
// 不再需要目标机器安装 Python。插件可执行文件同样被嵌进主程序，
// 首次启动释放到 `<数据目录>/plugins/sticky-summarizer/bin/`。

/// 插件后端可执行文件名（Windows 带 .exe）
#[cfg(windows)]
const PLUGIN_BIN_NAME: &str = "sticky-summarizer.exe";
#[cfg(not(windows))]
const PLUGIN_BIN_NAME: &str = "sticky-summarizer";

/// 判断一个命令字符串是否包含路径分隔符（说明它是文件路径而非 PATH 里的命令名）
fn has_sep_hint(s: &str) -> bool {
    s.contains('/') || s.contains('\\')
}

/// 内置插件：便签总结生成笔记 —— 清单
const SUMMARIZER_PLUGIN_JSON: &str = include_str!("../../plugins/sticky-summarizer/plugin.json");

/// 内置插件：便签总结生成笔记 —— 说明文档
const SUMMARIZER_PLUGIN_README: &str = include_str!("../../plugins/sticky-summarizer/README.md");

/// 内置插件：便签总结生成笔记 —— Rust 服务（编译产物）
///
/// 这个路径由 `plugins/sticky-summarizer` 独立 crate 构建产出。
/// 构建顺序：先在插件目录 `cargo build --release`，再构建主程序。
/// 若尚未构建插件，`build.rs` 会给出明确提示（见 src-tauri/build.rs）。
const SUMMARIZER_PLUGIN_BIN: &[u8] =
    include_bytes!("../../plugins/sticky-summarizer/target/release/sticky-summarizer.exe");

//! 数据模型定义
//!
//! 所有与前端 / HTTP API / 插件通信的数据结构都在这里统一定义，
//! 保证「一次定义，多处复用」。

use serde::{Deserialize, Serialize};

/// 便签主题色 —— 与微软便笺保持一致的基础色板
pub const DEFAULT_COLORS: [&str; 6] = [
    "yellow", "green", "pink", "purple", "blue", "gray",
];

/// 单张便签
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    /// 纯文本内容（用于搜索 / 预览）
    pub content: String,
    /// 主题色 id（yellow / green / ... 或主题包自定义 id）
    pub color: String,
    /// 便签窗口位置与尺寸（独立窗口模式使用）
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// 窗口是否已打开
    pub is_open: bool,
    /// 是否置顶
    pub is_pinned: bool,
    /// 堆叠顺序
    pub z_index: i64,
    /// 是否删除（回收站）
    pub deleted: bool,
    /// 是否在列表中显示（微软便笺的「从列表中隐藏」）
    pub hidden: bool,
    /// 字号缩放
    pub font_scale: f64,
    /// 创建的 ISO 时间
    pub created_at: String,
    /// 最后修改的 ISO 时间
    pub updated_at: String,
    /// 删除时间（回收站自动清理用）
    pub deleted_at: Option<String>,
    /// 拓展字段：插件 / Agent 可自由写入结构化数据
    #[serde(default)]
    pub meta: serde_json::Value,
}

impl Note {
    pub fn title(&self) -> String {
        let first_line = self
            .content
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim();
        if first_line.is_empty() {
            String::new()
        } else {
            first_line.chars().take(60).collect()
        }
    }
}

/// 新建 / 更新便签的入参（部分字段可选）
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotePatch {
    pub content: Option<String>,
    pub color: Option<String>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub is_open: Option<bool>,
    pub is_pinned: Option<bool>,
    pub z_index: Option<i64>,
    pub font_scale: Option<f64>,
    pub hidden: Option<bool>,
    pub meta: Option<serde_json::Value>,
}

/// 便签列表查询条件
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteQuery {
    /// 关键字（在内容中搜索）
    pub keyword: Option<String>,
    /// 是否包含回收站内容
    pub include_deleted: Option<bool>,
    /// 仅回收站
    pub only_deleted: Option<bool>,
    /// 按颜色筛选
    pub colors: Option<Vec<String>>,
    /// 排序：updated | created | color | alpha
    pub sort: Option<String>,
}

/// 应用设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// 界面主题：system | light | dark
    pub theme: String,
    /// 当前主题包 id
    pub theme_pack: String,
    /// 启动时自动打开上次的便签
    pub restore_on_start: bool,
    /// 关闭主窗口的最小化到托盘
    pub minimize_to_tray: bool,
    /// 新建便签默认色
    pub default_color: String,
    /// 新建便签默认尺寸
    pub default_note_width: i32,
    pub default_note_height: i32,
    /// 界面语言
    pub language: String,
    /// 便签默认字号缩放
    pub default_font_scale: f64,
    /// 是否启用本地 HTTP 服务（多 App 联动）
    pub http_api_enabled: bool,
    /// 本地 HTTP 服务端口
    pub http_api_port: u16,
    /// 是否允许插件运行
    pub plugins_enabled: bool,
    /// 回收站保留天数
    pub trash_retention_days: i64,
    /// 主窗口视图：grid | list
    pub view_mode: String,
    /// 全量拓展配置
    #[serde(default)]
    pub extra: serde_json::Value,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            theme_pack: "default".into(),
            restore_on_start: true,
            minimize_to_tray: true,
            default_color: "yellow".into(),
            default_note_width: 320,
            default_note_height: 360,
            language: "zh-CN".into(),
            default_font_scale: 1.0,
            // 默认开启：插件、Agent、多 App 联动都依赖本地命令总线。
            // 只监听 127.0.0.1，不对局域网暴露，因此默认开启是安全的。
            http_api_enabled: true,
            http_api_port: 43919,
            plugins_enabled: true,
            trash_retention_days: 30,
            view_mode: "grid".into(),
            extra: serde_json::json!({}),
        }
    }
}

/// 导出文件结构（JSON 备份）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub version: u32,
    pub app: String,
    pub exported_at: String,
    pub notes: Vec<Note>,
    pub settings: Option<Settings>,
}

/// 一次批量操作的结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpResult {
    pub ok: bool,
    pub affected: usize,
    pub message: String,
}

// ============================================================ 笔记（文档）
//
// 对标 Obsidian 的核心概念：
//   - 每个笔记是一份 Markdown 文档，存在 vault 目录里（磁盘上就是 .md 文件）
//   - `[[双向链接]]` 建立笔记之间的关系，可查询反向链接
//   - `#标签` 形成标签体系
//   - YAML frontmatter 存放元数据
// 这里额外把索引写进 SQLite，用于快速全文检索与关系查询，
// 磁盘 md 文件始终是唯一事实来源（便于用其他编辑器打开、便于云同步）。

/// 一篇笔记文档
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Doc {
    pub id: String,
    /// 相对 vault 根目录的路径，如 `日记/2026-10-11.md`
    pub path: String,
    /// 文件名（不含扩展名），即 Obsidian 里的「笔记名」
    pub title: String,
    /// 所在文件夹（相对 vault，根目录为空串）
    pub folder: String,
    /// 正文（不含 frontmatter）
    pub content: String,
    /// frontmatter 解析出的键值对
    #[serde(default)]
    pub frontmatter: serde_json::Value,
    /// 正文中出现的 [[链接]] 目标
    #[serde(default)]
    pub links: Vec<String>,
    /// 正文中出现的 #标签
    #[serde(default)]
    pub tags: Vec<String>,
    /// 字数（按字符计）
    pub word_count: usize,
    pub created_at: String,
    pub updated_at: String,
}

/// 新建 / 更新文档的入参
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocPatch {
    pub path: Option<String>,
    pub content: Option<String>,
    pub frontmatter: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// 文档树节点（文件夹 / 文件）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocNode {
    pub name: String,
    /// 相对 vault 的路径
    pub path: String,
    /// true = 文件夹
    pub is_dir: bool,
    #[serde(default)]
    pub children: Vec<DocNode>,
}

/// 反向链接：哪些笔记引用了目标笔记
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backlink {
    pub id: String,
    pub title: String,
    pub path: String,
    /// 引用所在行（上下文片段）
    pub context: String,
}

/// 笔记库概览
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    /// vault 根目录绝对路径
    pub root: String,
    pub doc_count: usize,
    pub folder_count: usize,
    /// 标签 -> 出现次数
    pub tags: Vec<(String, usize)>,
    /// 孤立笔记（没有任何链接进出）
    pub orphans: usize,
}

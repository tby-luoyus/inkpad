//! 笔记库（Vault）—— 对标 Obsidian
//!
//! 设计要点：
//!  1. **磁盘 md 文件是唯一事实来源**。vault 就是一个普通文件夹，
//!     你可以用 VS Code / Typora / Obsidian 直接打开编辑，本应用只做索引与展示。
//!  2. **SQLite 只做索引**。全文检索、反向链接、标签统计走索引，
//!     索引损坏时可以随时全量重建（`reindex`）。
//!  3. **[[双向链接]] 与 #标签** 在保存时解析并落库，支持反向链接查询。

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::models::{Backlink, Doc, DocNode, DocPatch, VaultInfo};

const DOC_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS docs (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL UNIQUE,
    title       TEXT NOT NULL,
    folder      TEXT NOT NULL DEFAULT '',
    content     TEXT NOT NULL DEFAULT '',
    frontmatter TEXT NOT NULL DEFAULT '{}',
    links       TEXT NOT NULL DEFAULT '[]',
    tags        TEXT NOT NULL DEFAULT '[]',
    word_count  INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    mtime       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_docs_path   ON docs(path);
CREATE INDEX IF NOT EXISTS idx_docs_folder ON docs(folder);
CREATE INDEX IF NOT EXISTS idx_docs_updated ON docs(updated_at DESC);
"#;

pub struct Vault {
    conn: Mutex<Connection>,
    pub root: PathBuf,
}

impl Vault {
    /// 打开（或创建）一个笔记库
    pub fn open(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root).context("创建笔记库目录失败")?;
        let db = root.join(".sticky-vault.db");
        let conn = Connection::open(&db).context("打开笔记索引失败")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(DOC_SCHEMA)?;
        let v = Self {
            conn: Mutex::new(conn),
            root,
        };
        v.reindex()?;
        Ok(v)
    }

    // ------------------------------------------------------------ 路径处理

    /// 把用户给的路径规整成相对 vault 的安全路径（禁止越界）
    fn safe_rel(&self, path: &str) -> Result<String> {
        let cleaned = path.replace('\\', "/");
        let trimmed = cleaned.trim_start_matches("./").trim_start_matches('/');
        if trimmed.split('/').any(|seg| seg == "..") {
            return Err(anyhow!("路径不允许越出笔记库: {path}"));
        }
        let mut out = trimmed.to_string();
        if !out.ends_with(".md") {
            out.push_str(".md");
        }
        if out.trim_matches('.').is_empty() || out == ".md" {
            return Err(anyhow!("笔记名不能为空"));
        }
        Ok(out)
    }

    pub fn abs_path(&self, rel: &str) -> PathBuf {
        self.root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
    }

    fn title_of(rel: &str) -> String {
        rel.rsplit('/')
            .next()
            .unwrap_or(rel)
            .trim_end_matches(".md")
            .to_string()
    }

    fn folder_of(rel: &str) -> String {
        match rel.rfind('/') {
            Some(i) => rel[..i].to_string(),
            None => String::new(),
        }
    }

    // -------------------------------------------------------- frontmatter

    /// 解析 YAML frontmatter（`---\nkey: value\n---`）。
    /// 这里做轻量解析，不引入 YAML 依赖：支持 `key: value`、`key: [a, b]`、`key:` + `- item` 列表。
    fn split_frontmatter(raw: &str) -> (Value, String) {
        let text = raw.strip_prefix('\u{feff}').unwrap_or(raw);
        if !text.starts_with("---") {
            return (json!({}), text.to_string());
        }
        let after = &text[3..];
        let Some(end) = after.find("\n---") else {
            return (json!({}), text.to_string());
        };
        let head = &after[..end];
        let body = after[end + 4..].trim_start_matches('\n').to_string();

        let mut map = serde_json::Map::new();
        let mut pending_list: Option<String> = None;
        for line in head.lines() {
            let line = line.trim_end();
            if line.trim().is_empty() {
                continue;
            }
            if let Some(item) = line.trim_start().strip_prefix("- ") {
                if let Some(key) = &pending_list {
                    let arr = map
                        .entry(key.clone())
                        .or_insert_with(|| Value::Array(vec![]));
                    if let Value::Array(a) = arr {
                        a.push(Value::String(item.trim().to_string()));
                    }
                }
                continue;
            }
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim().to_string();
                let val = v.trim();
                if val.is_empty() {
                    pending_list = Some(key.clone());
                    map.insert(key, Value::Array(vec![]));
                    continue;
                }
                pending_list = None;
                let parsed = if val.starts_with('[') && val.ends_with(']') {
                    let inner = &val[1..val.len() - 1];
                    Value::Array(
                        inner
                            .split(',')
                            .map(|s| Value::String(s.trim().trim_matches(['"', '\'']).to_string()))
                            .filter(|v| v.as_str().map(|s| !s.is_empty()).unwrap_or(true))
                            .collect(),
                    )
                } else if val == "true" || val == "false" {
                    Value::Bool(val == "true")
                } else if let Ok(n) = val.parse::<i64>() {
                    Value::Number(n.into())
                } else {
                    Value::String(val.trim_matches(['"', '\'']).to_string())
                };
                map.insert(key, parsed);
            }
        }
        (Value::Object(map), body)
    }

    /// 组装 frontmatter + 正文
    fn compose(fm: &Value, content: &str) -> String {
        let mut out = String::new();
        if let Value::Object(map) = fm {
            if !map.is_empty() {
                out.push_str("---\n");
                for (k, v) in map {
                    let line = match v {
                        Value::Array(a) => {
                            let items: Vec<String> =
                                a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect();
                            format!("{k}: [{}]\n", items.join(", "))
                        }
                        Value::String(s) => format!("{k}: {s}\n"),
                        other => format!("{k}: {other}\n"),
                    };
                    out.push_str(&line);
                }
                out.push_str("---\n\n");
            }
        }
        out.push_str(content);
        out
    }

    // ------------------------------------------------------------ 解析

    /// 抽取 [[链接]]。支持 `[[目标]]` 与 `[[目标|显示名]]`，忽略 `![[嵌入]]` 的图片语法歧义。
    fn extract_links(content: &str) -> Vec<String> {
        let mut out = Vec::new();
        let bytes: Vec<char> = content.chars().collect();
        let mut i = 0usize;
        while i + 1 < bytes.len() {
            if bytes[i] == '[' && bytes[i + 1] == '[' {
                let start = i + 2;
                let mut j = start;
                while j + 1 < bytes.len() && !(bytes[j] == ']' && bytes[j + 1] == ']') {
                    j += 1;
                }
                if j + 1 < bytes.len() {
                    let inner: String = bytes[start..j].iter().collect();
                    // 支持别名语法 [[target|alias]]
                    let target = inner.split('|').next().unwrap_or("").trim();
                    // 去掉 #heading 锚点
                    let target = target.split('#').next().unwrap_or("").trim();
                    if !target.is_empty() {
                        out.push(target.to_string());
                    }
                    i = j + 2;
                    continue;
                }
            }
            i += 1;
        }
        out.sort();
        out.dedup();
        out
    }

    /// 抽取 #标签。要求 # 前是行首或空白，且标签后跟空白/标点，避免把 URL 里的 # 当标签。
    fn extract_tags(content: &str) -> Vec<String> {
        let mut out = Vec::new();
        for line in content.lines() {
            // 跳过代码块里的内容不处理（简化：跳过以 ``` 开头的行为分隔，交给上层按行统计足够）
            let chars: Vec<char> = line.chars().collect();
            let mut i = 0usize;
            while i < chars.len() {
                if chars[i] == '#' {
                    let prev_ok = i == 0 || chars[i - 1].is_whitespace() || chars[i - 1] == '(';
                    let mut j = i + 1;
                    while j < chars.len()
                        && (chars[j].is_alphanumeric()
                            || chars[j] == '-'
                            || chars[j] == '_'
                            || chars[j] == '/'
                            || ('\u{4e00}'..='\u{9fff}').contains(&chars[j]))
                    {
                        j += 1;
                    }
                    if prev_ok && j > i + 1 {
                        let tag: String = chars[i + 1..j].iter().collect();
                        out.push(tag);
                        i = j;
                        continue;
                    }
                }
                i += 1;
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn count_words(s: &str) -> usize {
        s.chars().filter(|c| !c.is_whitespace()).count()
    }

    // ------------------------------------------------------------ 索引

    /// 全量重建索引（扫描 vault 下所有 .md）
    pub fn reindex(&self) -> Result<usize> {
        let mut files: Vec<PathBuf> = Vec::new();
        collect_md(&self.root, &mut files);
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction()?;
        let mut n = 0usize;
        for f in &files {
            let rel = match f.strip_prefix(&self.root) {
                Ok(r) => r.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            if rel.starts_with('.') {
                continue;
            }
            if let Ok(raw) = std::fs::read_to_string(f) {
                let (fm, content) = Self::split_frontmatter(&raw);
                let mtime = std::fs::metadata(f)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                upsert_row(
                    &tx,
                    &rel,
                    &fm,
                    &content,
                    mtime,
                    // 已存在则保留原 id / created_at
                    true,
                )?;
                n += 1;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    /// 确保磁盘文件已同步到索引（读取前调用，保证外部编辑器改动可见）
    pub fn sync_from_disk(&self) -> Result<usize> {
        self.reindex()
    }

    // ------------------------------------------------------------ 读写

    pub fn list(&self, folder: Option<&str>, tag: Option<&str>) -> Result<Vec<Doc>> {
        let conn = self.conn.lock();
        let mut sql = String::from("SELECT * FROM docs WHERE 1=1");
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(f) = folder {
            if !f.is_empty() {
                sql.push_str(" AND (folder = ? OR folder LIKE ?)");
                args.push(Box::new(f.to_string()));
                args.push(Box::new(format!("{f}/%")));
            }
        }
        if let Some(t) = tag {
            if !t.is_empty() {
                sql.push_str(" AND tags LIKE ?");
                args.push(Box::new(format!("%\"{t}\"%")));
            }
        }
        sql.push_str(" ORDER BY folder ASC, title ASC");
        let mut stmt = conn.prepare(&sql)?;
        let ps: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(ps.as_slice(), row_to_doc)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get(&self, id_or_path: &str) -> Result<Option<Doc>> {
        let conn = self.conn.lock();
        let mut stmt =
            conn.prepare("SELECT * FROM docs WHERE id = ?1 OR path = ?2 OR title = ?3 LIMIT 1")?;
        Ok(stmt
            .query_row(params![id_or_path, id_or_path, id_or_path], row_to_doc)
            .optional()?)
    }

    /// 读取笔记正文（从磁盘读，保证最新）
    pub fn read(&self, id_or_path: &str) -> Result<Option<Doc>> {
        let Some(meta) = self.get(id_or_path)? else {
            return Ok(None);
        };
        let abs = self.abs_path(&meta.path);
        let raw = std::fs::read_to_string(&abs).unwrap_or_default();
        let (fm, content) = Self::split_frontmatter(&raw);
        Ok(Some(Doc {
            frontmatter: fm,
            links: Self::extract_links(&content),
            tags: Self::extract_tags(&content),
            word_count: Self::count_words(&content),
            content,
            ..meta
        }))
    }

    /// 新建笔记
    pub fn create(&self, patch: DocPatch) -> Result<Doc> {
        let rel = patch
            .path
            .clone()
            .ok_or_else(|| anyhow!("缺少 path"))?;
        let rel = self.safe_rel(&rel)?;
        let abs = self.abs_path(&rel);
        if abs.exists() {
            return Err(anyhow!("笔记已存在: {rel}"));
        }
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = patch.content.clone().unwrap_or_default();
        let fm = patch.frontmatter.clone().unwrap_or_else(|| json!({}));
        std::fs::write(&abs, Self::compose(&fm, &content))?;
        let mtime = now_secs();
        {
            let conn = self.conn.lock();
            upsert_row(&conn, &rel, &fm, &content, mtime, false)?;
        }
        self.read(&rel)?.ok_or_else(|| anyhow!("创建后读取失败"))
    }

    /// 保存笔记（写回磁盘 + 更新索引）
    pub fn save(&self, id_or_path: &str, patch: DocPatch) -> Result<Option<Doc>> {
        let Some(existing) = self.get(id_or_path)? else {
            return Ok(None);
        };
        let content = patch.content.unwrap_or(existing.content.clone());
        let fm = patch
            .frontmatter
            .clone()
            .or_else(|| serde_json::from_str(&existing.frontmatter.to_string()).ok())
            .unwrap_or(existing.frontmatter.clone());

        let abs = self.abs_path(&existing.path);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&abs, Self::compose(&fm, &content))?;
        {
            let conn = self.conn.lock();
            upsert_row(&conn, &existing.path, &fm, &content, now_secs(), false)?;
        }
        self.read(&existing.path)
    }

    /// 重命名 / 移动笔记
    pub fn rename(&self, id_or_path: &str, new_path: &str) -> Result<Option<Doc>> {
        let Some(existing) = self.get(id_or_path)? else {
            return Ok(None);
        };
        let rel = self.safe_rel(new_path)?;
        let from = self.abs_path(&existing.path);
        let to = self.abs_path(&rel);
        if to.exists() && to != from {
            return Err(anyhow!("目标已存在: {rel}"));
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&from, &to)?;
        {
            let conn = self.conn.lock();
            conn.execute(
                "UPDATE docs SET path=?2, title=?3, folder=?4 WHERE id=?1",
                params![existing.id, rel, Self::title_of(&rel), Self::folder_of(&rel)],
            )?;
        }
        self.read(&rel)
    }

    pub fn delete(&self, id_or_path: &str) -> Result<bool> {
        let Some(existing) = self.get(id_or_path)? else {
            return Ok(false);
        };
        let abs = self.abs_path(&existing.path);
        let _ = std::fs::remove_file(&abs);
        let conn = self.conn.lock();
        Ok(conn.execute("DELETE FROM docs WHERE id=?1", [&existing.id])? > 0)
    }

    /// 全文检索（标题命中排在前面）
    pub fn search(&self, keyword: &str) -> Result<Vec<Doc>> {
        let kw = keyword.trim();
        if kw.is_empty() {
            return Ok(vec![]);
        }
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT * FROM docs
             WHERE title LIKE ?1 OR content LIKE ?1 OR tags LIKE ?1
             ORDER BY (CASE WHEN title LIKE ?1 THEN 0 ELSE 1 END), updated_at DESC
             LIMIT 200",
        )?;
        let like = format!("%{kw}%");
        let rows = stmt.query_map([like], row_to_doc)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// 反向链接：谁引用了这篇笔记
    pub fn backlinks(&self, id_or_path: &str) -> Result<Vec<Backlink>> {
        let Some(target) = self.get(id_or_path)? else {
            return Ok(vec![]);
        };
        let names = format!("%[[{}%", target.title);
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, title, path, content FROM docs
             WHERE id != ?1 AND (content LIKE ?2 OR content LIKE ?3)",
        )?;
        let with_alias = format!("%[[{}|%", target.title);
        let mut rows = stmt.query(params![target.id, names, with_alias])?;
        let mut out = Vec::new();
        while let Some(r) = rows.next()? {
            let content: String = r.get(3)?;
            let ctx = content
                .lines()
                .find(|l| l.contains(&format!("[[{}", target.title)))
                .unwrap_or("")
                .trim()
                .chars()
                .take(160)
                .collect::<String>();
            out.push(Backlink {
                id: r.get(0)?,
                title: r.get(1)?,
                path: r.get(2)?,
                context: ctx,
            });
        }
        Ok(out)
    }

    /// 出链：这篇笔记引用了谁
    pub fn outlinks(&self, id_or_path: &str) -> Result<Vec<Doc>> {
        let Some(doc) = self.get(id_or_path)? else {
            return Ok(vec![]);
        };
        let mut out = Vec::new();
        for l in &doc.links {
            if let Some(d) = self.get(l)? {
                out.push(d);
            }
        }
        Ok(out)
    }

    /// 全部标签及计数
    pub fn tags(&self) -> Result<Vec<(String, usize)>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT tags FROM docs")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut counter: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for r in rows {
            let raw = r?;
            if let Ok(list) = serde_json::from_str::<Vec<String>>(&raw) {
                for t in list {
                    *counter.entry(t).or_insert(0) += 1;
                }
            }
        }
        let mut v: Vec<(String, usize)> = counter.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        Ok(v)
    }

    /// 笔记树（文件夹结构）
    pub fn tree(&self) -> Result<Vec<DocNode>> {
        let docs = self.list(None, None)?;
        let mut root: Vec<DocNode> = Vec::new();
        for d in docs {
            insert_node(&mut root, &d);
        }
        sort_nodes(&mut root);
        Ok(root)
    }

    /// vault 概览
    ///
    /// ⚠️ 注意：`self.conn` 是 `parking_lot::Mutex`，**不可重入**。
    /// 持有锁的 guard 还活着时，绝不能再调用任何会自己加锁的方法
    /// （如 `self.tags()` / `self.list()` / `self.get()`），否则同一线程
    /// 二次 `lock()` 会**立即永久死锁**。由于这些命令跑在 Tauri 主线程上，
    /// 一旦死锁整个 UI 会彻底冻住（IPC 全部超时，但 HTTP 接口照常响应，
    /// 现象极具迷惑性）。因此这里显式用块把 guard 的生命周期收窄，
    /// 锁释放后再去取 tags。
    pub fn info(&self) -> Result<VaultInfo> {
        let docs = self.list(None, None)?;
        let (folder_count, linked) = {
            let conn = self.conn.lock();
            let folder_count: i64 = conn.query_row(
                "SELECT COUNT(DISTINCT folder) FROM docs WHERE folder != ''",
                [],
                |r| r.get(0),
            )?;
            let all_links: Vec<String> = {
                let mut stmt = conn.prepare("SELECT links FROM docs")?;
                let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            let linked: std::collections::HashSet<String> = all_links
                .iter()
                .filter_map(|s| serde_json::from_str::<Vec<String>>(s).ok())
                .flatten()
                .collect();
            (folder_count as usize, linked)
        }; // guard 在此释放
        let tags = self.tags()?;
        let orphans = docs
            .iter()
            .filter(|d| d.links.is_empty() && !linked.contains(&d.title))
            .count();
        Ok(VaultInfo {
            root: self.root.to_string_lossy().to_string(),
            doc_count: docs.len(),
            folder_count,
            tags,
            orphans,
        })
    }
}

// ============================================================ 辅助

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        if p.is_dir() {
            collect_md(&p, out);
        } else if p.extension().map(|x| x == "md").unwrap_or(false) {
            out.push(p);
        }
    }
}

fn row_to_doc(r: &rusqlite::Row) -> rusqlite::Result<Doc> {
    let fm_raw: String = r.get("frontmatter")?;
    let links_raw: String = r.get("links")?;
    let tags_raw: String = r.get("tags")?;
    Ok(Doc {
        id: r.get("id")?,
        path: r.get("path")?,
        title: r.get("title")?,
        folder: r.get("folder")?,
        content: r.get("content")?,
        frontmatter: serde_json::from_str(&fm_raw).unwrap_or_else(|_| json!({})),
        links: serde_json::from_str(&links_raw).unwrap_or_default(),
        tags: serde_json::from_str(&tags_raw).unwrap_or_default(),
        word_count: r.get::<_, i64>("word_count")? as usize,
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
    })
}

/// 写入或更新一行索引。`keep_id=true` 表示已存在时保留原有 id（重建索引用）。
fn upsert_row(
    conn: &Connection,
    rel: &str,
    fm: &Value,
    content: &str,
    mtime: i64,
    keep_id: bool,
) -> Result<()> {
    let title = Vault::title_of(rel);
    let folder = Vault::folder_of(rel);
    let links = json!(Vault::extract_links(content));
    let tags = json!(Vault::extract_tags(content));
    let wc = Vault::count_words(content) as i64;
    let now = Utc::now().to_rfc3339();

    let existing_id: Option<String> = conn
        .query_row("SELECT id FROM docs WHERE path = ?1", [rel], |r| r.get(0))
        .optional()?;

    match existing_id {
        Some(id) if keep_id => {
            conn.execute(
                "UPDATE docs SET title=?2, folder=?3, content=?4, frontmatter=?5, links=?6,
                                 tags=?7, word_count=?8, updated_at=?9, mtime=?10
                 WHERE id=?1",
                params![
                    id,
                    title,
                    folder,
                    content,
                    fm.to_string(),
                    links.to_string(),
                    tags.to_string(),
                    wc,
                    now,
                    mtime
                ],
            )?;
        }
        Some(id) => {
            conn.execute(
                "UPDATE docs SET title=?2, folder=?3, content=?4, frontmatter=?5, links=?6,
                                 tags=?7, word_count=?8, updated_at=?9, mtime=?10
                 WHERE id=?1",
                params![
                    id,
                    title,
                    folder,
                    content,
                    fm.to_string(),
                    links.to_string(),
                    tags.to_string(),
                    wc,
                    now,
                    mtime
                ],
            )?;
        }
        None => {
            conn.execute(
                "INSERT INTO docs (id, path, title, folder, content, frontmatter, links, tags,
                                   word_count, created_at, updated_at, mtime)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    rel,
                    title,
                    folder,
                    content,
                    fm.to_string(),
                    links.to_string(),
                    tags.to_string(),
                    wc,
                    now,
                    now,
                    mtime
                ],
            )?;
        }
    }
    Ok(())
}

fn insert_node(roots: &mut Vec<DocNode>, doc: &Doc) {
    let segs: Vec<&str> = if doc.folder.is_empty() {
        vec![]
    } else {
        doc.folder.split('/').collect()
    };
    let mut cur = roots;
    let mut acc = String::new();
    for seg in segs {
        acc = if acc.is_empty() {
            seg.to_string()
        } else {
            format!("{acc}/{seg}")
        };
        let pos = cur.iter().position(|n| n.is_dir && n.name == seg);
        let idx = match pos {
            Some(i) => i,
            None => {
                cur.push(DocNode {
                    name: seg.to_string(),
                    path: acc.clone(),
                    is_dir: true,
                    children: vec![],
                });
                cur.len() - 1
            }
        };
        cur = &mut cur[idx].children;
    }
    cur.push(DocNode {
        name: doc.title.clone(),
        path: doc.path.clone(),
        is_dir: false,
        children: vec![],
    });
}

fn sort_nodes(nodes: &mut Vec<DocNode>) {
    nodes.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    for n in nodes.iter_mut() {
        sort_nodes(&mut n.children);
    }
}

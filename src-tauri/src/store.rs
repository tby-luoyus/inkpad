//! SQLite 持久化层
//!
//! 采用单连接 + Mutex 的方式，读多写少的桌面场景足够，
//! 且避免了连接池带来的复杂度。所有写操作走事务。

use anyhow::{Context, Result};
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::models::{BackupFile, Note, NotePatch, NoteQuery, Settings};

pub struct Store {
    conn: Mutex<Connection>,
    pub db_path: PathBuf,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS notes (
    id           TEXT PRIMARY KEY,
    content      TEXT NOT NULL DEFAULT '',
    color        TEXT NOT NULL DEFAULT 'yellow',
    x            INTEGER NOT NULL DEFAULT 0,
    y            INTEGER NOT NULL DEFAULT 0,
    width        INTEGER NOT NULL DEFAULT 320,
    height       INTEGER NOT NULL DEFAULT 360,
    is_open      INTEGER NOT NULL DEFAULT 0,
    is_pinned    INTEGER NOT NULL DEFAULT 0,
    z_index      INTEGER NOT NULL DEFAULT 0,
    deleted      INTEGER NOT NULL DEFAULT 0,
    hidden       INTEGER NOT NULL DEFAULT 0,
    font_scale   REAL NOT NULL DEFAULT 1.0,
    created_at   TEXT NOT NULL,
    updated_at   TEXT NOT NULL,
    deleted_at   TEXT,
    meta         TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_notes_updated  ON notes(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_notes_deleted  ON notes(deleted);
CREATE INDEX IF NOT EXISTS idx_notes_zindex   ON notes(z_index DESC);

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

fn row_to_note(row: &Row) -> rusqlite::Result<Note> {
    let meta_raw: String = row.get("meta")?;
    let meta = serde_json::from_str(&meta_raw).unwrap_or_else(|_| serde_json::json!({}));
    Ok(Note {
        id: row.get("id")?,
        content: row.get("content")?,
        color: row.get("color")?,
        x: row.get("x")?,
        y: row.get("y")?,
        width: row.get("width")?,
        height: row.get("height")?,
        is_open: row.get::<_, i64>("is_open")? != 0,
        is_pinned: row.get::<_, i64>("is_pinned")? != 0,
        z_index: row.get("z_index")?,
        deleted: row.get::<_, i64>("deleted")? != 0,
        hidden: row.get::<_, i64>("hidden")? != 0,
        font_scale: row.get("font_scale")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        deleted_at: row.get("deleted_at")?,
        meta,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("创建数据目录失败")?;
        }
        let conn = Connection::open(path).context("打开数据库失败")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
            db_path: path.to_path_buf(),
        })
    }

    // ---------------------------------------------------------------- notes

    pub fn list_notes(&self, query: &NoteQuery) -> Result<Vec<Note>> {
        let conn = self.conn.lock();
        let mut sql = String::from("SELECT * FROM notes WHERE 1=1");
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if query.only_deleted.unwrap_or(false) {
            sql.push_str(" AND deleted = 1");
        } else if !query.include_deleted.unwrap_or(false) {
            sql.push_str(" AND deleted = 0");
        }

        if let Some(kw) = query.keyword.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            sql.push_str(" AND content LIKE ?");
            args.push(Box::new(format!("%{kw}%")));
        }

        if let Some(colors) = query.colors.as_ref().filter(|c| !c.is_empty()) {
            let holders = vec!["?"; colors.len()].join(",");
            sql.push_str(&format!(" AND color IN ({holders})"));
            for c in colors {
                args.push(Box::new(c.clone()));
            }
        }

        let order = match query.sort.as_deref().unwrap_or("updated") {
            "created" => "created_at DESC",
            "color" => "color ASC, updated_at DESC",
            "alpha" => "LOWER(content) ASC",
            "zindex" => "z_index DESC",
            _ => "updated_at DESC",
        };
        sql.push_str(&format!(" ORDER BY is_pinned DESC, {order}"));

        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(params.as_slice(), row_to_note)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get_note(&self, id: &str) -> Result<Option<Note>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT * FROM notes WHERE id = ?")?;
        Ok(stmt.query_row([id], row_to_note).optional()?)
    }

    pub fn create_note(&self, patch: NotePatch, settings: &Settings) -> Result<Note> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let max_z: i64 = self
            .conn
            .lock()
            .query_row("SELECT COALESCE(MAX(z_index), 0) FROM notes", [], |r| r.get(0))
            .unwrap_or(0);

        let note = Note {
            id: id.clone(),
            content: patch.content.clone().unwrap_or_default(),
            color: patch.color.clone().unwrap_or_else(|| settings.default_color.clone()),
            x: patch.x.unwrap_or(0),
            y: patch.y.unwrap_or(0),
            width: patch.width.unwrap_or(settings.default_note_width),
            height: patch.height.unwrap_or(settings.default_note_height),
            is_open: patch.is_open.unwrap_or(false),
            is_pinned: patch.is_pinned.unwrap_or(false),
            z_index: patch.z_index.unwrap_or(max_z + 1),
            deleted: false,
            hidden: false,
            font_scale: patch.font_scale.unwrap_or(settings.default_font_scale),
            created_at: now.clone(),
            updated_at: now,
            deleted_at: None,
            meta: patch.meta.unwrap_or_else(|| serde_json::json!({})),
        };

        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO notes (id, content, color, x, y, width, height, is_open, is_pinned,
                                z_index, deleted, hidden, font_scale, created_at, updated_at, deleted_at, meta)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,0,0,?11,?12,?13,NULL,?14)",
            params![
                note.id, note.content, note.color, note.x, note.y, note.width, note.height,
                note.is_open as i64, note.is_pinned as i64, note.z_index, note.font_scale,
                note.created_at, note.updated_at,
                serde_json::to_string(&note.meta)?
            ],
        )?;
        Ok(note)
    }

    pub fn update_note(&self, id: &str, patch: &NotePatch) -> Result<Option<Note>> {
        let Some(existing) = self.get_note(id)? else {
            return Ok(None);
        };
        let now = Utc::now().to_rfc3339();

        // 只有内容/颜色/字号变化才视为「修改时间」更新，避免拖窗口刷屏
        let content_changed = patch.content.is_some();
        let updated_at = if content_changed { now } else { existing.updated_at.clone() };

        let merged = Note {
            content: patch.content.clone().unwrap_or(existing.content),
            color: patch.color.clone().unwrap_or(existing.color),
            x: patch.x.unwrap_or(existing.x),
            y: patch.y.unwrap_or(existing.y),
            width: patch.width.unwrap_or(existing.width),
            height: patch.height.unwrap_or(existing.height),
            is_open: patch.is_open.unwrap_or(existing.is_open),
            is_pinned: patch.is_pinned.unwrap_or(existing.is_pinned),
            z_index: patch.z_index.unwrap_or(existing.z_index),
            font_scale: patch.font_scale.unwrap_or(existing.font_scale),
            hidden: patch.hidden.unwrap_or(existing.hidden),
            meta: patch.meta.clone().unwrap_or(existing.meta),
            updated_at,
            ..existing
        };

        let conn = self.conn.lock();
        conn.execute(
            "UPDATE notes SET content=?2, color=?3, x=?4, y=?5, width=?6, height=?7, is_open=?8,
                              is_pinned=?9, z_index=?10, font_scale=?11, hidden=?12, updated_at=?13, meta=?14
             WHERE id=?1",
            params![
                id, merged.content, merged.color, merged.x, merged.y, merged.width, merged.height,
                merged.is_open as i64, merged.is_pinned as i64, merged.z_index, merged.font_scale,
                merged.hidden as i64, merged.updated_at, serde_json::to_string(&merged.meta)?
            ],
        )?;
        Ok(Some(merged))
    }

    /// 软删除 -> 回收站
    pub fn trash_note(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "UPDATE notes SET deleted=1, deleted_at=?, is_open=0 WHERE id=?",
            params![Utc::now().to_rfc3339(), id],
        )?;
        Ok(n > 0)
    }

    /// 从回收站恢复
    pub fn restore_note(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "UPDATE notes SET deleted=0, deleted_at=NULL WHERE id=?",
            [id],
        )?;
        Ok(n > 0)
    }

    /// 彻底删除
    pub fn purge_note(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock();
        Ok(conn.execute("DELETE FROM notes WHERE id=?", [id])? > 0)
    }

    pub fn purge_all_trashed(&self) -> Result<usize> {
        let conn = self.conn.lock();
        Ok(conn.execute("DELETE FROM notes WHERE deleted=1", [])?)
    }

    /// 自动清理超过保留期的回收站内容
    pub fn purge_expired_trash(&self, days: i64) -> Result<usize> {
        if days <= 0 {
            return Ok(0);
        }
        let cutoff = (Utc::now() - chrono::Duration::days(days)).to_rfc3339();
        let conn = self.conn.lock();
        Ok(conn.execute(
            "DELETE FROM notes WHERE deleted=1 AND deleted_at IS NOT NULL AND deleted_at < ?",
            [cutoff],
        )?)
    }

    pub fn bring_to_front(&self, id: &str) -> Result<i64> {
        let conn = self.conn.lock();
        let max_z: i64 = conn.query_row("SELECT COALESCE(MAX(z_index), 0) FROM notes", [], |r| r.get(0))?;
        let next = max_z + 1;
        conn.execute("UPDATE notes SET z_index=?2 WHERE id=?1", params![id, next])?;
        Ok(next)
    }

    pub fn duplicate_note(&self, id: &str, settings: &Settings) -> Result<Option<Note>> {
        let Some(src) = self.get_note(id)? else {
            return Ok(None);
        };
        let patch = NotePatch {
            content: Some(src.content.clone()),
            color: Some(src.color.clone()),
            x: Some(src.x + 28),
            y: Some(src.y + 28),
            width: Some(src.width),
            height: Some(src.height),
            font_scale: Some(src.font_scale),
            meta: Some(src.meta.clone()),
            ..Default::default()
        };
        let mut n = self.create_note(patch, settings)?;
        n.is_open = src.is_open;
        Ok(Some(n))
    }

    pub fn count_all(&self) -> Result<(usize, usize, usize)> {
        let conn = self.conn.lock();
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))?;
        let active: i64 = conn.query_row("SELECT COUNT(*) FROM notes WHERE deleted=0", [], |r| r.get(0))?;
        let trashed: i64 = conn.query_row("SELECT COUNT(*) FROM notes WHERE deleted=1", [], |r| r.get(0))?;
        Ok((total as usize, active as usize, trashed as usize))
    }

    // ------------------------------------------------------------- settings

    pub fn get_settings(&self) -> Result<Settings> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key='app'")?;
        let raw: Option<String> = stmt.query_row([], |r| r.get(0)).optional()?;
        drop(stmt);
        match raw {
            Some(s) => Ok(serde_json::from_str(&s).unwrap_or_default()),
            None => Ok(Settings::default()),
        }
    }

    pub fn save_settings(&self, s: &Settings) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('app', ?1)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [serde_json::to_string(s)?],
        )?;
        Ok(())
    }

    // -------------------------------------------------------------- backup

    pub fn export_backup(&self, with_settings: bool) -> Result<BackupFile> {
        let notes = self.list_notes(&NoteQuery {
            include_deleted: Some(true),
            sort: Some("created".into()),
            ..Default::default()
        })?;
        Ok(BackupFile {
            version: 1,
            app: "sticky-notes".into(),
            exported_at: Utc::now().to_rfc3339(),
            notes,
            settings: if with_settings { Some(self.get_settings()?) } else { None },
        })
    }

    pub fn import_backup(&self, b: &BackupFile, overwrite: bool) -> Result<usize> {
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction()?;
        let mut n = 0usize;
        for note in &b.notes {
            let exists: bool = tx
                .query_row("SELECT 1 FROM notes WHERE id=?", [&note.id], |_| Ok(true))
                .optional()?
                .unwrap_or(false);
            if exists && !overwrite {
                continue;
            }
            tx.execute(
                "INSERT INTO notes (id, content, color, x, y, width, height, is_open, is_pinned,
                                    z_index, deleted, hidden, font_scale, created_at, updated_at, deleted_at, meta)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
                 ON CONFLICT(id) DO UPDATE SET
                    content=excluded.content, color=excluded.color, x=excluded.x, y=excluded.y,
                    width=excluded.width, height=excluded.height, is_pinned=excluded.is_pinned,
                    deleted=excluded.deleted, hidden=excluded.hidden, font_scale=excluded.font_scale,
                    updated_at=excluded.updated_at, deleted_at=excluded.deleted_at, meta=excluded.meta",
                params![
                    note.id, note.content, note.color, note.x, note.y, note.width, note.height,
                    note.is_open as i64, note.is_pinned as i64, note.z_index,
                    note.deleted as i64, note.hidden as i64, note.font_scale,
                    note.created_at, note.updated_at, note.deleted_at,
                    serde_json::to_string(&note.meta)?
                ],
            )?;
            n += 1;
        }
        tx.commit()?;
        Ok(n)
    }
}

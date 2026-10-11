//! Markdown 生成：把便签渲染成结构化笔记。
//!
//! 行为与 Python 版严格对齐（字段、分组、待办识别规则一致）。

use chrono::{DateTime, Local, NaiveDateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use crate::{color_label, title_of};

/// 现在（本地时区），用于「生成于」时间戳
pub fn now_local() -> DateTime<Local> {
    Local::now()
}

/// 解析 ISO 时间字符串，统一返回 **naive UTC**，
/// 避免 aware / naive 混用导致比较 panic（Python 版踩过这个坑）。
fn parse_dt(s: &str) -> Option<NaiveDateTime> {
    if s.is_empty() {
        return None;
    }
    // 先试带时区的
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc).naive_utc());
    }
    // Ruby/JS 风格的 "+00:00" 已覆盖在上面；再试无时区
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(dt);
    }
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt);
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(&s[..10.min(s.len())], "%Y-%m-%d") {
        return Some(d.and_hms_opt(0, 0, 0)?);
    }
    None
}

/// 抽取正文里的 #标签（支持中文、`/`、`-`）
pub fn extract_tags(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' {
            // 前一个字符必须是空白或行首或 ( —— 避免误抓 URL 里的 #
            let ok_prefix = i == 0
                || chars[i - 1].is_whitespace()
                || chars[i - 1] == '('
                || chars[i - 1] == '（';
            if ok_prefix {
                let mut j = i + 1;
                let mut tag = String::new();
                while j < chars.len() {
                    let c = chars[j];
                    if c.is_alphanumeric() || c == '_' || c == '-' || c == '/' {
                        tag.push(c);
                        j += 1;
                    } else {
                        break;
                    }
                }
                if !tag.is_empty() {
                    out.push(tag);
                    i = j;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

/// 待办识别规则（与 Python 版逐条对齐）
fn match_todo(line: &str) -> Option<String> {
    let t = line.trim_start();
    // - [ ] xxx
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(bullet) {
            if let Some(r) = rest.strip_prefix("[ ]") {
                return Some(r.trim().to_string());
            }
            // - TODO: xxx / - 待办 xxx
            let low = rest.to_lowercase();
            if low.starts_with("todo") || rest.starts_with("待办") {
                let r = rest
                    .trim_start_matches(|c: char| c.is_alphabetic() && c != '待')
                    .trim_start_matches("TODO")
                    .trim_start_matches("todo")
                    .trim_start_matches("待办")
                    .trim_start_matches([':', '：'])
                    .trim();
                if !r.is_empty() {
                    return Some(r.to_string());
                }
            }
        }
    }
    // [ ] xxx
    if let Some(r) = t.strip_prefix("[ ]") {
        return Some(r.trim().to_string());
    }
    // #todo xxx
    let low = t.to_lowercase();
    if low.starts_with("#todo") {
        let r = t[5..].trim_start_matches([':', '：']).trim();
        if !r.is_empty() {
            return Some(r.to_string());
        }
    }
    // TODO: xxx / 待办：xxx
    for kw in ["TODO:", "TODO：", "todo:", "待办:", "待办："] {
        if t.starts_with(kw) {
            let r = t[kw.len()..].trim();
            if !r.is_empty() {
                return Some(r.to_string());
            }
        }
    }
    None
}

fn match_done(line: &str) -> Option<String> {
    let t = line.trim_start();
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(bullet) {
            if let Some(r) = rest.strip_prefix("[x]").or_else(|| rest.strip_prefix("[X]")) {
                return Some(r.trim().to_string());
            }
        }
    }
    let low = t.to_lowercase();
    if low.starts_with("#done") {
        let r = t[5..].trim_start_matches([':', '：']).trim();
        if !r.is_empty() {
            return Some(r.to_string());
        }
    }
    None
}

/// 从所有便签里抽出待办与已完成项
pub fn extract_todos(notes: &[Value]) -> (Vec<(String, String)>, Vec<(String, String)>) {
    let mut open = Vec::new();
    let mut done = Vec::new();
    for n in notes {
        let title = {
            let t = title_of(n);
            if t.is_empty() { "未命名".to_string() } else { t }
        };
        let content = n.get("content").and_then(|v| v.as_str()).unwrap_or("");
        for line in content.lines() {
            if let Some(item) = match_done(line) {
                done.push((title.clone(), item));
            } else if let Some(item) = match_todo(line) {
                open.push((title.clone(), item));
            }
        }
    }
    (open, done)
}

/// 核心：把便签列表渲染成一篇结构化 Markdown
pub fn build_summary(notes: &[Value], title: &str, group_by: &str) -> String {
    let now = now_local();
    let mut out: Vec<String> = Vec::new();

    out.push(format!("# {title}\n"));
    out.push(format!(
        "> 由 **便签总结** 插件生成 · {} · 共 {} 张便签\n",
        now.format("%Y-%m-%d %H:%M"),
        notes.len()
    ));

    // ---- 概览
    let total_chars: usize = notes
        .iter()
        .map(|n| n.get("content").and_then(|v| v.as_str()).unwrap_or("").chars().count())
        .sum();
    let pinned = notes
        .iter()
        .filter(|n| n.get("isPinned").and_then(|v| v.as_bool()).unwrap_or(false))
        .count();
    let mut colors: HashMap<String, i64> = HashMap::new();
    for n in notes {
        let c = n
            .get("color")
            .and_then(|v| v.as_str())
            .unwrap_or("yellow")
            .to_string();
        *colors.entry(c).or_insert(0) += 1;
    }
    let mut color_pairs: Vec<(String, i64)> = colors.into_iter().collect();
    color_pairs.sort_by(|a, b| b.1.cmp(&a.1));

    out.push("## 概览\n".into());
    out.push("| 指标 | 数值 |".into());
    out.push("| --- | --- |".into());
    out.push(format!("| 便签数量 | {} |", notes.len()));
    out.push(format!("| 置顶数量 | {pinned} |"));
    out.push(format!("| 总字数 | {total_chars} |"));
    out.push(format!(
        "| 颜色分布 | {} |",
        color_pairs
            .iter()
            .map(|(c, v)| format!("{} {}", color_label(c), v))
            .collect::<Vec<_>>()
            .join("、")
    ));
    out.push(String::new());

    // ---- 待办速览
    let (todos, done) = extract_todos(notes);
    if !todos.is_empty() {
        out.push("## 待办速览\n".into());
        for (src, item) in &todos {
            out.push(format!("- [ ] {item}  <sub>来自「{src}」</sub>"));
        }
        out.push(String::new());
    }
    if !done.is_empty() {
        out.push("## 已完成\n".into());
        for (src, item) in &done {
            out.push(format!("- [x] {item}  <sub>来自「{src}」</sub>"));
        }
        out.push(String::new());
    }

    // ---- 目录
    out.push("## 目录\n".into());
    for (i, n) in notes.iter().enumerate() {
        let t = {
            let t = title_of(n);
            if t.is_empty() { format!("便签 {}", i + 1) } else { t }
        };
        out.push(format!("{}. {}", i + 1, one_line(&t)));
    }
    out.push(String::new());
    out.push("---\n".into());

    // ---- 分组正文
    match group_by {
        "color" => {
            let mut buckets: HashMap<String, Vec<&Value>> = HashMap::new();
            for n in notes {
                let c = n
                    .get("color")
                    .and_then(|v| v.as_str())
                    .unwrap_or("yellow")
                    .to_string();
                buckets.entry(c).or_default().push(n);
            }
            let mut entries: Vec<(String, Vec<&Value>)> = buckets.into_iter().collect();
            entries.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
            for (color, items) in entries {
                out.push(format!("## {}（{}）\n", color_label(&color), items.len()));
                for (i, n) in items.iter().enumerate() {
                    out.push(render_note(n, i + 1));
                }
            }
        }
        "tag" => {
            let mut buckets: HashMap<String, Vec<&Value>> = HashMap::new();
            for n in notes {
                let content = n.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let tags = extract_tags(content);
                if tags.is_empty() {
                    buckets.entry("（无标签）".to_string()).or_default().push(n);
                }
                for tg in tags {
                    buckets.entry(tg).or_default().push(n);
                }
            }
            let mut entries: Vec<(String, Vec<&Value>)> = buckets.into_iter().collect();
            entries.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
            for (tag, items) in entries {
                out.push(format!("## #{tag}（{}）\n", items.len()));
                for (i, n) in items.iter().enumerate() {
                    out.push(render_note(n, i + 1));
                }
            }
        }
        _ => {
            out.push("## 正文\n".into());
            for (i, n) in notes.iter().enumerate() {
                out.push(render_note(n, i + 1));
            }
        }
    }

    out.join("\n")
}

fn one_line(s: &str) -> String {
    s.replace('\n', " ")
}

/// 渲染单张便签
fn render_note(n: &Value, idx: usize) -> String {
    let t = {
        let t = title_of(n);
        if t.is_empty() { format!("便签 {idx}") } else { t }
    };
    let mut out = vec![format!("### {}\n", one_line(&t))];

    let color = n.get("color").and_then(|v| v.as_str()).unwrap_or("yellow");
    let updated = n
        .get("updatedAt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .chars()
        .take(16)
        .collect::<String>();
    let mut meta = vec![
        format!("颜色 `{}`", color_label(color)),
        format!("更新 {updated}"),
    ];
    if n.get("isPinned").and_then(|v| v.as_bool()).unwrap_or(false) {
        meta.push("已置顶".into());
    }
    out.push(format!("<sub>{}</sub>\n", meta.join(" · ")));

    // 去掉与标题重复的首行
    let content = n.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let body = content.trim();
    let mut body_lines: Vec<&str> = body.lines().collect();
    if let Some(first) = body_lines.first() {
        if first.trim() == t.trim() {
            body_lines.remove(0);
        }
    }
    let body = body_lines.join("\n");
    let body = body.trim();
    out.push(if body.is_empty() {
        "_（空便签）_".into()
    } else {
        body.to_string()
    });
    out.push(String::new());
    out.join("\n")
}

/// 待办清单笔记
pub fn build_todos_doc(
    open: &[(String, String)],
    done: &[(String, String)],
    title: &str,
) -> String {
    let now = now_local();
    let mut out = vec![
        format!("# {title}\n"),
        format!("> 由便签总结插件生成 · {}\n", now.format("%Y-%m-%d %H:%M")),
        format!("## 进行中（{}）\n", open.len()),
    ];
    if open.is_empty() {
        out.push("_暂无待办_".into());
    } else {
        for (src, item) in open {
            out.push(format!("- [ ] {item}  <sub>来自「{src}」</sub>"));
        }
    }
    out.push(String::new());
    out.push(format!("## 已完成（{}）\n", done.len()));
    if done.is_empty() {
        out.push("_暂无_".into());
    } else {
        for (src, item) in done {
            out.push(format!("- [x] {item}  <sub>来自「{src}」</sub>"));
        }
    }
    out.push(String::new());
    out.join("\n")
}

/// 统计笔记
pub fn build_stats_doc(notes: &[Value], days: i64, title: &str) -> String {
    let now_utc = Utc::now().naive_utc();
    let cutoff = now_utc - chrono::Duration::days(days);
    let recent = notes
        .iter()
        .filter(|n| {
            let s = n.get("updatedAt").and_then(|v| v.as_str()).unwrap_or("");
            parse_dt(s).map(|d| d >= cutoff).unwrap_or(true)
        })
        .count();

    let mut colors: HashMap<String, i64> = HashMap::new();
    for n in notes {
        let c = n
            .get("color")
            .and_then(|v| v.as_str())
            .unwrap_or("yellow")
            .to_string();
        *colors.entry(c).or_insert(0) += 1;
    }
    let total_chars: usize = notes
        .iter()
        .map(|n| n.get("content").and_then(|v| v.as_str()).unwrap_or("").chars().count())
        .sum();
    let avg = if notes.is_empty() {
        0
    } else {
        total_chars / notes.len()
    };

    let mut per_day: HashMap<String, i64> = HashMap::new();
    for n in notes {
        let s = n.get("updatedAt").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(dt) = parse_dt(s) {
            *per_day.entry(dt.format("%Y-%m-%d").to_string()).or_insert(0) += 1;
        }
    }

    let mut out = vec![
        format!("# {title}\n"),
        format!("> 由便签总结插件生成 · {}\n", now_local().format("%Y-%m-%d %H:%M")),
        "## 总体\n".into(),
        "| 指标 | 数值 |".into(),
        "| --- | --- |".into(),
        format!("| 便签总数 | {} |", notes.len()),
        format!("| 最近 {days} 天有更新 | {recent} |"),
        format!("| 总字数 | {total_chars} |"),
        format!("| 平均每张字数 | {avg} |"),
        String::new(),
        "## 颜色分布\n".into(),
    ];

    let mut cp: Vec<(String, i64)> = colors.into_iter().collect();
    cp.sort_by(|a, b| b.1.cmp(&a.1));
    let max_c = cp.iter().map(|(_, v)| *v).max().unwrap_or(1).max(1);
    for (c, v) in cp {
        let n = ((v as f64 / max_c as f64) * 24.0).round() as usize;
        let bar = "█".repeat(n.max(1));
        out.push(format!("- {} `{bar}` {v}", color_label(&c)));
    }
    out.push(String::new());

    if !per_day.is_empty() {
        out.push("## 更新活跃度（按天）\n".into());
        out.push("| 日期 | 便签数 |".into());
        out.push("| --- | --- |".into());
        let mut days_sorted: Vec<(String, i64)> = per_day.into_iter().collect();
        days_sorted.sort_by(|a, b| b.0.cmp(&a.0));
        for (d, c) in days_sorted.into_iter().take(30) {
            out.push(format!("| {d} | {c} |"));
        }
        out.push(String::new());
    }

    out.join("\n")
}

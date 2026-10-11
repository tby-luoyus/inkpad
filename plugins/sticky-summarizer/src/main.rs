//! 便签总结生成笔记 —— 便签应用的第一个内置插件（Rust 版）
//!
//! # 职责
//!
//! 订阅便签事件、注册三条命令，把便签内容整理成结构化 Markdown 笔记。
//!
//! # 为什么用 HTTP 而不是直接读写数据库？
//!
//! 插件契约刻意设计成 HTTP + JSON，好处是：
//!   * 插件可以用**任意语言**写（这是 Rust 实现，零运行时依赖，单文件 exe）
//!   * 插件不接触数据库，避免并发写坏数据；所有写操作都经便签应用的命令总线
//!   * 插件可以跑在别的进程、甚至别的机器
//!
//! # 命令
//!
//!   summary.notes   把便签汇总成一篇笔记（核心功能）
//!   summary.todos   提取便签里的待办项，生成清单
//!   summary.stats   统计便签分布，生成统计笔记
//!
//! # 启动
//!
//!   sticky-summarizer                     默认监听 127.0.0.1:43110
//!   sticky-summarizer --port 43110 --app http://127.0.0.1:43919
//!
//! 若便签应用的「本地 HTTP API」未开启，插件需要它来读写数据：
//! 在应用 设置 → 开发者 里打开「启用本地 HTTP API」，默认端口 43919。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::time::Duration;

mod md;
mod sticky;

use md::*;
use sticky::StickyClient;

const DEFAULT_PORT: u16 = 43110;
const DEFAULT_APP: &str = "http://127.0.0.1:43919";

fn main() -> ExitCode {
    let mut port = DEFAULT_PORT;
    let mut app = DEFAULT_APP.to_string();

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse::<u16>().ok()) {
                    port = v;
                    i += 2;
                    continue;
                }
                i += 1;
            }
            "--app" | "-a" => {
                if let Some(v) = args.get(i + 1) {
                    app = v.clone();
                    i += 2;
                    continue;
                }
                i += 1;
            }
            "--help" | "-h" => {
                println!(
                    "便签总结生成笔记 v{} —— 监听 127.0.0.1:{}\n\
                     用法: sticky-summarizer [--port 43110] [--app http://127.0.0.1:43919]",
                    env!("CARGO_PKG_VERSION"),
                    DEFAULT_PORT
                );
                return ExitCode::SUCCESS;
            }
            _ => i += 1,
        }
    }

    let addr = format!("127.0.0.1:{port}");
    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            // 端口被占：说明已经有一个实例在跑（宿主也会主动探测并复用），
            // 这里安静退出，不要打错误日志污染宿主。
            eprintln!("[summarizer] 无法绑定 {addr}: {e}（可能已有实例在运行）");
            return ExitCode::FAILURE;
        }
    };

    eprintln!("[summarizer] 已启动 → http://{addr}/sticky");
    eprintln!("[summarizer] 宿主: {app}");

    let client = StickyClient::new(&app);

    // 启动时探测宿主，方便排查「API 没开」这类问题。
    // 注意：宿主拉起插件的时机早于自身 HTTP API 就绪（相差几百毫秒很常见），
    // 所以这里要重试几次再下结论，否则会误报「无法连接」。
    let mut connected = None;
    for attempt in 1..=8 {
        match client.health() {
            Some(h) => {
                connected = Some(h);
                break;
            }
            None => {
                if attempt == 1 {
                    eprintln!("[summarizer] 宿主 {app} 尚未就绪，等待中…");
                }
                std::thread::sleep(std::time::Duration::from_millis(750));
            }
        }
    }
    match connected {
        Some(h) => eprintln!("[summarizer] 已连接便签应用: v{}", h.version),
        None => eprintln!(
            "[summarizer] 警告：等待约 6 秒后仍无法连接 {app}\n\
             [summarizer]       请在 便签 → 设置 → 开发者 打开「启用本地 HTTP API」\n\
             [summarizer]       （之后的调用仍会实时重连，API 开启后无需重启插件）"
        ),
    }

    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                let c = client.clone();
                // 每个连接一个线程：请求量极小，用标准库线程足够，还能省掉 async 运行时。
                std::thread::spawn(move || {
                    let _ = handle_conn(s, &c);
                });
            }
            Err(e) => eprintln!("[summarizer] 接受连接失败: {e}"),
        }
    }
    ExitCode::SUCCESS
}

/// 处理一个 HTTP 连接：读请求行 + 头 + body，路由后写响应。
fn handle_conn(mut s: TcpStream, client: &StickyClient) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    s.set_write_timeout(Some(Duration::from_secs(30)))?;

    let mut reader = BufReader::new(s.try_clone()?);

    // ---- 请求行
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Ok(());
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let raw_path = parts.next().unwrap_or("/").to_string();

    // ---- 头
    let mut content_length = 0usize;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h)? == 0 {
            break;
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
    }

    // ---- body
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }
    let body_str = String::from_utf8_lossy(&body).to_string();

    let path = raw_path.split('?').next().unwrap_or("/").to_string();

    // ---- 路由
    match (method.as_str(), path.as_str()) {
        ("GET", "/") | ("GET", "/health") | ("GET", "/sticky") => {
            let st = client.health();
            let payload = serde_json::json!({
                "ok": true,
                "plugin": "sticky-summarizer",
                "runtime": "rust",
                "version": env!("CARGO_PKG_VERSION"),
                "appReachable": st.is_some(),
                "app": st,
            });
            write_json(&mut s, 200, &payload)
        }
        ("POST", "/sticky") => {
            let parsed: serde_json::Value =
                serde_json::from_str(&body_str).unwrap_or(serde_json::Value::Null);

            // 形式一：事件推送 {"name": "note.created", "payload": {...}}
            if parsed.get("name").is_some() && parsed.get("command").is_none() {
                // 演示实现：仅记录。真实场景可做自动归档、摘要推送等。
                write_json(&mut s, 200, &serde_json::json!({ "ok": true }))
            } else {
                // 形式二：命令调用 {"command": "summary.notes", "args": {...}}
                let command = parsed.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let args = parsed
                    .get("args")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));

                match run_command(client, command, &args) {
                    Ok(result) => write_json(
                        &mut s,
                        200,
                        &serde_json::json!({ "ok": true, "data": result }),
                    ),
                    Err(e) => write_json(
                        &mut s,
                        400,
                        &serde_json::json!({ "ok": false, "error": e }),
                    ),
                }
            }
        }
        _ => write_json(
            &mut s,
            404,
            &serde_json::json!({ "ok": false, "error": "not found" }),
        ),
    }
}

/// 命令分发
fn run_command(
    client: &StickyClient,
    command: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    match command {
        "summary.notes" => cmd_summary_notes(client, args),
        "summary.todos" => cmd_summary_todos(client, args),
        "summary.stats" => cmd_summary_stats(client, args),
        other => Err(format!("未知命令: {other}")),
    }
}

// ------------------------------------------------------------------ 命令实现

/// 读取便签并按 args 过滤（与 Python 版行为一致）
fn fetch_notes(
    client: &StickyClient,
    args: &serde_json::Value,
) -> Result<Vec<serde_json::Value>, String> {
    let include_trashed = args
        .get("includeTrashed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut notes = client.list_notes(include_trashed)?;

    let ids: Vec<String> = str_list(args, "ids");
    let colors: Vec<String> = str_list(args, "colors");
    let tags: Vec<String> = str_list(args, "tags");

    if !ids.is_empty() {
        notes.retain(|n| {
            n.get("id")
                .and_then(|v| v.as_str())
                .map(|s| ids.iter().any(|i| i == s))
                .unwrap_or(false)
        });
    }
    if !colors.is_empty() {
        notes.retain(|n| {
            n.get("color")
                .and_then(|v| v.as_str())
                .map(|s| colors.iter().any(|c| c == s))
                .unwrap_or(false)
        });
    }
    if !tags.is_empty() {
        notes.retain(|n| {
            let content = n.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let found = extract_tags(content);
            tags.iter().any(|t| found.iter().any(|f| f == t))
        });
    }
    Ok(notes)
}

fn str_list(args: &serde_json::Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn cmd_summary_notes(
    client: &StickyClient,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let notes = fetch_notes(client, args)?;

    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("便签汇总 · {}", now_local().format("%Y-%m-%d")));

    let folder = args
        .get("folder")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("便签汇总")
        .to_string();

    let group_by = args
        .get("groupBy")
        .and_then(|v| v.as_str())
        .unwrap_or("none")
        .to_string();

    let body = build_summary(&notes, &title, &group_by);

    let doc = create_doc(client, &title, &folder, &body, &notes)?;

    let open_in_app = args
        .get("openInApp")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if open_in_app {
        if let Some(id) = doc.get("id").and_then(|v| v.as_str()) {
            let _ = client.execute("doc.read", &serde_json::json!({ "id": id }));
        }
    }

    Ok(serde_json::json!({
        "doc": doc,
        "count": notes.len(),
        "folder": folder,
    }))
}

fn cmd_summary_todos(
    client: &StickyClient,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let notes = fetch_notes(client, args)?;
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("便签待办清单")
        .to_string();
    let folder = args
        .get("folder")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("便签汇总")
        .to_string();

    let (open, done) = extract_todos(&notes);
    let body = build_todos_doc(&open, &done, &title);
    let doc = create_doc(client, &title, &folder, &body, &notes)?;

    Ok(serde_json::json!({
        "doc": doc,
        "open": open.len(),
        "done": done.len(),
    }))
}

fn cmd_summary_stats(
    client: &StickyClient,
    args: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let notes = fetch_notes(client, args)?;
    let days = args.get("days").and_then(|v| v.as_i64()).unwrap_or(30);
    let title = args
        .get("title")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("便签统计")
        .to_string();
    let folder = args
        .get("folder")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("便签汇总")
        .to_string();

    let body = build_stats_doc(&notes, days, &title);
    let doc = create_doc(client, &title, &folder, &body, &notes)?;

    Ok(serde_json::json!({
        "doc": doc,
        "count": notes.len(),
        "days": days,
    }))
}

/// 通过宿主的 `doc.create` 写入笔记，并把 frontmatter 一并交回宿主
///
/// 宿主契约：`doc.create` 接收 **`path`**（相对笔记库的路径，如 `便签汇总/标题.md`），
/// 而不是 `folder`。路径分隔符统一用 `/`。
fn create_doc(
    client: &StickyClient,
    title: &str,
    folder: &str,
    content: &str,
    notes: &[serde_json::Value],
) -> Result<serde_json::Value, String> {
    let fm = serde_json::json!({
        "generatedAt": now_local().format("%Y-%m-%dT%H:%M:%S").to_string(),
        "source": "sticky-summarizer",
        "noteCount": notes.len(),
        "tags": ["便签汇总", "自动生成"],
    });

    // 组装 path：folder/title.md，并清洗掉文件名里非法的字符
    let safe_title = sanitize_filename(title);
    let path = if folder.trim().is_empty() {
        format!("{safe_title}.md")
    } else {
        format!("{}/{}.md", folder.trim().trim_end_matches('/'), safe_title)
    };

    client.execute(
        "doc.create",
        &serde_json::json!({
            "path": path,
            "content": content,
            "frontmatter": fm,
        }),
    )
}

/// 清洗成安全的文件名（去掉 Windows 非法字符）
fn sanitize_filename(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "便签汇总".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

// ------------------------------------------------------------------ HTTP 输出

fn write_json(s: &mut TcpStream, code: u16, obj: &serde_json::Value) -> std::io::Result<()> {
    let body = serde_json::to_string(obj).unwrap_or_else(|_| "{}".into());
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let head = format!(
        "HTTP/1.1 {code} {reason}\r\n\
         Content-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        body.as_bytes().len()
    );
    s.write_all(head.as_bytes())?;
    s.write_all(body.as_bytes())?;
    s.flush()
}

// ------------------------------------------------------------------ 小工具

/// 颜色 key → 中文标签
fn color_label(key: &str) -> String {
    match key {
        "yellow" => "黄色".into(),
        "green" => "绿色".into(),
        "pink" => "粉色".into(),
        "purple" => "紫色".into(),
        "blue" => "蓝色".into(),
        "gray" => "灰色".into(),
        other => other.to_string(),
    }
}

/// 便签标题：取首个非空行
fn title_of(note: &serde_json::Value) -> String {
    let content = note.get("content").and_then(|v| v.as_str()).unwrap_or("");
    for line in content.lines() {
        if !line.trim().is_empty() {
            return line.trim().chars().take(80).collect();
        }
    }
    String::new()
}

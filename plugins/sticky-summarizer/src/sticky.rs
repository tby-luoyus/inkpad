//! 与便签应用通信的极简 HTTP 客户端。
//!
//! 刻意手写而不是用 reqwest：插件要尽量轻、启动要快，
//! 而且只需要「发一个 JSON 请求、收一个 JSON 响应」这一种能力。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Clone)]
pub struct StickyClient {
    /// 形如 http://127.0.0.1:43919
    #[allow(dead_code)] // 保留给日志/调试用
    base: String,
    host: String,
    port: u16,
    token: String,
}

impl StickyClient {
    pub fn new(base: &str) -> Self {
        let base = base.trim_end_matches('/').to_string();
        // 从 URL 里剥出 host:port，默认 43919
        let without_scheme = base
            .strip_prefix("http://")
            .or_else(|| base.strip_prefix("https://"))
            .unwrap_or(&base);
        let hostport = without_scheme.split('/').next().unwrap_or("127.0.0.1:43919");
        let (host, port) = match hostport.split_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().unwrap_or(43919)),
            None => (hostport.to_string(), 43919),
        };

        // 令牌：与应用约定存在数据目录的 api.token。
        // 不带令牌时，本机回环请求宿主会直接放行（便于调试），所以这里尽力读取即可。
        let token = std::env::var("STICKY_API_TOKEN").unwrap_or_default();

        Self {
            base: format!("http://{host}:{port}"),
            host,
            port,
            token,
        }
    }

    /// 健康检查（失败返回 None，不抛错）
    pub fn health(&self) -> Option<HealthInfo> {
        let resp = self.request("GET", "/api/v1/health", None, 3).ok()?;
        let v: serde_json::Value = serde_json::from_str(&resp).ok()?;
        if v.get("ok").and_then(|b| b.as_bool()) == Some(true) {
            Some(HealthInfo {
                version: v
                    .get("version")
                    .and_then(|s| s.as_str())
                    .unwrap_or("?")
                    .to_string(),
            })
        } else {
            None
        }
    }

    /// 调用宿主的统一命令入口 POST /api/v1/execute
    pub fn execute(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let payload = serde_json::json!({ "command": command, "args": args });
        let body = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
        let resp = self
            .request("POST", "/api/v1/execute", Some(&body), 20)
            .map_err(|e| format!("调用宿主命令 {command} 失败: {e}"))?;

        let v: serde_json::Value =
            serde_json::from_str(&resp).map_err(|e| format!("解析宿主响应失败: {e}"))?;

        if v.get("ok").and_then(|b| b.as_bool()) != Some(true) {
            let msg = v
                .get("error")
                .and_then(|s| s.as_str())
                .unwrap_or("未知错误");
            return Err(format!("宿主拒绝 {command}: {msg}"));
        }
        // 剥掉 {"ok":true,"data":…} 外壳
        Ok(v.get("data").cloned().unwrap_or(serde_json::Value::Null))
    }

    /// 列出便签（走 notes.list，行为与界面完全一致）
    pub fn list_notes(&self, include_deleted: bool) -> Result<Vec<serde_json::Value>, String> {
        let data = self.execute(
            "notes.list",
            &serde_json::json!({ "includeDeleted": include_deleted }),
        )?;
        // data 可能是数组，也可能是 {"notes":[...]}
        if let Some(arr) = data.as_array() {
            return Ok(arr.clone());
        }
        if let Some(arr) = data.get("notes").and_then(|v| v.as_array()) {
            return Ok(arr.clone());
        }
        Ok(Vec::new())
    }

    /// 发一个裸 HTTP/1.1 请求，返回响应 body 字符串
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        timeout_secs: u64,
    ) -> Result<String, String> {
        let addr = (self.host.as_str(), self.port)
            .to_socket_addrs()
            .map_err(|e| e.to_string())?
            .next()
            .ok_or_else(|| "无法解析宿主地址".to_string())?;

        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(timeout_secs))
            .map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(timeout_secs)))
            .ok();
        stream
            .set_write_timeout(Some(Duration::from_secs(timeout_secs)))
            .ok();

        let body_bytes = body.unwrap_or("").as_bytes();
        let mut req = format!(
            "{method} {path} HTTP/1.1\r\n\
             Host: {}:{}\r\n\
             Accept: application/json\r\n\
             Connection: close\r\n",
            self.host, self.port
        );
        if body.is_some() {
            req.push_str("Content-Type: application/json\r\n");
            req.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));
        }
        if !self.token.is_empty() {
            req.push_str(&format!("X-Api-Token: {}\r\n", self.token));
        }
        req.push_str("\r\n");

        stream
            .write_all(req.as_bytes())
            .and_then(|_| stream.write_all(body_bytes))
            .and_then(|_| stream.flush())
            .map_err(|e| e.to_string())?;

        // 读响应：跳过头，取 body
        let mut reader = BufReader::new(stream);
        let mut status_line = String::new();
        reader
            .read_line(&mut status_line)
            .map_err(|e| e.to_string())?;

        let mut content_length: Option<usize> = None;
        let mut chunked = false;
        loop {
            let mut h = String::new();
            if reader.read_line(&mut h).map_err(|e| e.to_string())? == 0 {
                break;
            }
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                let k = k.to_ascii_lowercase();
                if k == "content-length" {
                    content_length = v.trim().parse().ok();
                } else if k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked") {
                    chunked = true;
                }
            }
        }

        let mut raw = Vec::new();
        if chunked {
            // 极简 chunked 解码
            loop {
                let mut size_line = String::new();
                if reader
                    .read_line(&mut size_line)
                    .map_err(|e| e.to_string())?
                    == 0
                {
                    break;
                }
                let size = usize::from_str_radix(size_line.trim().split(';').next().unwrap_or("0"), 16)
                    .unwrap_or(0);
                if size == 0 {
                    break;
                }
                let mut buf = vec![0u8; size];
                reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
                raw.extend_from_slice(&buf);
                let mut crlf = [0u8; 2];
                let _ = reader.read_exact(&mut crlf);
            }
        } else if let Some(n) = content_length {
            let mut buf = vec![0u8; n];
            reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
            raw = buf;
        } else {
            reader.read_to_end(&mut raw).map_err(|e| e.to_string())?;
        }

        String::from_utf8(raw).map_err(|e| e.to_string())
    }
}

pub struct HealthInfo {
    pub version: String,
}

impl serde::Serialize for HealthInfo {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("HealthInfo", 1)?;
        st.serialize_field("version", &self.version)?;
        st.end()
    }
}

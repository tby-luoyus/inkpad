# 便签插件开发

插件让便签具备无限可能：同步到笔记库、接入 LLM、联动其他应用……
插件通过 **HTTP** 与便签通信，因此**可以用任意语言编写**。

## 目录结构

```
<数据目录>/plugins/
└── my-plugin/
    ├── plugin.json      # 必需：插件清单
    ├── index.js         # 可选：纯前端脚本
    └── server.py        # 可选：你自己的后端
```

数据目录位置：在应用「设置 → 数据」点「打开」，或看 README 的路径表。

## plugin.json

```json
{
  "id": "my-plugin",
  "name": "我的插件",
  "version": "1.0.0",
  "author": "你",
  "description": "一句话说明这个插件做什么",
  "permissions": ["notes.read", "notes.write", "events.subscribe"],
  "events": ["note.*"],
  "commands": [
    { "name": "my.doSomething", "description": "做什么", "params": { "id": "string" } }
  ],
  "endpoint": "http://127.0.0.1:8080/sticky",
  "frontend": null,
  "enabled": true,
  "autoStart": {
    "command": "python",
    "args": ["server.py"],
    "cwd": "."
  }
}
```

| 字段 | 说明 |
| --- | --- |
| `id` | 唯一标识，建议 kebab-case |
| `events` | 订阅的事件名，支持前缀通配 `note.*`，或 `*` 表示全部 |
| `commands` | 注册到命令总线的命令，外部可通过 `my.doSomething` 调用 |
| `endpoint` | 事件与命令都 POST 到这里；不填则为纯前端插件 |
| `frontend` | 相对插件目录的 JS 路径，模块导出 `activate(bridge, manifest)` |
| `enabled` | 是否启用（可在应用内切换，会回写此文件） |
| `autoStart` | 可选：应用启动时自动拉起的后端进程 |

### autoStart（自动拉起后端）

配置后，便签会在启动时（以及你在「拓展 → 插件」点「启动」时）自动运行你的后端，
并跟踪该进程，应用退出时一并结束：

| 字段 | 说明 |
| --- | --- |
| `command` | 可执行程序。可以是 PATH 里的命令（`python`、`node`），也可以是插件目录下的相对路径（`bin/my-plugin.exe`） |
| `args` | 参数数组 |
| `cwd` | 相对插件目录的工作目录，默认插件目录本身 |

> 相对路径的 `command` 会被解析到插件目录下，宿主会检查文件是否存在并给出日志提示。

启动前，宿主会先探测 `endpoint` 的 `/health`：**若已有实例在运行则复用、不重复拉起**，
因此即使上次异常退出留下了进程，也不会因为端口冲突而启动失败。
「拓展 → 插件」页可查看进程状态（运行中 / 未运行），并手动启动、停止、探测。

> 后端建议实现 `GET /health` 返回 `{"ok": true}`，宿主用它做存活探测；
> `/sticky` 上的 GET/POST 才是事件与命令入口。


## 接收事件

便签发改变时，会向 `endpoint` 发送：

```http
POST /sticky
X-Plugin-Id: my-plugin
Content-Type: application/json

{
  "name": "note.created",
  "payload": { "id": "…", "title": "买牛奶", "content": "…", "color": "yellow", "pinned": false },
  "at": "2026-10-11T08:42:21+00:00",
  "source": "ui"
}
```

**Python 示例**

```python
from http.server import BaseHTTPRequestHandler, HTTPServer
import json, requests

BASE = "http://127.0.0.1:43919/api/v1"
TOKEN = open(r"%APPDATA%/com.stickynotes.desktop/api.token").read().strip()
H = {"X-Api-Token": TOKEN, "Content-Type": "application/json"}

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        body = json.loads(self.rfile.read(n) or "{}")

        # 事件推送
        if "name" in body:
            if body["name"] == "note.created":
                note = body["payload"]
                if "#待办" in note["content"]:
                    # 打上粉色标记
                    requests.post(f"{BASE}/notes/{note['id']}/color",
                                  headers=H, json={"color": "pink"})

        # 命令调用
        elif "command" in body:
            if body["command"] == "my.doSomething":
                # 用 Agent 追加一行处理结果
                requests.post(f"{BASE}/notes/{body['args']['id']}/append",
                              headers=H, json={"text": "\n✅ 已处理"})

        self.send_response(200)
        self.end_headers()
        self.wfile.write(b'{"ok":true}')

HTTPServer(("127.0.0.1", 8080), Handler).serve_forever()
```

## 处理命令

外部（UI / Agent / 脚本）调用 `my.doSomething` 时，便签会把请求转发到
`endpoint`，请求体为：

```json
{ "command": "my.doSomething", "args": { "id": "…" } }
```

返回内容会透传给调用方。

## 纯前端插件

不想起后端？用 `frontend` 字段挂一个前端模块：

```js
// plugins/my-plugin/index.js
export function activate(bridge, manifest) {
  console.log('插件已加载:', manifest.name);

  // bridge 就是 window.__sticky
  bridge.on('note.created', async (note) => {
    if (note.content.includes('#置顶')) {
      await bridge.execute('notes.pin', { id: note.id, pinned: true });
    }
  });

  // 也可以挂自己的工具函数供控制台/其他插件使用
  bridge.exportAll = () => bridge.notes.list({ includeDeleted: true });
}
```

## 完整可用命令

在应用「拓展 → Agent 命令」面板查看带参数说明的完整列表。常用命令速查：

**便签**

| 命令 | 作用 |
| --- | --- |
| `notes.list` | 列出便签（`includeDeleted` 含回收站） |
| `notes.create` | 新建便签 |
| `notes.update` / `notes.append` | 覆盖 / 追加内容 |
| `notes.search` | 全文搜索 |
| `notes.setColor` / `notes.setPinned` | 改色 / 置顶 |
| `notes.trash` / `notes.restore` / `notes.purge` | 删除 / 恢复 / 彻底删除 |

**笔记库**（磁盘 `.md` 为权威源）

| 命令 | 作用 |
| --- | --- |
| `doc.list` | 列出笔记（可按 `folder` / `tag` 过滤） |
| `doc.tree` | 获取笔记目录树 |
| `doc.info` | 笔记库统计（总数 / 文件夹 / 标签 / 孤立笔记） |
| `doc.tags` | 全部标签及其出现次数 |
| `doc.get` / `doc.search` | 读取单篇 / 全文搜索 |
| `doc.create` | 新建笔记（`path` / `content` / `frontmatter`） |
| `doc.save` / `doc.rename` / `doc.delete` | 保存 / 重命名 / 删除 |
| `doc.backlinks` / `doc.outlinks` | 反向链接 / 出链 |
| `doc.reindex` | 从磁盘重建索引 |
| `doc.fromNotes` | **把便签汇总成一篇笔记**（内置实现） |

**插件**

| 命令 | 作用 |
| --- | --- |
| `plugin.startProcess` / `plugin.stopProcess` | 启动 / 停止插件的 autoStart 进程 |

### 参考实现：便签总结生成笔记

`plugins/sticky-summarizer/` 是一个完整可用的示例（**Rust 实现**，编译成单个 exe，零运行时依赖）：

- 订阅 `note.*` / `doc.*` 事件
- 注册 `summary.notes` / `summary.todos` / `summary.stats` 三个命令
- 通过 `autoStart` 随应用自动启动
- 调用宿主的 `notes.list` 读取便签、`doc.create` 写回笔记

想看一个「读数据 → 生成 Markdown → 写回」的完整闭环，直接读它的 `src/`：
`main.rs`（HTTP 服务与路由）、`sticky.rs`（宿主客户端）、`md.rs`（Markdown 生成）。

## 运行前提

插件后端通过**本机 HTTP API**（`http://127.0.0.1:43919/api/v1`）读写便签与笔记。
该接口**默认开启**，只监听回环地址，不对局域网暴露，所以插件开箱即用。

若你手动关掉了它（「设置 → 开发者 → 启用本地 HTTP API」），插件会连不上宿主，
日志里会出现类似提示：

```
[xxx] 警告：无法连接 http://127.0.0.1:43919
      请在 便签 → 设置 → 开发者 打开「启用本地 HTTP API」
```

## 排查插件

插件后端的 stdout / stderr 会落到 `<数据目录>/plugin-logs/<插件id>.log`
（`%APPDATA%/com.stickynotes.desktop/plugin-logs/`）。

- 界面上：「拓展 → 插件」卡片点「查看日志」直接看最后 200 行
- 命令行：直接打开这个目录

启动失败、端口冲突、依赖缺失等问题都会在这里留下痕迹。

## 安全提示

- 插件的 `permissions` 目前仅用于**展示**，让用户知道插件声明了什么。
  沙箱与强制校验是后续计划。
- 只安装你信任的插件。插件后端能通过 HTTP API 读写你的全部便签与笔记。
- 本地 HTTP API 只监听 `127.0.0.1`，不对局域网暴露。

# 便签 · Sticky Notes

现代桌面便签应用。**主窗口统一管理**所有便签，每张便签可**独立贴到桌面**。
基础套件对齐 Windows 便笺（Sticky Notes），并预留了四类拓展接口，便于后续接入
AI Agent、多 App 联动、自定义主题与云同步。

技术栈：**Tauri 2**（Rust 后端）+ **React 18 + TypeScript**（前端）+ **SQLite**（本地存储）。

---

## 特性

### 基础套件（对齐 Windows 便笺）

| 能力 | 说明 |
| --- | --- |
| 独立便签窗口 | 无边框、可拖动、可缩放，位置与尺寸自动记忆 |
| 六色配色 | 黄 / 绿 / 粉 / 紫 / 蓝 / 灰，颜色轮盘选择器 |
| 即时编辑 + 自动保存 | 输入防抖 380ms 落盘，关闭前强制保存 |
| 主窗口管理面板 | 网格 / 列表双视图，毛玻璃质感卡片 |
| 搜索 | 全文搜索便签内容，支持按颜色筛选、多种排序 |
| 置顶 | 单张置顶（窗口层级 + 列表排序），支持批量 |
| 回收站 | 软删除、恢复、清空，超期自动清理 |
| 批量操作 | Ctrl / Shift 多选，批量换色、置顶、删除、**汇总成笔记** |
| 系统托盘 | 快速新建、显示面板、隐藏全部便签 |
| 深色模式 | 跟随系统 / 强制亮色 / 强制暗色 |

### 笔记库（Obsidian 风格）

主窗口的「笔记」页签是一个三栏笔记工作台，磁盘上的 `.md` 文件是唯一权威数据源，
所以你的笔记可以同时用 **Obsidian / VS Code / Typora** 打开，互不锁定。

| 能力 | 说明 |
| --- | --- |
| 三栏布局 | 文件树（248px）· 编辑器（弹性）· 右侧栏（264px） |
| 编辑模式 | 编辑 / 分栏 / 阅读三档切换 |
| 双向链接 | `[[笔记名]]` 语法，自动计算**反向链接**与**出链** |
| 标签 | `#标签` 自动索引，左侧标签云过滤，支持中文标签 |
| YAML 属性 | `---` frontmatter 可视化编辑与解析 |
| Markdown 渲染 | 标题、列表、任务列表、表格、引用、提示块（`> [!note]`）、代码高亮 |
| 自动保存 | 输入防抖 700ms 落盘；`Ctrl+S` 立即保存 |
| 全文搜索 | 跨文件名与正文检索，命中高亮 |
| 重索引 | 一键从磁盘重建索引（外部改动后同步） |
| 自动完成 | 输入 `[[` 弹出笔记名补全 |

### 拓展接口（四类）

| 接口 | 用途 | 入口 |
| --- | --- | --- |
| **AI / Agent** | 统一命令总线，Agent 可读写便签与笔记 | `ext.execute()` / `POST /api/v1/execute` |
| **多 App 联动** | 本地 HTTP API + SSE 事件流 + Webhook 推送 | `http://127.0.0.1:43919/api/v1` |
| **主题与卡片** | 主题包覆盖配色 / CSS，可挂自定义渲染器 | `themes/<id>/theme.json` |
| **插件系统** | 插件订阅事件、注册命令、**可自动拉起进程** | `plugins/<id>/plugin.json` |
| **备份同步** | JSON 全量导入导出 | `backup.*` |

**内置首个插件：便签总结生成笔记**（`plugins/sticky-summarizer`）

把选中的便签（或全部便签）自动整理成结构化 Markdown 笔记，写入笔记库。
支持按颜色 / 标签分组、提取待办、生成统计。

插件后端用 **Rust 实现**，编译成单个 exe（约 550 KB）**嵌入主程序**，
首次启动自动释放 —— 不需要目标机器安装 Python 或任何运行时，双击即用。
启动应用时自动拉起进程（`autoStart`）。

| 命令 | 作用 |
| --- | --- |
| `summary.notes` | 汇总便签 → 笔记，可按 `ids` / `colors` / 分组方式定制 |
| `summary.todos` | 提取所有便签里的 `- [ ]` 待办，生成待办清单笔记 |
| `summary.stats` | 统计便签分布（颜色 / 字数 / 活跃度），生成统计笔记 |

---

## 快速开始

### 环境要求

- Node.js ≥ 18
- Rust ≥ 1.77（含 MSVC 工具链）
- Windows 上需要 WebView2 Runtime（Win11 自带）

### 开发

```bash
npm install
npm run app:dev      # 启动开发模式（热重载）
```

### 打包

```bash
npm run app:build    # 产物在 src-tauri/target/release/bundle/
```

---

## 架构

```
src-tauri/src/
├── lib.rs          入口：状态装配、托盘、生命周期
├── main.rs         可执行入口
├── store.rs        SQLite 持久化（notes / settings 表）
├── vault.rs        笔记库：磁盘 .md 为权威源，SQLite 仅作索引
├── service.rs      业务逻辑 + 统一命令执行器（UI 与 HTTP 共用）
├── commands.rs     Tauri 命令（前端 IPC 契约）
├── api.rs          本地 HTTP API（多 App 联动）
├── hub.rs          拓展中枢：事件总线 / 命令注册表 / 插件 / 主题 / 后台运行时
├── window.rs       多窗口管理
└── models.rs       数据模型

src/
├── main.tsx / note-main.tsx     两个窗口入口
├── components/                  界面组件（含 NotesWorkspace 笔记工作台）
├── lib/api.ts                   类型化 IPC 封装（唯一出入口）
├── lib/markdown.ts              轻量 Markdown 渲染器（支持 wikilink / 标签 / 提示块）
├── lib/bridge.ts                window.__sticky 桥接层
└── plugins/sticky-summarizer/   内置插件（Rust 工程，产物嵌入主程序，首次运行释放）
    ├── src/                     main.rs（HTTP 服务）/ sticky.rs（宿主客户端）/ md.rs（生成逻辑）
    └── bin/                     运行时释放位置
```

> 构建顺序：插件是独立 crate，**必须先于主程序构建**（主程序用 `include_bytes!` 嵌入其产物）。
> `npm run app:build` / `npm run app:dev` 已自动包含这一步；单独构建插件用 `npm run plugin:build`。

**设计要点**

1. **单一命令总线**：UI、HTTP、插件、Agent 全部汇流到
   `service::execute_command`，行为完全一致，不会出现「界面能做、API 不能做」。
2. **事件驱动**：任何数据变更都广播到事件总线，前端订阅刷新，
   同时推送给 Webhook 与插件后端。
3. **磁盘优先的笔记库**：笔记就是普通 `.md` 文件，SQLite 只存索引；
   重新索引可从磁盘完整重建，不与外部编辑器抢所有权。
4. **契约优先**：插件用 JSON 声明能力，通过 HTTP 通信，
   因此插件可以用**任意语言**编写，并可被自动拉起进程。
5. **线程安全的事件派发**：事件派发可能发生在任意线程（Tauri 命令、HTTP 线程池、
   插件管理线程），统一投递到常驻后台运行时，避免「no reactor running」崩溃。

> ⚠️ **Tauri 状态注册陷阱**：`app.manage(Arc<T>)` 注册的是 `Arc<T>`，
> 命令必须声明 `State<'_, Arc<T>>`。Tauri v2 按 `TypeId` 匹配状态，
> 写成 `State<'_, T>` 会导致**所有** IPC 命令静默失败（表现为界面永远加载中）。


---

## 拓展开发

### 1. Agent / 脚本接入

统一入口，所有能力都在这一个接口里：

```js
// 前端 / 插件（浏览器环境）
await window.__sticky.execute('notes.create', {
  content: '由 Agent 创建',
  color: 'blue',
  open: true,
});

await window.__sticky.execute('notes.append', {
  id: '<noteId>',
  text: '\n— 追加于 ' + new Date().toLocaleString(),
});
```

```bash
# 任意语言 / 脚本
curl -X POST http://127.0.0.1:43919/api/v1/execute \
  -H "Content-Type: application/json" \
  -d '{"command":"notes.create","args":{"content":"来自 curl"}}'
```

**可用命令**：在应用「拓展 → Agent 命令」面板中查看完整列表（含参数说明），
或在 `src-tauri/src/hub.rs` 的 `register_core_specs()` 中查看定义。

核心命令一览：

| 命令 | 说明 |
| --- | --- |
| `notes.list` / `notes.search` / `notes.get` | 查询 |
| `notes.create` / `notes.update` / `notes.replace` / `notes.append` | 写入 |
| `notes.trash` / `notes.restore` / `notes.purge` | 删除 |
| `notes.pin` / `notes.setColor` / `notes.duplicate` | 属性 |
| `window.openNote` / `window.closeNote` / `window.openMain` | 窗口 |
| `app.settings` / `app.updateSettings` / `app.stats` | 应用 |
| `app.export` / `app.import` | 数据 |
| `theme.list` / `theme.set` / `theme.reload` | 主题 |
| `plugin.list` / `plugin.enable` / `plugin.reload` | 插件 |
| `events.recent` / `events.emit` | 事件 |
| `webhook.list` / `webhook.upsert` / `webhook.remove` | Webhook |
| `api.status` / `api.enable` | 本地接口 |

> 想要一个「让 AI 整理便签」的能力？注册 `agent.notes.assist` 命令即可 —— 便签窗口的
> 「AI 助手」面板已经在调用它，接上你的 LLM 服务就能用。

### 2. 本地 HTTP API

**默认开启。** 仅监听 `127.0.0.1`，不对局域网暴露，因此默认开启是安全的 ——
插件、Agent、多 App 联动都依赖这条本地命令总线，开箱即用比让用户先去翻设置更重要。
如需关闭，到「设置 → 开发者 → 启用本地 HTTP API」关掉即可。

> 如果你在浏览器里访问 `http://127.0.0.1:43919/api/v1/health` 看到「拒绝连接」，
> 那就是这个开关被关掉了（或应用没在运行）。

访问令牌存于数据目录的 `api.token`。带 `X-Api-Token` 头时会校验；
不带则按本机回环请求放行（便于快速调试）。

```
GET    /api/v1/health              健康检查
GET    /api/v1/capabilities        全部能力清单
GET    /api/v1/commands            命令列表
GET    /api/v1/notes               列出便签（?q= &sort= &colors=）
POST   /api/v1/notes               新建便签
GET    /api/v1/notes/:id           获取便签
PATCH  /api/v1/notes/:id           更新便签
DELETE /api/v1/notes/:id           移入回收站
POST   /api/v1/notes/:id/content   替换全文
POST   /api/v1/notes/:id/append    追加内容
POST   /api/v1/notes/:id/pin       置顶
POST   /api/v1/notes/:id/color     改颜色
POST   /api/v1/notes/:id/open      打开窗口
POST   /api/v1/notes/:id/close     关闭窗口
GET    /api/v1/search?q=           搜索
GET    /api/v1/trash               回收站列表
DELETE /api/v1/trash               清空回收站
POST   /api/v1/execute             通用命令入口
POST   /api/v1/backup/export       导出
POST   /api/v1/backup/import       导入
GET    /api/v1/settings            读取设置
PATCH  /api/v1/settings            更新设置
GET    /api/v1/themes              主题列表
GET    /api/v1/events              最近事件
GET    /api/v1/events/subscribe    SSE 实时事件流
```

**响应格式**

```json
{ "ok": true,  "data": { } }
{ "ok": false, "error": "错误说明" }
```

**Python 示例**

```python
import requests

BASE = "http://127.0.0.1:43919/api/v1"

# 新建
note = requests.post(f"{BASE}/notes", json={"content": "买牛奶"}).json()["data"]

# 追加（Agent 常用）
requests.post(f"{BASE}/notes/{note['id']}/append", json={"text": "还有鸡蛋"})

# 搜索
print(requests.get(f"{BASE}/search", params={"q": "牛奶"}).json()["data"])
```

**订阅实时事件**

```python
import requests, json

with requests.get(f"{BASE}/events/subscribe", stream=True) as r:
    for line in r.iter_lines():
        if line and line.startswith(b"data:"):
            print(json.loads(line[5:]))
```

### 3. Webhook

在「拓展 → App 联动」中配置，或通过命令：

```
webhook.upsert  { "config": {
  "id": "wh-1", "name": "我的服务",
  "url": "https://example.com/hook",
  "events": ["note.*"],           // 支持前缀通配，* 为全部
  "enabled": true, "headers": {}
}}
```

事件以 `POST JSON` 发送：

```json
{
  "name": "note.created",
  "payload": { "id": "...", "title": "买牛奶", "content": "...", "color": "yellow" },
  "at": "2026-10-11T08:30:00+00:00",
  "source": "ui"
}
```

**事件清单**：`note.created` `note.updated` `note.trashed` `note.restored`
`note.purged` `note.pinnedChanged` `note.colorChanged` `note.hiddenChanged`
`note.focused` `note.closed` `trash.emptied` `settings.changed` `theme.changed`
`plugin.toggled` `data.imported` `api.started` `app.ready`

### 4. 插件

在数据目录的 `plugins/<id>/plugin.json`：

```json
{
  "id": "my-plugin",
  "name": "我的插件",
  "version": "1.0.0",
  "description": "把便签同步到我的笔记库",
  "permissions": ["notes.read", "notes.write", "events.subscribe"],
  "events": ["note.created", "note.updated"],
  "commands": [
    { "name": "my.sync", "description": "立即同步", "params": { "force": "bool?" } }
  ],
  "endpoint": "http://127.0.0.1:8080/sticky",
  "frontend": "index.js",
  "enabled": true
}
```

- **`endpoint`**：事件与命令都会 `POST` 到这里。命令请求体为 `{ "command", "args" }`。
- **`frontend`**：可选。纯前端插件，模块导出一个 `activate(bridge, manifest)` 函数，
  拿到 `window.__sticky` 同款 bridge 直接操作便签。

```js
// plugins/my-plugin/index.js
export function activate(bridge, manifest) {
  console.log('已加载', manifest.name);

  // 监听事件
  bridge.on('note.created', async (note) => {
    if (note.content.includes('#待办')) {
      await bridge.execute('notes.setColor', { id: note.id, color: 'pink' });
    }
  });

  // 注册 UI 可用能力
  bridge.myHelper = () => bridge.notes.list({});
}
```

用任意语言写后端都行 —— Node / Python / Go，只要能收 HTTP。

### 5. 主题包

在数据目录的 `themes/<id>/theme.json`：

```json
{
  "id": "my-theme",
  "name": "我的主题",
  "appearance": "light",
  "colors": {
    "yellow": "#FFF3C4", "green": "#DCEFD0",
    "pink": "#FBD9E4",   "purple": "#E7DFF7",
    "blue": "#D3E8F7",   "gray": "#EAEAEA"
  },
  "cssVars": {
    "--sn-radius": "14px",
    "--sn-note-shadow": "0 1px 2px rgba(0,0,0,.08), 0 8px 24px rgba(0,0,0,.12)"
  },
  "cssFile": "theme.css",
  "renderer": null
}
```

**可覆盖的 CSS 变量**（定义在 `src/styles/tokens.css`）：
`--sn-yellow` `--sn-green` `--sn-pink` `--sn-purple` `--sn-blue` `--sn-gray`、
`--sn-app-bg` `--sn-surface` `--sn-border` `--sn-text` `--sn-accent`、
`--sn-radius` `--sn-shadow-note` `--sn-font` 等。

放好后在「拓展 → 主题包」点「重新加载」。

---

## 数据位置

| 内容 | 路径 |
| --- | --- |
| 数据目录 | `%APPDATA%/com.stickynotes.desktop` |
| 数据库 | 上者 `notes.db` |
| 插件 | 上者 `plugins/` |
| 插件日志 | 上者 `plugin-logs/<插件id>.log` |
| 笔记库 | 上者 `vault/`（`.md` 文件 + `.sticky-vault.db` 索引） |
| 主题 | 上者 `themes/` |
| 备份 | 上者 `backups/` |
| 接口令牌 | 上者 `api.token` |

可用环境变量 `STICKY_NOTES_HOME` 覆盖数据目录（方便便携 / 测试）。

**排查插件问题**：插件后端的 stdout / stderr 会写入 `plugin-logs/<插件id>.log`。
在「拓展 → 插件」卡片上点「查看日志」即可直接看，也可从「设置 → 数据」打开日志目录。

---

## 快捷键

**主窗口**

| 快捷键 | 功能 |
| --- | --- |
| `Ctrl+N` | 新建便签 |
| `Ctrl+F` | 聚焦搜索 |
| `Ctrl+A` | 全选当前视图便签 |
| `Delete` | 删除选中 |
| `Esc` | 取消选择 / 清除搜索 |

**便签窗口**

| 快捷键 | 功能 |
| --- | --- |
| `Ctrl+N` | 新建便签 |
| `Ctrl+P` | 置顶 / 取消置顶 |
| `Ctrl+D` | 复制便签 |
| `Ctrl+Delete` | 删除便签 |
| `Esc` | 关闭便签窗口 |

---

## 许可

MIT

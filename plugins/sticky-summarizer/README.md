# 便签总结生成笔记 · sticky-summarizer

第一个官方插件，也是**真实可用**的生产级插件。

**v2 起后端用 Rust 实现**：编译成一个独立 exe，随主程序一起分发、首次启动自动释放。
不依赖 Python 或任何运行时环境，双击应用即可用。

把便签一键整理成结构化 Markdown 笔记，自动写入笔记库。

## 它做什么

| 命令 | 作用 |
| --- | --- |
| `summary.notes` | 把便签汇总成一篇笔记（概览表 + 待办速览 + 目录 + 正文） |
| `summary.todos` | 提取所有便签里的待办项（`- [ ]` / `#todo` / `TODO:`），生成清单 |
| `summary.stats` | 统计便签分布（颜色 / 字数 / 活跃度），生成数据笔记 |

汇总结果会带上 frontmatter（`source: sticky-summarizer`），方便日后批量筛选。

## 怎么运行

### 方式一：应用自动拉起（推荐，零配置）

`plugin.json` 里声明了：

```json
"autoStart": { "command": "bin/sticky-summarizer.exe", "args": [], "cwd": "." }
```

应用启动、或你在「拓展 → 插件」里启用本插件时，会自动运行这个 exe。
**不需要装 Python，也不需要配 PATH。**

### 方式二：手动启动

```bash
cd <数据目录>/plugins/sticky-summarizer
./bin/sticky-summarizer.exe --port 43110 --app http://127.0.0.1:43919
```

## 前置条件

插件通过便签应用的本地 HTTP API 读写数据。该接口**默认开启**（只监听 127.0.0.1），
所以正常情况下无需任何操作。若你手动关过它，到
**设置 → 开发者** 打开「启用本地 HTTP API」（默认端口 43919）。

若插件连不上宿主，日志里会出现：

```
[summarizer] 警告：无法连接 http://127.0.0.1:43919
[summarizer]       请在 便签 → 设置 → 开发者 打开「启用本地 HTTP API」
```

日志位置：`<数据目录>/plugin-logs/sticky-summarizer.log`，
也可在「拓展 → 插件」卡片点「查看日志」直接看。

## 调用示例

### 通过应用内的 Agent 面板

「拓展 → Agent 命令」里找到 `summary.notes`，直接填参数执行：

```json
{
  "ids": [],
  "colors": ["yellow", "blue"],
  "title": "本周便签汇总",
  "folder": "便签汇总",
  "groupBy": "color"
}
```

### 通过 HTTP

```bash
curl -X POST http://127.0.0.1:43919/api/v1/execute \
  -H "Content-Type: application/json" \
  -d '{
    "command": "summary.notes",
    "args": { "groupBy": "tag", "title": "按标签汇总" }
  }'
```

### 直接打插件端点

```bash
curl -X POST http://127.0.0.1:43110/sticky \
  -H "Content-Type: application/json" \
  -d '{ "command": "summary.notes", "args": { "title": "汇总" } }'
```

### 通过前端脚本 / 控制台

```js
await window.__sticky.execute('summary.notes', {
  colors: ['yellow'],
  title: '待办便签汇总',
});
```

## 参数说明

`summary.notes`

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `ids` | string[]? | 只汇总指定便签；留空表示全部 |
| `colors` | string[]? | 按颜色筛选 |
| `tags` | string[]? | 按标签筛选 |
| `includeTrashed` | bool? | 是否包含回收站内容，默认 false |
| `title` | string? | 笔记标题，默认带时间戳 |
| `folder` | string? | 写入笔记库的文件夹，默认 `便签汇总` |
| `groupBy` | string? | `none` / `color` / `tag` |

`summary.stats` 额外支持 `days`（统计最近多少天的活跃度）。

## 目录结构

```
sticky-summarizer/
├── plugin.json        插件清单（能力声明 + 自启动配置）
├── Cargo.toml         Rust 工程定义
├── src/
│   ├── main.rs        HTTP 服务 + 命令分发
│   ├── sticky.rs      宿主 HTTP 客户端
│   └── md.rs          Markdown 生成（三个命令的核心逻辑）
├── bin/               ← 运行时释放位置（应用首次启动时生成）
│   └── sticky-summarizer.exe
└── README.md          本文件
```

## 构建

插件是独立 crate，**必须先于主程序构建**（主程序用 `include_bytes!` 把它的产物嵌进去）：

```bash
# 项目根目录
npm run plugin:build      # = cargo build --release --manifest-path plugins/sticky-summarizer/Cargo.toml
```

`npm run app:build` 与 `npm run app:dev` 已自动包含这一步。

## 作为开发模板

写自己的插件时可以直接参考这个目录：

1. 改 `plugin.json` 的 `id` / `name` / `commands`
2. 改 `src/main.rs` 的 `run_command` 分发逻辑
3. 用 `StickyClient::execute()` 调用任意宿主命令（见「拓展 → Agent 命令」里的完整列表）

关键的三个约定：

- **事件**：宿主 POST 到 `endpoint`，body 是 `{ name, payload, at, source }`
- **命令**：宿主 POST 到 `endpoint`，body 是 `{ command, args }`，你把结果作为 JSON 返回
- **不碰数据库**：所有读写都走宿主的命令总线，避免并发问题

插件后端只需要一个能监听 HTTP、返回 JSON 的东西 —— Python、Node、Go、Rust 都行。

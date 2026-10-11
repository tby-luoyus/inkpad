# 主题包

主题包决定便签的配色与观感。放进数据目录的 `themes/<id>/` 即可生效。

## 目录结构

```
<数据目录>/themes/
└── my-theme/
    ├── theme.json     # 必需：主题清单
    └── theme.css      # 可选：附加样式
```

## theme.json

```json
{
  "id": "my-theme",
  "name": "我的主题",
  "author": "你",
  "description": "一句话描述",
  "version": "1.0.0",
  "appearance": "light",
  "colors": {
    "yellow": "#FFF3C4",
    "green":  "#DCEFD0",
    "pink":   "#FBD9E4",
    "purple": "#E7DFF7",
    "blue":   "#D3E8F7",
    "gray":   "#EAEAEA"
  },
  "cssVars": {
    "--sn-radius": "14px"
  },
  "cssFile": "theme.css",
  "renderer": null
}
```

| 字段 | 说明 |
| --- | --- |
| `appearance` | `light` / `dark` / `system`，决定便签文字用深色还是浅色 |
| `colors` | 六个色键对应的十六进制值。键名必须与内置一致（yellow/green/pink/purple/blue/gray） |
| `cssVars` | 覆盖任意 CSS 变量，见下表 |
| `cssFile` | 附加样式表（相对主题目录） |
| `renderer` | 自定义卡片渲染器 JS 路径（扩展点，预留） |

## 可覆盖的 CSS 变量

定义在 `src/styles/tokens.css`：

**便签配色**
`--sn-yellow` `--sn-green` `--sn-pink` `--sn-purple` `--sn-blue` `--sn-gray`
对应描边：`--sn-yellow-border` 等同名 `-border` 变量

**应用外壳**
`--sn-app-bg` `--sn-surface` `--sn-surface-2` `--sn-sidebar-bg`
`--sn-border` `--sn-border-strong`

**文字**
`--sn-text` `--sn-text-2` `--sn-text-3` `--sn-note-text`

**强调色**
`--sn-accent` `--sn-accent-hover` `--sn-accent-soft` `--sn-danger`

**形状与阴影**
`--sn-radius` `--sn-radius-sm` `--sn-radius-lg`
`--sn-shadow-note` `--sn-shadow-note-hover` `--sn-shadow-dialog` `--sn-shadow-note-win`

**字体**
`--sn-font` `--sn-font-note` `--sn-font-mono`

**语义别名**（笔记库 / 拓展面板使用，建议一并覆盖以免主题错配）
`--sn-text-1` `--sn-side-bg` `--sn-card-bg` `--sn-hover`
`--sn-mono` `--sn-code-bg` `--sn-code-text` `--sn-mark`
`--sn-ok` `--sn-ok-soft` `--sn-danger-soft` `--sn-shadow-lg`

> 这些别名默认映射到上面的基础变量。深色主题下需要同步覆盖
> `--sn-hover` `--sn-code-bg` `--sn-code-text` `--sn-mark` `--sn-ok` `--sn-ok-soft`
> `--sn-danger-soft` `--sn-shadow-lg`，否则笔记编辑器与插件面板在暗色下对比度会不足。

## 附加样式

`theme.css` 里可以直接写选择器：

```css
/* 便签窗口整体 */
.note-root { }

/* 便签正文 */
.note-textarea { }

/* 主窗口卡片 */
.note-card { }

/* 主窗口侧边栏 */
.sidebar { }
```

## 调试

改完主题后在应用「拓展 → 主题包」点「重新加载」，即时生效。

如果主题没出现，检查：

1. `theme.json` 是否为合法 JSON（少逗号、多了逗号都会失败）
2. `id` 是否与目录名不同导致困惑（以 `id` 为准）
3. 应用日志中的解析警告（`RUST_LOG=debug` 启动可见）

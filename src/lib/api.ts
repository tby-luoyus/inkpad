/**
 * 与 Rust 后端通信的类型化封装
 *
 * 这里是前端唯一的 IPC 出入口。所有其他组件都通过它访问后端，
 * 便于将来做 mock、埋点或替换传输层（例如切到 HTTP）。
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

// ---------------------------------------------------------------- 类型

export interface NoteMeta {
  [key: string]: unknown;
}

export interface Note {
  id: string;
  content: string;
  color: string;
  x: number;
  y: number;
  width: number;
  height: number;
  isOpen: boolean;
  isPinned: boolean;
  zIndex: number;
  deleted: boolean;
  hidden: boolean;
  fontScale: number;
  createdAt: string;
  updatedAt: string;
  deletedAt: string | null;
  meta: NoteMeta;
}

export interface NotePatch {
  content?: string;
  color?: string;
  x?: number;
  y?: number;
  width?: number;
  height?: number;
  isOpen?: boolean;
  isPinned?: boolean;
  zIndex?: number;
  fontScale?: number;
  hidden?: boolean;
  meta?: NoteMeta;
}

export interface NoteQuery {
  keyword?: string;
  includeDeleted?: boolean;
  onlyDeleted?: boolean;
  colors?: string[];
  sort?: 'updated' | 'created' | 'color' | 'alpha' | 'zindex';
}

export interface Settings {
  theme: 'system' | 'light' | 'dark';
  themePack: string;
  restoreOnStart: boolean;
  minimizeToTray: boolean;
  defaultColor: string;
  defaultNoteWidth: number;
  defaultNoteHeight: number;
  language: string;
  defaultFontScale: number;
  httpApiEnabled: boolean;
  httpApiPort: number;
  pluginsEnabled: boolean;
  trashRetentionDays: number;
  viewMode: 'grid' | 'list';
  extra: Record<string, unknown>;
}

export interface ThemePack {
  id: string;
  name: string;
  author: string;
  description: string;
  version: string;
  appearance: 'light' | 'dark' | 'system';
  colors: Record<string, string>;
  cssVars: Record<string, string>;
  cssFile: string | null;
  renderer: string | null;
  builtin: boolean;
}

export interface CommandSpec {
  name: string;
  description: string;
  params: unknown;
  category: string;
}

export interface PluginRuntime {
  id: string;
  name: string;
  author: string;
  description: string;
  version: string;
  permissions: string[];
  events: string[];
  commands: { name: string; description: string; params: unknown }[];
  endpoint: string | null;
  frontend: string | null;
  enabled: boolean;
  path: string;
  loaded: boolean;
  /** 自启动进程是否在运行 */
  processRunning?: boolean;
  /** 插件声明的自启动配置 */
  autoStart?: { command: string; args: string[]; cwd?: string | null } | null;
}

export interface AppEvent {
  name: string;
  payload: unknown;
  at: string;
  source: string;
}

export interface WebhookConfig {
  id: string;
  name: string;
  url: string;
  events: string[];
  enabled: boolean;
  headers: Record<string, string>;
}

export interface ApiStatus {
  running: boolean;
  port: number | null;
  baseUrl: string | null;
  token: string;
}

export interface PathsInfo {
  data: string;
  plugins: string;
  themes: string;
  backups: string;
  /** 笔记库根目录 */
  vault: string;
  /** 插件进程日志目录 */
  logs: string;
  database: string;
  http: ApiStatus;
}

export interface BackupFile {
  version: number;
  app: string;
  exportedAt: string;
  notes: Note[];
  settings: Settings | null;
}

export interface OpResult {
  ok: boolean;
  affected: number;
  message: string;
}

// ------------------------------------------------------------ 笔记（Vault）

/** 一篇笔记文档（对标 Obsidian 的 Markdown 笔记） */
export interface Doc {
  id: string;
  /** 相对笔记库根目录的路径，如 `日记/2026-10-11.md` */
  path: string;
  /** 笔记名（不含扩展名） */
  title: string;
  /** 所在文件夹（根目录为空串） */
  folder: string;
  /** 正文（不含 frontmatter） */
  content: string;
  /** frontmatter 键值对 */
  frontmatter: Record<string, unknown>;
  /** 正文中的 [[双向链接]] */
  links: string[];
  /** 正文中的 #标签 */
  tags: string[];
  wordCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface DocPatch {
  path?: string;
  content?: string;
  frontmatter?: Record<string, unknown>;
}

/** 笔记目录树节点 */
export interface DocNode {
  name: string;
  path: string;
  isDir: boolean;
  children: DocNode[];
}

/** 反向链接 */
export interface Backlink {
  id: string;
  title: string;
  path: string;
  context: string;
}

/** 笔记库概览 */
export interface VaultInfo {
  root: string;
  docCount: number;
  folderCount: number;
  tags: [string, number][];
  orphans: number;
}

// ------------------------------------------------------------ 便签 API

export const notes = {
  list: (query?: NoteQuery) => invoke<Note[]>('notes_list', { query: query ?? null }),
  get: (id: string) => invoke<Note | null>('notes_get', { id }),
  count: () => invoke<[number, number, number]>('notes_count'),
  create: (patch?: NotePatch, open = true) =>
    invoke<Note>('notes_create', { patch: patch ?? null, open }),
  update: (id: string, patch: NotePatch) => invoke<Note | null>('notes_update', { id, patch }),
  append: (id: string, text: string) => invoke<Note | null>('notes_append', { id, text }),
  trash: (id: string) => invoke<boolean>('notes_trash', { id }),
  restore: (id: string) => invoke<boolean>('notes_restore', { id }),
  purge: (id: string) => invoke<boolean>('notes_purge', { id }),
  purgeTrash: () => invoke<number>('notes_purge_trash'),
  duplicate: (id: string) => invoke<Note | null>('notes_duplicate', { id }),
  setPinned: (id: string, pinned: boolean) => invoke<boolean>('notes_set_pinned', { id, pinned }),
  setColor: (id: string, color: string) => invoke<boolean>('notes_set_color', { id, color }),
  setHidden: (id: string, hidden: boolean) => invoke<boolean>('notes_set_hidden', { id, hidden }),
  bringToFront: (id: string) => invoke<number>('notes_bring_to_front', { id }),
};

// ------------------------------------------------------------ 窗口 API

export const win = {
  openNote: (id: string) => invoke<void>('window_open_note', { id }),
  closeNote: (id: string) => invoke<void>('window_close_note', { id }),
  showMain: () => invoke<void>('window_show_main'),
  syncGeometry: (id: string) => invoke<void>('window_sync_geometry', { id }),
  noteClosing: (id: string) => invoke<void>('window_note_closing', { id }),
};

// ------------------------------------------------------------ 笔记 API

export const docs = {
  list: (folder?: string, tag?: string) =>
    invoke<Doc[]>('docs_list', { folder: folder ?? null, tag: tag ?? null }),
  tree: () => invoke<DocNode[]>('docs_tree'),
  info: () => invoke<VaultInfo>('docs_info'),
  tags: () => invoke<[string, number][]>('docs_tags'),
  get: (id: string) => invoke<Doc | null>('docs_get', { id }),
  search: (keyword: string) => invoke<Doc[]>('docs_search', { keyword }),
  create: (patch: DocPatch) => invoke<Doc>('docs_create', { patch }),
  save: (id: string, patch: DocPatch) => invoke<Doc | null>('docs_save', { id, patch }),
  rename: (id: string, path: string) => invoke<Doc | null>('docs_rename', { id, path }),
  remove: (id: string) => invoke<boolean>('docs_delete', { id }),
  backlinks: (id: string) => invoke<Backlink[]>('docs_backlinks', { id }),
  outlinks: (id: string) => invoke<Doc[]>('docs_outlinks', { id }),
  reindex: () => invoke<number>('docs_reindex'),
  /** 便签汇总生成笔记 */
  fromNotes: (ids: string[], title?: string, folder?: string) =>
    invoke<Doc>('docs_from_notes', {
      ids,
      title: title ?? null,
      folder: folder ?? null,
    }),
};

// ------------------------------------------------------------ 设置 API

export const settings = {
  get: () => invoke<Settings>('settings_get'),
  save: (s: Settings) => invoke<Settings>('settings_save', { settings: s }),
};

// ------------------------------------------------------------ 备份 API

export const backup = {
  exportDialog: () => invoke<string>('backup_export'),
  importDialog: () => invoke<OpResult>('backup_import'),
  exportPath: (path: string) => invoke<OpResult>('backup_export_path', { path }),
  importPath: (path: string, overwrite = false) =>
    invoke<OpResult>('backup_import_path', { path, overwrite }),
  snapshot: () => invoke<BackupFile>('data_export_json'),
};

// -------------------------------------------------------- 拓展接口 API

export const ext = {
  commands: () => invoke<CommandSpec[]>('ext_commands'),
  themes: () => invoke<ThemePack[]>('ext_themes'),
  reloadThemes: () => invoke<ThemePack[]>('ext_reload_themes'),
  plugins: () => invoke<PluginRuntime[]>('ext_plugins'),
  reloadPlugins: () => invoke<PluginRuntime[]>('ext_reload_plugins'),
  setPluginEnabled: (id: string, enabled: boolean) =>
    invoke<boolean>('ext_set_plugin_enabled', { id, enabled }),
  /** 启动插件的自启动进程 */
  startPlugin: (id: string) => invoke<boolean>('ext_plugin_start', { id }),
  /** 停止插件的自启动进程 */
  stopPlugin: (id: string) => invoke<boolean>('ext_plugin_stop', { id }),
  /** 探测插件后端是否可达 */
  pingPlugin: (id: string) =>
    invoke<{ reachable: boolean; status?: number; body?: string; reason?: string }>(
      'ext_plugin_ping',
      { id }
    ),
  /** 读取插件后端进程日志（排查启动失败） */
  pluginLog: (id: string, limit = 200) =>
    invoke<{ path: string; exists: boolean; text: string }>('ext_plugin_log', { id, limit }),
  eventsRecent: (limit = 50) => invoke<AppEvent[]>('ext_events_recent', { limit }),
  webhooks: () => invoke<WebhookConfig[]>('ext_webhooks'),
  upsertWebhook: (config: WebhookConfig) => invoke<boolean>('ext_webhook_upsert', { config }),
  removeWebhook: (id: string) => invoke<boolean>('ext_webhook_remove', { id }),
  emitEvent: (name: string, payload?: unknown) =>
    invoke<void>('ext_emit_event', { name, payload: payload ?? null }),
  /** 统一命令入口 —— Agent / 插件 / 自动化都走它 */
  execute: <T = unknown>(command: string, args: Record<string, unknown> = {}) =>
    invoke<T>('ext_execute', { command, args }),
  apiStatus: () => invoke<ApiStatus>('ext_api_status'),
  setApiEnabled: (enabled: boolean, port?: number) =>
    invoke<ApiStatus>('ext_api_set_enabled', { enabled, port: port ?? null }),
  paths: () => invoke<PathsInfo>('ext_paths'),
  openDir: (kind: 'data' | 'plugins' | 'themes' | 'backups' | 'vault' | 'logs') =>
    invoke<void>('ext_open_dir', { kind }),
};

// ------------------------------------------------------- 事件订阅 API

/**
 * 内部事件名 → Tauri 事件名。
 *
 * Tauri 2 的事件名校验只允许字母数字与 `-`、`/`、`:`、`_`，**不允许 `.`**，
 * 而本项目对外的事件契约沿用 `note.created` 这类带点的写法（插件 / Webhook 也用）。
 * 因此只在「传给 Tauri」这一层做转换，业务代码无需关心。
 * 后端 `hub::to_tauri_event_name` 是同一套规则。
 */
function tauriEventName(name: string): string {
  return name.replace(/\./g, ':');
}

/** 后端事件 → 前端。返回取消订阅函数。 */
export function onEvent<T = unknown>(
  name: string,
  handler: (payload: T) => void
): Promise<UnlistenFn> {
  return listen<T>(tauriEventName(name), (e) => handler(e.payload));
}

/**
 * 订阅多个事件，统一返回一个取消订阅函数。
 *
 * 单个事件名不合法或订阅失败时**不会拖垮其余订阅**，也不会中断调用方的初始化：
 * 失败的会被记下来，其余照常生效。
 */
export async function onEvents(
  names: string[],
  handler: (name: string, payload: unknown) => void
): Promise<UnlistenFn> {
  const results = await Promise.allSettled(
    names.map((n) => listen(tauriEventName(n), (e) => handler(n, e.payload)))
  );
  const unlisteners: UnlistenFn[] = [];
  results.forEach((r, i) => {
    if (r.status === 'fulfilled') {
      unlisteners.push(r.value);
    } else {
      console.warn(`订阅事件 ${names[i]} 失败:`, r.reason);
    }
  });
  return () => unlisteners.forEach((u) => u());
}

// ------------------------------------------------------------ 便捷封装

export const COLORS = ['yellow', 'green', 'pink', 'purple', 'blue', 'gray'] as const;
export type ColorId = (typeof COLORS)[number];

export const COLOR_LABELS: Record<string, string> = {
  yellow: '黄色',
  green: '绿色',
  pink: '粉色',
  purple: '紫色',
  blue: '蓝色',
  gray: '灰色',
};

/** 一张便签的显示标题（首行） */
export function noteTitle(n: Note): string {
  const first = n.content.split('\n').find((l) => l.trim().length > 0);
  return first?.trim().slice(0, 80) ?? '';
}

/** 便签正文预览（去掉首行标题） */
export function notePreview(n: Note): string {
  const lines = n.content.split('\n');
  const idx = lines.findIndex((l) => l.trim().length > 0);
  if (idx < 0) return '';
  return lines.slice(idx + 1).join(' ').replace(/\s+/g, ' ').trim().slice(0, 160);
}

/** 相对时间：刚刚 / 5 分钟前 / 昨天 14:08 / 2026/3/5 */
export function relativeTime(iso: string): string {
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return '';
  const diff = Date.now() - t;
  const min = Math.floor(diff / 60000);
  if (min < 1) return '刚刚';
  if (min < 60) return `${min} 分钟前`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr} 小时前`;
  const d = new Date(t);
  const today = new Date();
  const isYesterday =
    d.getFullYear() === today.getFullYear() &&
    d.getMonth() === today.getMonth() &&
    today.getDate() - d.getDate() === 1;
  const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  if (isYesterday) return `昨天 ${hm}`;
  const sameYear = d.getFullYear() === today.getFullYear();
  return sameYear
    ? `${d.getMonth() + 1}月${d.getDate()}日`
    : `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()}`;
}

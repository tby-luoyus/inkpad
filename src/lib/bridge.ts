/**
 * 前端桥接层
 *
 * 把命令总线暴露到 `window.__sticky`，让：
 *  - 浏览器扩展 / 用户脚本
 *  - 纯前端插件（通过 theme/plugin 声明注入的 JS）
 *  - 开发者控制台
 * 都能用统一 API 操作便签，无需了解 Tauri IPC 细节。
 *
 * 用法：
 *   await window.__sticky.execute('notes.create', { content: '来自脚本' })
 *   await window.__sticky.notes.list()
 *   window.__sticky.on('note.created', n => console.log(n))
 */

import { ext, notes as notesApi, win as winApi, settings as settingsApi, onEvent } from './api';

export interface StickyBridge {
  version: string;
  /** 统一命令入口（推荐） */
  execute: <T = unknown>(command: string, args?: Record<string, unknown>) => Promise<T>;
  /** 便捷封装 */
  notes: typeof notesApi;
  win: typeof winApi;
  settings: typeof settingsApi;
  ext: typeof ext;
  /** 订阅事件，返回取消函数 */
  on: (name: string, handler: (payload: unknown) => void) => Promise<() => void>;
  /** 常用能力速查 */
  commands: () => Promise<unknown>;
}

declare global {
  interface Window {
    __sticky?: StickyBridge;
  }
}

export function installStickyBridge() {
  if (window.__sticky) return window.__sticky;

  const bridge: StickyBridge = {
    version: '1.0.0',
    execute: (command, args = {}) => ext.execute(command, args),
    notes: notesApi,
    win: winApi,
    settings: settingsApi,
    ext,
    on: async (name, handler) => onEvent(name, handler),
    commands: () => ext.commands(),
  };

  window.__sticky = bridge;

  // 加载纯前端插件
  void loadFrontendPlugins(bridge);

  return bridge;
}

/**
 * 纯前端插件：plugin.json 里声明 `frontend: "index.js"`，
 * 我们把它作为 ES 模块动态 import，插件拿到 bridge 即可工作。
 */
async function loadFrontendPlugins(bridge: StickyBridge) {
  try {
    const list = await ext.plugins();
    const enabled = list.filter((p) => p.enabled && p.frontend);
    for (const p of enabled) {
      const entry = p.frontend!;
      // 插件目录是绝对路径，交由 Tauri 的 asset 协议或本地文件服务加载。
      // 生产环境下通过 convertFileSrc 转成 webview 可访问的 URL。
      try {
        const { convertFileSrc } = await import('@tauri-apps/api/core');
        const abs = entry.startsWith('/') || /^[a-zA-Z]:/.test(entry) ? entry : `${p.path}/${entry}`;
        const url = convertFileSrc(abs.replace(/\\/g, '/'));
        const mod = await import(/* @vite-ignore */ url);
        if (typeof mod.default === 'function') {
          await mod.default(bridge, p);
        } else if (typeof mod.activate === 'function') {
          await mod.activate(bridge, p);
        }
        console.info(`[插件] ${p.name} 已加载`);
      } catch (e) {
        console.warn(`[插件] ${p.name} 前端脚本加载失败:`, e);
      }
    }
  } catch {
    /* 无插件时静默 */
  }
}

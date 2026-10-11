/**
 * 便签窗口入口
 *
 * 从 URL 查询串拿到便签 id，加载后渲染；同时订阅后端事件以便外部改动实时反映。
 * 主题（亮/暗）与配色在此应用，保证独立窗口与主窗口视觉一致。
 */

import React, { useEffect, useState } from 'react';
import ReactDOM from 'react-dom/client';
import { NoteWindow } from './components/NoteWindow';
import { notes as notesApi, settings as settingsApi, ext, onEvent, type Note, type Settings } from './lib/api';
import './styles/tokens.css';

function NoteApp() {
  const [note, setNote] = useState<Note | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [colorMap, setColorMap] = useState<Record<string, string> | undefined>();

  const id = new URLSearchParams(window.location.search).get('id') ?? '';

  // 初载
  useEffect(() => {
    (async () => {
      if (!id) {
        setError('缺少便签 ID');
        return;
      }
      try {
        const [n, s] = await Promise.all([notesApi.get(id), settingsApi.get()]);
        if (!n) {
          setError('便签不存在或已被删除');
          return;
        }
        setNote(n);
        setSettings(s);
        const themes = await ext.themes();
        setColorMap(themes.find((t) => t.id === s.themePack)?.colors);
      } catch (e) {
        setError(String(e));
      }
    })();
  }, [id]);

  // 主题
  useEffect(() => {
    if (!settings) return;
    const apply = (dark: boolean) => {
      document.documentElement.dataset.theme = dark ? 'dark' : 'light';
    };
    if (settings.theme === 'system') {
      const mq = window.matchMedia('(prefers-color-scheme: dark)');
      apply(mq.matches);
      const on = (e: MediaQueryListEvent) => apply(e.matches);
      mq.addEventListener('change', on);
      return () => mq.removeEventListener('change', on);
    }
    apply(settings.theme === 'dark');
  }, [settings]);

  // 后端事件：内容/颜色可能被 Agent 或主窗口改动
  useEffect(() => {
    let uns: (() => void)[] = [];
    (async () => {
      const a = await onEvent<{ id: string; color: string }>('note.colorChanged', (p) => {
        if (p.id === id) setNote((prev) => (prev ? { ...prev, color: p.color } : prev));
      });
      const b = await onEvent<{ id: string; pinned: boolean }>('note.pinnedChanged', (p) => {
        if (p.id === id) setNote((prev) => (prev ? { ...prev, isPinned: p.pinned } : prev));
      });
      const c = await onEvent<{ id: string }>('note.purged', (p) => {
        if (p.id === id) setError('此便签已被彻底删除');
      });
      uns = [a, b, c];
    })();
    return () => uns.forEach((u) => u());
  }, [id]);

  if (error) {
    return (
      <div className="note-root" data-color="gray">
        <div className="note-error">
          <p>{error}</p>
          <button
            type="button"
            onClick={() => {
              import('@tauri-apps/api/window').then(({ getCurrentWindow }) =>
                getCurrentWindow().destroy()
              );
            }}
          >
            关闭
          </button>
        </div>
      </div>
    );
  }

  if (!note) {
    return <div className="note-root" data-color="yellow" />;
  }

  return (
    <NoteWindow
      note={note}
      colorMap={colorMap}
      onPatch={(patch) => setNote((prev) => (prev ? { ...prev, ...patch } : prev))}
    />
  );
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <NoteApp />
  </React.StrictMode>
);

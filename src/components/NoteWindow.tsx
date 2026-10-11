/**
 * 单张便签（独立窗口）
 *
 * 完全对齐 Windows 便笺的行为：
 *  - 无边框；顶部工具栏即拖动区域
 *  - 新建(+) / 颜色 / 删除 / 置顶 / 更多
 *  - 正文即时编辑 + 自动保存（防抖 400ms）
 *  - Esc 关闭、Ctrl+N 新建、Ctrl+B 加粗等快捷键
 *  - 失焦不关闭；关闭时把窗口几何写回数据库
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { PhysicalPosition, PhysicalSize } from '@tauri-apps/api/window';
import { ColorPicker } from './ColorPicker';
import {
  IconPlus,
  IconTrash,
  IconPin,
  IconMoreVertical,
  IconCopy,
  IconEyeOff,
  IconArchive,
  IconBolt,
  IconLink,
} from './Icons';
import { notes as notesApi, win as winApi, ext, type Note } from '../lib/api';
import './NoteWindow.css';

interface Props {
  note: Note;
  onPatch: (patch: Partial<Note>) => void;
  colorMap?: Record<string, string>;
}

type MenuId = 'copy' | 'hide' | 'pin' | 'trash' | 'purge' | 'agent' | 'link' | null;

export function NoteWindow({ note, onPatch, colorMap }: Props) {
  const [content, setContent] = useState(note.content);
  const [menuOpen, setMenuOpen] = useState(false);
  const [pinned, setPinned] = useState(note.isPinned);
  const [saving, setSaving] = useState(false);
  const [showAgent, setShowAgent] = useState(false);
  const [agentPrompt, setAgentPrompt] = useState('');
  const [agentResult, setAgentResult] = useState<string | null>(null);

  const areaRef = useRef<HTMLTextAreaElement>(null);
  const saveTimer = useRef<number | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const lastSaved = useRef(note.content);

  // 主题属性挂到 body，让整个窗口共享
  useEffect(() => {
    document.body.classList.add('note-window');
    document.body.dataset.color = note.color;
    return () => {
      document.body.classList.remove('note-window');
    };
  }, [note.color]);

  // 外部更新（例如 Agent 改写了内容）时同步进输入框
  useEffect(() => {
    if (note.content !== lastSaved.current && note.content !== content) {
      setContent(note.content);
      lastSaved.current = note.content;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [note.content]);

  // ---------------------------------------------------------------- 自动保存
  const flush = useCallback(
    async (value: string) => {
      if (value === lastSaved.current) return;
      setSaving(true);
      try {
        await notesApi.update(note.id, { content: value });
        lastSaved.current = value;
      } catch (e) {
        console.error('保存失败', e);
      } finally {
        setSaving(false);
      }
    },
    [note.id]
  );

  const scheduleSave = useCallback(
    (value: string) => {
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
      saveTimer.current = window.setTimeout(() => flush(value), 380);
    },
    [flush]
  );

  // 关闭前确保落盘
  useEffect(() => {
    const handle = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    (async () => {
      unlisten = await handle.onCloseRequested(async (event) => {
        event.preventDefault();
        if (saveTimer.current) window.clearTimeout(saveTimer.current);
        await flush(content);
        await syncGeometry();
        await winApi.noteClosing(note.id);
        await handle.destroy();
      });
    })();
    return () => unlisten?.();
  }, [content, flush, note.id]);

  const syncGeometry = useCallback(async () => {
    try {
      const h = getCurrentWindow();
      const pos = await h.outerPosition();
      const size = await h.innerSize();
      await notesApi.update(note.id, {
        x: pos.x,
        y: pos.y,
        width: size.width,
        height: size.height,
      });
    } catch {
      /* 窗口可能已销毁 */
    }
  }, [note.id]);

  // 窗口获得焦点时置顶
  useEffect(() => {
    const h = getCurrentWindow();
    let un: (() => void) | undefined;
    (async () => {
      un = await h.onFocusChanged(async (focused) => {
        if (focused) {
          try {
            await notesApi.bringToFront(note.id);
          } catch {
            /* ignore */
          }
        }
      });
    })();
    return () => un?.();
  }, [note.id]);

  // ------------------------------------------------------------------ 操作
  const handleNew = useCallback(async () => {
    try {
      const n = await notesApi.create({ color: note.color }, true);
      if (n) {
        // 新便签稍微错开
        await winApi.openNote(n.id);
      }
    } catch (e) {
      console.error(e);
    }
  }, [note.color]);

  const handleColor = useCallback(
    async (color: string) => {
      document.body.dataset.color = color;
      onPatch({ color });
      try {
        await notesApi.setColor(note.id, color);
      } catch (e) {
        console.error(e);
      }
    },
    [note.id, onPatch]
  );

  const handleDelete = useCallback(async () => {
    try {
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
      await notesApi.trash(note.id);
    } catch (e) {
      console.error(e);
    }
  }, [note.id]);

  const handlePin = useCallback(async () => {
    const next = !pinned;
    setPinned(next);
    onPatch({ isPinned: next });
    try {
      await notesApi.setPinned(note.id, next);
    } catch (e) {
      console.error(e);
    }
  }, [note.id, pinned, onPatch]);

  const handleCopy = useCallback(async () => {
    try {
      const n = await notesApi.duplicate(note.id);
      if (n) await winApi.openNote(n.id);
    } catch (e) {
      console.error(e);
    }
    setMenuOpen(false);
  }, [note.id]);

  const handleHide = useCallback(async () => {
    setMenuOpen(false);
    try {
      await notesApi.setHidden(note.id, true);
      await getCurrentWindow().hide();
    } catch (e) {
      console.error(e);
    }
  }, [note.id]);

  const handlePurge = useCallback(async () => {
    setMenuOpen(false);
    try {
      await notesApi.purge(note.id);
    } catch (e) {
      console.error(e);
    }
  }, [note.id]);

  /** 调用 Agent 命令总线 —— 预留的智能化入口 */
  const handleAgent = useCallback(async () => {
    if (!agentPrompt.trim()) return;
    setAgentResult('正在处理…');
    try {
      // 约定：Agent 侧注册 `agent.notes.assist` 命令。
      // 未注册时后端会返回错误，这里降级为本地可读提示。
      const r = await ext.execute<{ text?: string }>('agent.notes.assist', {
        id: note.id,
        prompt: agentPrompt,
        content,
      });
      const text = (r as any)?.text ?? JSON.stringify(r);
      setAgentResult(String(text));
    } catch (e) {
      setAgentResult(
        `未接入 Agent：${String(e)}\n\n在「拓展」面板注册 agent.notes.assist 命令后即可使用。`
      );
    }
  }, [agentPrompt, content, note.id]);

  const handleLink = useCallback(async () => {
    setMenuOpen(false);
    const url = `sticky-notes://note/${note.id}`;
    try {
      await navigator.clipboard.writeText(url);
      setAgentResult(`已复制便签链接：${url}`);
      setShowAgent(true);
    } catch {
      setAgentResult(url);
      setShowAgent(true);
    }
  }, [note.id]);

  // ------------------------------------------------------------------ 快捷键
  useEffect(() => {
    const onKey = async (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (e.key === 'Escape' && !showAgent) {
        e.preventDefault();
        if (saveTimer.current) window.clearTimeout(saveTimer.current);
        await flush(content);
        await syncGeometry();
        await winApi.noteClosing(note.id);
        await getCurrentWindow().destroy();
        return;
      }
      if (mod && e.key.toLowerCase() === 'n') {
        e.preventDefault();
        handleNew();
        return;
      }
      if (mod && e.key.toLowerCase() === 'd') {
        e.preventDefault();
        handleCopy();
        return;
      }
      if (mod && e.key.toLowerCase() === 'p') {
        e.preventDefault();
        handlePin();
        return;
      }
      if (mod && e.key === 'Delete') {
        e.preventDefault();
        handleDelete();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [content, flush, handleCopy, handleDelete, handleNew, handlePin, note.id, showAgent, syncGeometry]);

  // 菜单点击外部关闭
  useEffect(() => {
    if (!menuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [menuOpen]);

  // 首次挂载后聚焦正文
  useEffect(() => {
    const t = window.setTimeout(() => areaRef.current?.focus(), 90);
    return () => window.clearTimeout(t);
  }, []);

  const statusText = useMemo(() => {
    if (saving) return '保存中…';
    const len = content.length;
    if (len === 0) return '开始输入…';
    return `${len} 字`;
  }, [saving, content]);

  return (
    <div className="note-root" data-color={note.color}>
      {/* ---------------------------------------------------------- 工具栏 */}
      <header className="note-bar" data-tauri-drag-region>
        <div className="note-bar__left sn-no-drag">
          <button
            type="button"
            className="nb-btn"
            onClick={handleNew}
            title="新建便签 (Ctrl+N)"
          >
            <IconPlus size={16} />
          </button>
          <ColorPicker value={note.color} onChange={handleColor} colorMap={colorMap} />
        </div>

        <div className="note-bar__spacer" data-tauri-drag-region />

        <div className="note-bar__right sn-no-drag">
          <button
            type="button"
            className={`nb-btn ${pinned ? 'is-active' : ''}`}
            onClick={handlePin}
            title={pinned ? '取消置顶 (Ctrl+P)' : '置顶便签 (Ctrl+P)'}
          >
            <IconPin size={16} filled={pinned} />
          </button>
          <button
            type="button"
            className="nb-btn"
            onClick={handleDelete}
            title="删除便签 (Ctrl+Del)"
          >
            <IconTrash size={16} />
          </button>
          <div className="note-bar__menu-wrap" ref={menuRef}>
            <button
              type="button"
              className={`nb-btn ${menuOpen ? 'is-active' : ''}`}
              onClick={() => setMenuOpen((v) => !v)}
              title="更多选项"
            >
              <IconMoreVertical size={16} />
            </button>
            {menuOpen && (
              <div className="note-menu" role="menu">
                <button role="menuitem" onClick={handleCopy}>
                  <IconCopy size={15} />
                  <span>复制便签</span>
                  <kbd>Ctrl+D</kbd>
                </button>
                <button role="menuitem" onClick={handleHide}>
                  <IconEyeOff size={15} />
                  <span>从列表中隐藏</span>
                </button>
                <button role="menuitem" onClick={() => { setMenuOpen(false); handlePin(); }}>
                  <IconPin size={15} filled={pinned} />
                  <span>{pinned ? '取消置顶' : '置顶便签'}</span>
                  <kbd>Ctrl+P</kbd>
                </button>
                <button
                  role="menuitem"
                  onClick={() => {
                    setMenuOpen(false);
                    setShowAgent(true);
                    setAgentResult(null);
                  }}
                >
                  <IconBolt size={15} />
                  <span>AI 助手…</span>
                </button>
                <button role="menuitem" onClick={handleLink}>
                  <IconLink size={15} />
                  <span>复制便签链接</span>
                </button>
                <div className="note-menu__sep" />
                <button role="menuitem" className="is-danger" onClick={handlePurge}>
                  <IconArchive size={15} />
                  <span>彻底删除</span>
                </button>
              </div>
            )}
          </div>
        </div>
      </header>

      {/* ---------------------------------------------------------- 正文 */}
      <div className="note-body">
        <textarea
          ref={areaRef}
          className="note-textarea"
          value={content}
          spellCheck={false}
          placeholder="写下点什么…"
          onChange={(e) => {
            setContent(e.target.value);
            scheduleSave(e.target.value);
          }}
          onBlur={() => {
            if (saveTimer.current) window.clearTimeout(saveTimer.current);
            flush(content);
          }}
          style={{ fontSize: `${16 * (note.fontScale || 1)}px` }}
        />

        {showAgent && (
          <div className="note-agent">
            <div className="note-agent__head">
              <IconBolt size={15} />
              <span>AI 助手</span>
              <button
                type="button"
                className="note-agent__close"
                onClick={() => setShowAgent(false)}
                title="关闭"
              >
                ✕
              </button>
            </div>
            <textarea
              className="note-agent__input"
              placeholder="让 AI 帮你总结、续写、翻译或整理这张便签…"
              value={agentPrompt}
              onChange={(e) => setAgentPrompt(e.target.value)}
              rows={2}
            />
            <div className="note-agent__actions">
              <button
                type="button"
                className="note-agent__run"
                onClick={handleAgent}
                disabled={!agentPrompt.trim()}
              >
                执行
              </button>
              <span className="note-agent__hint">走命令总线 agent.notes.assist</span>
            </div>
            {agentResult && <pre className="note-agent__out">{agentResult}</pre>}
          </div>
        )}
      </div>

      {/* ---------------------------------------------------------- 状态栏 */}
      <footer className="note-foot" data-tauri-drag-region>
        <span className="note-foot__len" data-tauri-drag-region>{statusText}</span>
      </footer>
    </div>
  );
}

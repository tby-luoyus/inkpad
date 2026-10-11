/**
 * 便签卡片
 *
 * 主窗口网格 / 列表两种视图共用的卡片。
 * 视觉对齐 Windows 便笺：便签色底 + 首行标题 + 预览 + 底部时间与操作。
 */

import { useState } from 'react';
import { IconPin, IconMore, IconTrash, IconCopy, IconEyeOff, IconNote } from './Icons';
import { noteTitle, notePreview, relativeTime, type Note } from '../lib/api';
import './NoteCard.css';

interface Props {
  note: Note;
  view: 'grid' | 'list';
  onOpen: (id: string) => void;
  onPin: (id: string, pinned: boolean) => void;
  onTrash: (id: string) => void;
  onDuplicate: (id: string) => void;
  onHide: (id: string) => void;
  /** 回收站视图里显示「恢复」 */
  trashed?: boolean;
  onRestore?: (id: string) => void;
  onPurge?: (id: string) => void;
  /** 主题提供的颜色覆盖 */
  colorMap?: Record<string, string>;
}

export function NoteCard({
  note,
  view,
  onOpen,
  onPin,
  onTrash,
  onDuplicate,
  onHide,
  trashed,
  onRestore,
  onPurge,
  colorMap,
}: Props) {
  const [menuOpen, setMenuOpen] = useState(false);
  const title = noteTitle(note);
  const preview = notePreview(note);
  const hex = colorMap?.[note.color];

  return (
    <article
      className={`note-card note-card--${view} ${note.isPinned ? 'is-pinned' : ''}`}
      data-color={note.color}
      style={hex ? ({ '--sn-note-bg': hex } as React.CSSProperties) : undefined}
      onClick={() => onOpen(note.id)}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenuOpen(true);
      }}
      tabIndex={0}
      role="button"
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onOpen(note.id);
        }
      }}
    >
      <div className="note-card__head">
        <h3 className="note-card__title">
          {title || <span className="note-card__untitled">空白便签</span>}
        </h3>
        <div className="note-card__actions" onClick={(e) => e.stopPropagation()}>
          {!trashed && (
            <button
              type="button"
              className={`nc-btn ${note.isPinned ? 'is-active' : ''}`}
              title={note.isPinned ? '取消置顶' : '置顶'}
              onClick={() => onPin(note.id, !note.isPinned)}
            >
              <IconPin size={14} filled={note.isPinned} />
            </button>
          )}
          <div className="nc-menu-wrap">
            <button
              type="button"
              className={`nc-btn ${menuOpen ? 'is-active' : ''}`}
              title="更多"
              onClick={() => setMenuOpen((v) => !v)}
            >
              <IconMore size={14} />
            </button>
            {menuOpen && (
              <>
                <div className="nc-menu-backdrop" onClick={() => setMenuOpen(false)} />
                <div className="nc-menu" role="menu">
                  {trashed ? (
                    <>
                      <button
                        role="menuitem"
                        onClick={() => {
                          setMenuOpen(false);
                          onRestore?.(note.id);
                        }}
                      >
                        <IconCopy size={14} />
                        <span>恢复便签</span>
                      </button>
                      <button
                        role="menuitem"
                        className="is-danger"
                        onClick={() => {
                          setMenuOpen(false);
                          onPurge?.(note.id);
                        }}
                      >
                        <IconTrash size={14} />
                        <span>彻底删除</span>
                      </button>
                    </>
                  ) : (
                    <>
                      <button
                        role="menuitem"
                        onClick={() => {
                          setMenuOpen(false);
                          onOpen(note.id);
                        }}
                      >
                        <IconNote size={14} />
                        <span>打开便签</span>
                      </button>
                      <button
                        role="menuitem"
                        onClick={() => {
                          setMenuOpen(false);
                          onDuplicate(note.id);
                        }}
                      >
                        <IconCopy size={14} />
                        <span>复制便签</span>
                      </button>
                      <button
                        role="menuitem"
                        onClick={() => {
                          setMenuOpen(false);
                          onHide(note.id);
                        }}
                      >
                        <IconEyeOff size={14} />
                        <span>从列表中隐藏</span>
                      </button>
                      <button
                        role="menuitem"
                        className="is-danger"
                        onClick={() => {
                          setMenuOpen(false);
                          onTrash(note.id);
                        }}
                      >
                        <IconTrash size={14} />
                        <span>删除便签</span>
                      </button>
                    </>
                  )}
                </div>
              </>
            )}
          </div>
        </div>
      </div>

      <div className="note-card__body">
        {preview ? (
          <p className="note-card__preview">{preview}</p>
        ) : (
          <p className="note-card__preview note-card__preview--empty">
            {title ? '（无更多内容）' : '空白便签'}
          </p>
        )}
      </div>

      <div className="note-card__foot">
        <time className="note-card__time" dateTime={note.updatedAt}>
          {relativeTime(note.updatedAt)}
        </time>
        {note.isPinned && <span className="note-card__badge">已置顶</span>}
      </div>
    </article>
  );
}

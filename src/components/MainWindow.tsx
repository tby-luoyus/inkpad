/**
 * 主窗口 —— 便签面板
 *
 * 对齐 Windows 便笺主窗口：
 *  - 左侧导航（全部便签 / 已置顶 / 回收站 / 拓展）
 *  - 顶部搜索框 + 新建便签
 *  - 便签网格 / 列表两种视图，网格是默认
 *  - 选中便签时顶部出现批量操作条
 *  - 自定义标题栏（无边框窗口）
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { NoteCard } from './NoteCard';
import { SettingsPanel } from './SettingsPanel';
import { ExtensionPanel } from './ExtensionPanel';
import { NotesWorkspace } from './NotesWorkspace';
import { ColorPicker } from './ColorPicker';
import {
  IconPlus,
  IconSearch,
  IconClose,
  IconMinimize,
  IconMaximize,
  IconRestore,
  IconGrid,
  IconList,
  IconSettings,
  IconPuzzle,
  IconTrash,
  IconNote,
  IconPin,
  IconRefresh,
  IconBook,
  IconSparkle,
} from './Icons';
import {
  notes as notesApi,
  settings as settingsApi,
  docs as docsApi,
  ext,
  onEvents,
  COLORS,
  noteTitle,
  type Note,
  type Settings,
  type ThemePack,
} from '../lib/api';
import './MainWindow.css';

type NavKey = 'all' | 'pinned' | 'trash' | 'docs' | 'ext' | 'settings';
type SortKey = 'updated' | 'created' | 'color' | 'alpha';

export function MainWindow() {
  const [allNotes, setAllNotes] = useState<Note[]>([]);
  const [loading, setLoading] = useState(true);
  const [nav, setNav] = useState<NavKey>('all');
  const [keyword, setKeyword] = useState('');
  const [sort, setSort] = useState<SortKey>('updated');
  const [colorFilter, setColorFilter] = useState<string | null>(null);
  const [view, setView] = useState<'grid' | 'list'>('grid');
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [settings, setSettings] = useState<Settings | null>(null);
  const [themes, setThemes] = useState<ThemePack[]>([]);
  const [maximized, setMaximized] = useState(false);
  const [toast, setToast] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [ready, setReady] = useState(false);

  const searchRef = useRef<HTMLInputElement>(null);
  const toastTimer = useRef<number | null>(null);

  const flash = useCallback((msg: string) => {
    setToast(msg);
    if (toastTimer.current) window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 2200);
  }, []);

  // -------------------------------------------------------------- 数据加载
  const reload = useCallback(async () => {
    try {
      const list = await notesApi.list({
        includeDeleted: true,
        sort: 'updated',
      });
      setAllNotes(list);
    } catch (e) {
      console.error('加载便签失败', e);
      setLoadError(friendlyError(e, '便签'));
    } finally {
      setLoading(false);
    }
  }, []);

  /**
   * 首屏加载：三个请求各自独立容错。
   * 关键点——任何一个失败都不能阻塞其余部分，否则会卡在「正在读取设置…」。
   */
  useEffect(() => {
    (async () => {
      await reload();

      const [s, t] = await Promise.allSettled([settingsApi.get(), ext.themes()]);

      if (s.status === 'fulfilled') {
        setSettings(s.value);
        setView(s.value.viewMode === 'list' ? 'list' : 'grid');
      } else {
        console.error('读取设置失败', s.reason);
        setLoadError(friendlyError(s.reason, '设置'));
      }

      if (t.status === 'fulfilled') {
        setThemes(t.value);
      } else {
        console.error('读取主题失败', t.reason);
      }

      setReady(true);
    })();
  }, [reload]);

  // --------------------------------------------------- 主题：亮/暗 + 主题包
  useEffect(() => {
    if (!settings) return;
    const pack = themes.find((t) => t.id === settings.themePack);
    const apply = (dark: boolean) => {
      const root = document.documentElement;
      root.dataset.theme = dark ? 'dark' : 'light';
      // 主题配色 -> CSS 变量
      if (pack) {
        for (const [k, v] of Object.entries(pack.colors ?? {})) {
          root.style.setProperty(`--sn-${k}`, v);
        }
        for (const [k, v] of Object.entries(pack.cssVars ?? {})) {
          root.style.setProperty(k, v);
        }
      }
    };

    if (settings.theme === 'system') {
      const mq = window.matchMedia('(prefers-color-scheme: dark)');
      apply(mq.matches);
      const on = (e: MediaQueryListEvent) => apply(e.matches);
      mq.addEventListener('change', on);
      return () => mq.removeEventListener('change', on);
    }
    apply(settings.theme === 'dark' || pack?.appearance === 'dark');
  }, [settings, themes]);

  // ---------------------------------------------------------- 补丁式更新
  const patchLocal = useCallback((id: string, patch: Partial<Note>) => {
    setAllNotes((prev) => prev.map((n) => (n.id === id ? { ...n, ...patch } : n)));
  }, []);

  // ------------------------------------------------------------ 后端事件
  useEffect(() => {
    let un: (() => void) | undefined;
    (async () => {
      un = await onEvents(
        [
          'note.created',
          'note.updated',
          'note.trashed',
          'note.restored',
          'note.purged',
          'note.colorChanged',
          'note.pinnedChanged',
          'trash.emptied',
          'data.imported',
        ],
        (name, payload) => {
          const p = payload as { id?: string; color?: string; pinned?: boolean; count?: number };
          switch (name) {
            case 'note.created':
            case 'note.updated':
            case 'note.restored':
              reload();
              break;
            case 'note.colorChanged':
              if (p?.id && p.color) patchLocal(p.id, { color: p.color });
              break;
            case 'note.pinnedChanged':
              if (p?.id) patchLocal(p.id, { isPinned: !!p.pinned });
              break;
            case 'note.trashed':
            case 'note.purged':
            case 'trash.emptied':
              reload();
              break;
            case 'data.imported':
              reload();
              flash('导入完成');
              break;
            default:
              break;
          }
        }
      );
    })();
    return () => un?.();
  }, [reload, patchLocal, flash]);

  // ------------------------------------------------------------ 过滤 + 排序
  const visible = useMemo(() => {
    let list = allNotes;

    if (nav === 'trash') {
      list = list.filter((n) => n.deleted);
    } else {
      list = list.filter((n) => !n.deleted && !n.hidden);
      if (nav === 'pinned') list = list.filter((n) => n.isPinned);
    }

    if (colorFilter) list = list.filter((n) => n.color === colorFilter);

    const kw = keyword.trim().toLowerCase();
    if (kw) {
      list = list.filter(
        (n) =>
          n.content.toLowerCase().includes(kw) ||
          noteTitle(n).toLowerCase().includes(kw)
      );
    }

    const sorted = [...list];
    const cmp: Record<SortKey, (a: Note, b: Note) => number> = {
      updated: (a, b) => b.updatedAt.localeCompare(a.updatedAt),
      created: (a, b) => b.createdAt.localeCompare(a.createdAt),
      color: (a, b) => a.color.localeCompare(b.color),
      alpha: (a, b) => noteTitle(a).localeCompare(noteTitle(b)),
    };
    sorted.sort((a, b) => {
      if (a.isPinned !== b.isPinned) return a.isPinned ? -1 : 1;
      return cmp[sort](a, b);
    });
    return sorted;
  }, [allNotes, nav, keyword, sort, colorFilter]);

  // 统计
  const stats = useMemo(() => {
    const active = allNotes.filter((n) => !n.deleted);
    return {
      all: active.length,
      pinned: active.filter((n) => n.isPinned).length,
      trash: allNotes.filter((n) => n.deleted).length,
    };
  }, [allNotes]);

  // ---------------------------------------------------------------- 操作
  const handleNew = useCallback(async () => {
    try {
      const n = await notesApi.create({ color: settings?.defaultColor ?? 'yellow' }, true);
      if (n) {
        setAllNotes((prev) => [n, ...prev]);
        flash('已新建便签');
      }
    } catch (e) {
      console.error(e);
      flash('新建失败');
    }
  }, [settings, flash]);

  const handleOpen = useCallback(async (id: string) => {
    try {
      await notesApi.bringToFront(id);
      // 已打开则聚焦，未打开则创建窗口
      await ext.execute('window.openNote', { id });
    } catch (e) {
      console.error(e);
    }
  }, []);

  const handleSelect = useCallback((id: string, e: React.MouseEvent) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (e.ctrlKey || e.metaKey) {
        next.has(id) ? next.delete(id) : next.add(id);
      } else {
        next.clear();
        next.add(id);
      }
      return next;
    });
  }, []);

  const handleTrashSelected = useCallback(async () => {
    const ids = [...selected];
    for (const id of ids) {
      await notesApi.trash(id);
    }
    setSelected(new Set());
    await reload();
    flash(`已删除 ${ids.length} 条便签`);
  }, [selected, reload, flash]);

  const handleSetColor = useCallback(
    async (color: string) => {
      for (const id of selected) {
        await notesApi.setColor(id, color);
      }
      patchLocalMany([...selected], { color });
      flash('已更改颜色');
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [selected, flash]
  );

  const patchLocalMany = useCallback((ids: string[], patch: Partial<Note>) => {
    const set = new Set(ids);
    setAllNotes((prev) => prev.map((n) => (set.has(n.id) ? { ...n, ...patch } : n)));
  }, []);

  const handlePinSelected = useCallback(async () => {
    const ids = [...selected];
    // 若全是置顶则取消，否则全部置顶
    const allPinned = ids.every((id) => allNotes.find((n) => n.id === id)?.isPinned);
    for (const id of ids) {
      await notesApi.setPinned(id, !allPinned);
    }
    patchLocalMany(ids, { isPinned: !allPinned });
    setSelected(new Set());
    flash(allPinned ? '已取消置顶' : '已置顶');
  }, [selected, allNotes, patchLocalMany, flash]);

  const handlePurge = useCallback(async () => {
    for (const id of selected) {
      await notesApi.purge(id);
    }
    setSelected(new Set());
    await reload();
    flash('已彻底删除');
  }, [selected, reload, flash]);

  const handleEmptyTrash = useCallback(async () => {
    const n = await notesApi.purgeTrash();
    await reload();
    flash(`已清空回收站（${n} 条）`);
  }, [reload, flash]);

  /**
   * 便签汇总生成笔记。
   * 优先走插件（summary.notes，可自定义分组/模板），
   * 插件不可用时回退到内置的 doc.fromNotes。
   */
  const handleSummarize = useCallback(
    async (ids: string[]) => {
      const target = ids.length > 0 ? ids : visible.map((n) => n.id);
      if (target.length === 0) {
        flash('没有可汇总的便签');
        return;
      }
      const title = `${new Date().toLocaleDateString('zh-CN')} 便签汇总`;
      try {
        // 先试插件版本
        const r = await ext.execute<{ path?: string; noteCount?: number }>('summary.notes', {
          ids: target,
          title,
          folder: '便签汇总',
          groupBy: 'color',
        });
        const path = (r as { result?: { path?: string } })?.result?.path;
        flash(`已生成笔记${path ? `：${path}` : ''}`);
        setNav('docs');
        return;
      } catch (e) {
        // 插件没起来 -> 用内置实现
        console.warn('插件不可用，回退到内置实现', e);
      }
      try {
        const d = await docsApi.fromNotes(target, title, '便签汇总');
        flash(`已生成笔记：${d.title}`);
        setNav('docs');
      } catch (e) {
        console.error(e);
        flash(`生成失败：${String(e).slice(0, 60)}`);
      }
    },
    [visible, flash]
  );

  const handleSaveSettings = useCallback(
    async (next: Settings) => {
      try {
        const saved = await settingsApi.save(next);
        setSettings(saved);
        flash('设置已保存');
      } catch (e) {
        console.error(e);
        flash('保存设置失败');
      }
    },
    [flash]
  );

  // -------------------------------------------------------------- 快捷键
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key.toLowerCase() === 'n') {
        e.preventDefault();
        handleNew();
        return;
      }
      if (mod && e.key.toLowerCase() === 'f') {
        e.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
        return;
      }
      if (mod && e.key.toLowerCase() === 'a' && nav !== 'trash') {
        const target = e.target as HTMLElement;
        if (target.tagName === 'TEXTAREA' || target.tagName === 'INPUT') return;
        e.preventDefault();
        setSelected(new Set(visible.map((n) => n.id)));
        return;
      }
      if (e.key === 'Escape') {
        if (selected.size > 0) setSelected(new Set());
        else if (keyword) setKeyword('');
      }
      if (e.key === 'Delete' && selected.size > 0 && nav !== 'trash') {
        e.preventDefault();
        handleTrashSelected();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [handleNew, handleTrashSelected, nav, selected, visible, keyword]);

  // 最大化状态同步
  useEffect(() => {
    const h = getCurrentWindow();
    let un: (() => void) | undefined;
    (async () => {
      setMaximized(await h.isMaximized());
      un = await h.onResized(async () => {
        setMaximized(await h.isMaximized());
      });
    })();
    return () => un?.();
  }, []);

  const colorMap = useMemo(() => {
    const pack = themes.find((t) => t.id === settings?.themePack);
    return pack?.colors;
  }, [themes, settings]);

  // ------------------------------------------------------------------ 渲染
  const navItems: { key: NavKey; label: string; icon: React.ReactNode; count?: number }[] = [
    { key: 'all', label: '所有便签', icon: <IconNote size={16} />, count: stats.all },
    { key: 'pinned', label: '已置顶', icon: <IconPin size={16} />, count: stats.pinned },
    { key: 'trash', label: '回收站', icon: <IconTrash size={16} />, count: stats.trash },
  ];

  return (
    <div className="app-shell">
      {/* -------------------------------------------------------- 标题栏 */}
      <header className="titlebar" data-tauri-drag-region>
        <div className="titlebar__brand" data-tauri-drag-region>
          <span className="titlebar__logo" aria-hidden />
          <span className="titlebar__name">便签</span>
        </div>
        <div className="titlebar__drag" data-tauri-drag-region />
        <div className="titlebar__controls sn-no-drag">
          <button
            type="button"
            className="wc-btn"
            title="最小化"
            onClick={() => getCurrentWindow().minimize()}
          >
            <IconMinimize size={13} />
          </button>
          <button
            type="button"
            className="wc-btn"
            title={maximized ? '还原' : '最大化'}
            onClick={async () => {
              const w = getCurrentWindow();
              (await w.isMaximized()) ? w.unmaximize() : w.maximize();
            }}
          >
            {maximized ? <IconRestore size={13} /> : <IconMaximize size={13} />}
          </button>
          <button
            type="button"
            className="wc-btn wc-btn--close"
            title="关闭"
            onClick={() => getCurrentWindow().close()}
          >
            <IconClose size={13} />
          </button>
        </div>
      </header>

      <div className="app-main">
        {/* ------------------------------------------------------ 侧边栏 */}
        <nav className="sidebar">
          <button type="button" className="btn-new" onClick={handleNew}>
            <IconPlus size={16} />
            <span>新建便签</span>
          </button>

          <ul className="nav-list">
            {navItems.map((it) => (
              <li key={it.key}>
                <button
                  type="button"
                  className={`nav-item ${nav === it.key ? 'is-active' : ''}`}
                  onClick={() => {
                    setNav(it.key);
                    setSelected(new Set());
                  }}
                >
                  <span className="nav-item__icon">{it.icon}</span>
                  <span className="nav-item__label">{it.label}</span>
                  {!!it.count && <span className="nav-item__count">{it.count}</span>}
                </button>
              </li>
            ))}
          </ul>

          <div className="sidebar__divider" />

          <ul className="nav-list">
            <li>
              <button
                type="button"
                className={`nav-item ${nav === 'docs' ? 'is-active' : ''}`}
                onClick={() => setNav('docs')}
              >
                <span className="nav-item__icon">
                  <IconBook size={16} />
                </span>
                <span className="nav-item__label">笔记</span>
              </button>
            </li>
            <li>
              <button
                type="button"
                className={`nav-item ${nav === 'ext' ? 'is-active' : ''}`}
                onClick={() => setNav('ext')}
              >
                <span className="nav-item__icon">
                  <IconPuzzle size={16} />
                </span>
                <span className="nav-item__label">拓展</span>
              </button>
            </li>
            <li>
              <button
                type="button"
                className={`nav-item ${nav === 'settings' ? 'is-active' : ''}`}
                onClick={() => setNav('settings')}
              >
                <span className="nav-item__icon">
                  <IconSettings size={16} />
                </span>
                <span className="nav-item__label">设置</span>
              </button>
            </li>
          </ul>

          <div className="sidebar__foot">
            <button
              type="button"
              className="nav-item nav-item--ghost"
              onClick={async () => {
                await reload();
                flash('已刷新');
              }}
            >
              <span className="nav-item__icon">
                <IconRefresh size={16} />
              </span>
              <span className="nav-item__label">刷新</span>
            </button>
          </div>
        </nav>

        {/* -------------------------------------------------------- 内容区 */}
        <section className="content">
          {loadError && (
            <div className="load-error" role="alert">
              <span className="load-error__icon">!</span>
              <span className="load-error__text">{loadError}</span>
              <button
                type="button"
                className="load-error__retry"
                onClick={() => {
                  setLoadError(null);
                  setLoading(true);
                  void reload();
                  settingsApi
                    .get()
                    .then((s) => {
                      setSettings(s);
                      setLoadError(null);
                    })
                    .catch((e) => setLoadError(friendlyError(e, '设置')));
                }}
              >
                重试
              </button>
            </div>
          )}
          {nav === 'settings' ? (
            <SettingsPanel
              settings={settings}
              ready={ready}
              themes={themes}
              onSave={handleSaveSettings}
              onReloadThemes={async () => {
                const t = await ext.reloadThemes();
                setThemes(t);
                flash('主题已重载');
              }}
              onFlash={flash}
            />
          ) : nav === 'ext' ? (
            <ExtensionPanel onFlash={flash} />
          ) : nav === 'docs' ? (
            <NotesWorkspace onFlash={flash} />
          ) : (
            <>
              {/* 工具条 */}
              <div className="toolbar">
                <div className="search-box">
                  <IconSearch size={15} />
                  <input
                    ref={searchRef}
                    type="search"
                    placeholder="搜索便签"
                    value={keyword}
                    onChange={(e) => setKeyword(e.target.value)}
                  />
                  {keyword && (
                    <button
                      type="button"
                      className="search-box__clear"
                      title="清除"
                      onClick={() => setKeyword('')}
                    >
                      <IconClose size={13} />
                    </button>
                  )}
                </div>

                <div className="toolbar__right">
                  <button
                    type="button"
                    className="tb-btn tb-btn--accent"
                    title="把当前列表里的所有便签汇总成一篇笔记"
                    onClick={() => handleSummarize([])}
                  >
                    <IconSparkle size={15} />
                    <span>汇总成笔记</span>
                  </button>

                  <select
                    className="sort-select"
                    value={sort}
                    onChange={(e) => setSort(e.target.value as SortKey)}
                    title="排序方式"
                  >
                    <option value="updated">最近修改</option>
                    <option value="created">创建时间</option>
                    <option value="color">按颜色</option>
                    <option value="alpha">按标题</option>
                  </select>

                  <div className="view-switch" role="group" aria-label="视图切换">
                    <button
                      type="button"
                      className={`vs-btn ${view === 'grid' ? 'is-active' : ''}`}
                      title="网格视图"
                      onClick={() => setView('grid')}
                    >
                      <IconGrid size={15} />
                    </button>
                    <button
                      type="button"
                      className={`vs-btn ${view === 'list' ? 'is-active' : ''}`}
                      title="列表视图"
                      onClick={() => setView('list')}
                    >
                      <IconList size={15} />
                    </button>
                  </div>
                </div>
              </div>

              {/* 颜色筛选 */}
              <div className="filter-row">
                <button
                  type="button"
                  className={`chip ${colorFilter === null ? 'is-active' : ''}`}
                  onClick={() => setColorFilter(null)}
                >
                  全部
                </button>
                {COLORS.map((c) => (
                  <button
                    key={c}
                    type="button"
                    className={`chip chip--color ${colorFilter === c ? 'is-active' : ''}`}
                    data-color-chip={c}
                    style={colorMap?.[c] ? { background: colorMap[c] } : undefined}
                    title={`仅显示${c}`}
                    onClick={() => setColorFilter(colorFilter === c ? null : c)}
                  />
                ))}
                <span className="filter-row__info">
                  {visible.length} 条
                </span>
              </div>

              {/* 批量操作条 */}
              {selected.size > 0 && (
                <div className="bulkbar">
                  <span className="bulkbar__count">已选 {selected.size} 条</span>
                  <div className="bulkbar__actions">
                    {nav === 'trash' ? (
                      <>
                        <button
                          type="button"
                          className="bulk-btn"
                          onClick={async () => {
                            for (const id of selected) await notesApi.restore(id);
                            setSelected(new Set());
                            await reload();
                            flash('已恢复');
                          }}
                        >
                          恢复
                        </button>
                        <button type="button" className="bulk-btn is-danger" onClick={handlePurge}>
                          彻底删除
                        </button>
                      </>
                    ) : (
                      <>
                        <ColorPicker
                          value=""
                          onChange={handleSetColor}
                          colorMap={colorMap}
                          variant="panel"
                          align="left"
                        />
                        <button type="button" className="bulk-btn" onClick={handlePinSelected}>
                          {[...selected].every(
                            (id) => allNotes.find((n) => n.id === id)?.isPinned
                          )
                            ? '取消置顶'
                            : '置顶'}
                        </button>
                        <button
                          type="button"
                          className="bulk-btn bulk-btn--accent"
                          title="把选中的便签汇总成一篇笔记"
                          onClick={() => handleSummarize([...selected])}
                        >
                          汇总成笔记
                        </button>
                        <button
                          type="button"
                          className="bulk-btn is-danger"
                          onClick={handleTrashSelected}
                        >
                          删除
                        </button>
                      </>
                    )}
                    <button
                      type="button"
                      className="bulk-btn bulk-btn--ghost"
                      onClick={() => setSelected(new Set())}
                    >
                      取消
                    </button>
                  </div>
                </div>
              )}

              {/* 列表 / 空状态 */}
              {nav === 'trash' && stats.trash > 0 && (
                <div className="trash-actions">
                  <button type="button" className="link-btn is-danger" onClick={handleEmptyTrash}>
                    清空回收站
                  </button>
                  <span className="trash-actions__hint">回收站内容保留 30 天后自动清理</span>
                </div>
              )}

              {loading ? (
                <div className="empty-state">
                  <div className="spinner" />
                  <p>正在加载…</p>
                </div>
              ) : visible.length === 0 ? (
                <div className="empty-state">
                  <IconNote size={40} />
                  <p className="empty-state__title">
                    {keyword
                      ? '没有匹配的便签'
                      : nav === 'trash'
                      ? '回收站是空的'
                      : nav === 'pinned'
                      ? '还没有置顶的便签'
                      : '还没有便签'}
                  </p>
                  <p className="empty-state__desc">
                    {nav === 'all' && !keyword
                      ? '点击「新建便签」或按 Ctrl+N 开始记录'
                      : keyword
                      ? '换个关键词试试'
                      : ''}
                  </p>
                  {nav === 'all' && !keyword && (
                    <button type="button" className="btn-new btn-new--inline" onClick={handleNew}>
                      <IconPlus size={15} />
                      <span>新建便签</span>
                    </button>
                  )}
                </div>
              ) : (
                <div className={`note-area note-area--${view}`}>
                  {visible.map((n, i) => (
                    <div
                      key={n.id}
                      className={`card-slot ${selected.has(n.id) ? 'is-selected' : ''}`}
                      style={{ animationDelay: `${Math.min(i * 14, 220)}ms` }}
                      onClick={(e) => {
                        if (e.ctrlKey || e.metaKey || e.shiftKey) {
                          e.preventDefault();
                          handleSelect(n.id, e);
                        }
                      }}
                    >
                      <NoteCard
                        note={n}
                        view={view}
                        colorMap={colorMap}
                        trashed={nav === 'trash'}
                        onOpen={(id) => {
                          if (selected.size > 0) {
                            handleSelect(id, { ctrlKey: true } as React.MouseEvent);
                          } else {
                            handleOpen(id);
                          }
                        }}
                        onPin={async (id, pinned) => {
                          await notesApi.setPinned(id, pinned);
                          patchLocal(id, { isPinned: pinned });
                        }}
                        onTrash={async (id) => {
                          await notesApi.trash(id);
                          await reload();
                        }}
                        onDuplicate={async (id) => {
                          await notesApi.duplicate(id);
                          await reload();
                        }}
                        onHide={async (id) => {
                          await notesApi.setHidden(id, true);
                          await reload();
                          flash('已从列表中隐藏（便签窗口仍保留）');
                        }}
                        onRestore={async (id) => {
                          await notesApi.restore(id);
                          await reload();
                          flash('已恢复');
                        }}
                        onPurge={async (id) => {
                          await notesApi.purge(id);
                          await reload();
                        }}
                      />
                      {selected.has(n.id) && <span className="card-slot__check" aria-hidden />}
                    </div>
                  ))}
                </div>
              )}
            </>
          )}
        </section>
      </div>

      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}

/** 把后端错误转成一句人话，便于在界面上提示 */
function friendlyError(e: unknown, what: string): string {
  const raw =
    typeof e === 'string'
      ? e
      : e instanceof Error
      ? e.message
      : (() => {
          try {
            return JSON.stringify(e);
          } catch {
            return String(e);
          }
        })();
  if (/not found|未找到|unknown|不存在/i.test(raw)) {
    return `无法连接后端：${what}接口未注册（${raw}）`;
  }
  return `读取${what}失败：${raw || '未知错误'}`;
}

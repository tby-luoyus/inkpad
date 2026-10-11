/**
 * 笔记工作区 —— 对标 Obsidian
 *
 * 布局（左 → 右）：
 *   1. 文件树：文件夹 / 笔记，支持新建、重命名、删除
 *   2. 编辑器：Markdown 源码编辑 + 实时预览（可切换 编辑 / 分栏 / 阅读）
 *   3. 右侧栏：大纲、反向链接、出链、标签、属性（frontmatter）
 *
 * 交互对齐 Obsidian：
 *   - `[[` 触发笔记补全
 *   - 点击 [[链接]] 跳转；点击不存在的链接直接创建
 *   - `Ctrl+N` 新建笔记，`Ctrl+S` 保存，`Ctrl+E` 切换编辑/阅读
 *   - 笔记库就是磁盘上的普通文件夹，外部编辑器改动可通过「重新索引」同步
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  docs as docsApi,
  ext,
  onEvents,
  type Doc,
  type DocNode,
  type Backlink,
  type VaultInfo,
} from '../lib/api';
import { renderMarkdown, docStats, extractWikilinks } from '../lib/markdown';
import {
  IconPlus,
  IconSearch,
  IconClose,
  IconRefresh,
  IconTrash,
  IconNote,
  IconLink,
  IconFolder,
  IconCheck,
} from './Icons';
import './NotesWorkspace.css';

type ViewMode = 'edit' | 'split' | 'read';

interface Props {
  onFlash: (msg: string) => void;
}

export function NotesWorkspace({ onFlash }: Props) {
  const [tree, setTree] = useState<DocNode[]>([]);
  const [info, setInfo] = useState<VaultInfo | null>(null);
  const [active, setActive] = useState<Doc | null>(null);
  const [draftContent, setDraftContent] = useState('');
  const [dirty, setDirty] = useState(false);
  const [view, setView] = useState<ViewMode>('split');
  const [keyword, setKeyword] = useState('');
  const [results, setResults] = useState<Doc[] | null>(null);
  const [backlinks, setBacklinks] = useState<Backlink[]>([]);
  const [outlinks, setOutlinks] = useState<Doc[]>([]);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [renaming, setRenaming] = useState<{ id: string; value: string } | null>(null);
  const [showSuggest, setShowSuggest] = useState(false);
  const [suggestItems, setSuggestItems] = useState<DocNode[]>([]);

  const taRef = useRef<HTMLTextAreaElement>(null);
  const saveTimer = useRef<number | null>(null);
  /** 记录光标位置：用于 [[ 补全后插入内容 */
  const pendingInsert = useRef<number | null>(null);

  // -------------------------------------------------------------- 数据加载
  const refreshTree = useCallback(async () => {
    try {
      const [t, i] = await Promise.all([docsApi.tree(), docsApi.info()]);
      setTree(t);
      setInfo(i);
    } catch (e) {
      console.error('读取笔记库失败', e);
    }
  }, []);

  useEffect(() => {
    void refreshTree();
  }, [refreshTree]);

  // 后端事件：插件或外部 App 改了笔记 -> 刷新
  useEffect(() => {
    let un: (() => void) | undefined;
    (async () => {
      un = await onEvents(
        ['doc.created', 'doc.saved', 'doc.deleted', 'doc.renamed', 'doc.reindexed'],
        () => void refreshTree()
      );
    })();
    return () => un?.();
  }, [refreshTree]);

  // -------------------------------------------------------------- 选中笔记
  const openDoc = useCallback(
    async (idOrTitle: string) => {
      try {
        // 允许传「笔记名」直接打开（Obsidian 的 [[链接]] 语义）
        const d = await docsApi.get(idOrTitle);
        if (!d) {
          onFlash(`笔记不存在：${idOrTitle}`);
          return;
        }
        if (dirty) await flushSave();
        setActive(d);
        setDraftContent(d.content);
        setDirty(false);
        const [bl, ol] = await Promise.all([docsApi.backlinks(d.id), docsApi.outlinks(d.id)]);
        setBacklinks(bl);
        setOutlinks(ol);
      } catch (e) {
        console.error(e);
        onFlash('打开笔记失败');
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [dirty]
  );

  // -------------------------------------------------------------- 保存
  const doSave = useCallback(
    async (doc: Doc, content: string) => {
      try {
        const saved = await docsApi.save(doc.id, { content });
        setDirty(false);
        if (saved) {
          setActive((cur) => (cur && cur.id === saved.id ? saved : cur));
          setBacklinks(await docsApi.backlinks(saved.id));
          setOutlinks(await docsApi.outlinks(saved.id));
        }
        void refreshTree();
      } catch (e) {
        console.error('保存失败', e);
        onFlash('保存失败');
      }
    },
    [refreshTree, onFlash]
  );

  const flushSave = useCallback(async () => {
    if (!active) return;
    if (saveTimer.current) {
      window.clearTimeout(saveTimer.current);
      saveTimer.current = null;
    }
    await doSave(active, draftContent);
  }, [active, draftContent, doSave]);

  /** 输入后 700ms 自动保存（Obsidian 默认行为） */
  const scheduleSave = useCallback(
    (next: string) => {
      if (!active) return;
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
      saveTimer.current = window.setTimeout(() => {
        void doSave(active, next);
      }, 700);
    },
    [active, doSave]
  );

  const onEdit = (next: string) => {
    setDraftContent(next);
    setDirty(true);
    scheduleSave(next);
  };

  // -------------------------------------------------------------- 新建 / 删除
  const createDoc = useCallback(
    async (path?: string) => {
      const name = path ?? window.prompt('新笔记名称（可用 文件夹/名称）', '未命名笔记');
      if (!name) return;
      try {
        const d = await docsApi.create({ path: name, content: '' });
        await refreshTree();
        setActive(d);
        setDraftContent('');
        setDirty(false);
        setBacklinks([]);
        setOutlinks([]);
        onFlash(`已创建 ${d.title}`);
      } catch (e) {
        onFlash(`创建失败：${String(e).slice(0, 80)}`);
      }
    },
    [refreshTree, onFlash]
  );

  const deleteDoc = useCallback(
    async (doc: Doc) => {
      if (!window.confirm(`确定删除「${doc.title}」？文件会从磁盘移除。`)) return;
      await docsApi.remove(doc.id);
      if (active?.id === doc.id) {
        setActive(null);
        setDraftContent('');
      }
      await refreshTree();
      onFlash('已删除');
    },
    [active, refreshTree, onFlash]
  );

  const commitRename = useCallback(
    async () => {
      if (!renaming) return;
      const next = renaming.value.trim();
      setRenaming(null);
      if (!next) return;
      try {
        const d = await docsApi.rename(renaming.id, next);
        await refreshTree();
        if (d && active?.id === d.id) setActive(d);
        onFlash('已重命名');
      } catch (e) {
        onFlash(`重命名失败：${String(e).slice(0, 80)}`);
      }
    },
    [renaming, refreshTree, active, onFlash]
  );

  // -------------------------------------------------------------- 搜索
  useEffect(() => {
    const kw = keyword.trim();
    if (!kw) {
      setResults(null);
      return;
    }
    let alive = true;
    const t = window.setTimeout(async () => {
      const r = await docsApi.search(kw).catch(() => []);
      if (alive) setResults(r);
    }, 200);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [keyword]);

  // -------------------------------------------------------------- [[ 补全
  const allDocs = useMemo(() => flatten(tree), [tree]);

  const checkSuggest = useCallback(
    (value: string, caret: number) => {
      const upto = value.slice(0, caret);
      const m = upto.match(/\[\[([^\]\n]*)$/);
      if (!m) {
        setShowSuggest(false);
        return;
      }
      const q = m[1].toLowerCase();
      const items = allDocs.filter((d) => d.name.toLowerCase().includes(q)).slice(0, 8);
      setSuggestItems(items);
      setShowSuggest(items.length > 0);
      // 记录 [[ 的起始位置，便于替换
      pendingInsert.current = caret - m[1].length;
    },
    [allDocs]
  );

  const applySuggest = useCallback(
    (name: string) => {
      const ta = taRef.current;
      if (!ta || pendingInsert.current == null) return;
      const caret = ta.selectionStart;
      const before = draftContent.slice(0, pendingInsert.current);
      const after = draftContent.slice(caret);
      const next = `${before}${name}]]${after}`;
      onEdit(next);
      setShowSuggest(false);
      pendingInsert.current = null;
      const pos = before.length + name.length + 2;
      window.setTimeout(() => {
        ta.focus();
        ta.setSelectionRange(pos, pos);
      }, 0);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [draftContent]
  );

  // -------------------------------------------------------------- 快捷键
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key.toLowerCase() === 's') {
        e.preventDefault();
        void flushSave();
        onFlash('已保存');
        return;
      }
      if (mod && e.key.toLowerCase() === 'n') {
        const target = e.target as HTMLElement;
        if (target?.tagName === 'TEXTAREA' && !e.shiftKey) return;
        e.preventDefault();
        void createDoc();
        return;
      }
      if (mod && e.key.toLowerCase() === 'e') {
        e.preventDefault();
        setView((v) => (v === 'read' ? 'edit' : 'read'));
        return;
      }
      if (e.key === 'Escape' && showSuggest) {
        setShowSuggest(false);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [flushSave, createDoc, onFlash, showSuggest]);

  // -------------------------------------------------------------- 渲染
  const linkTargets = useMemo(() => new Set(allDocs.map((d) => d.name)), [allDocs]);

  const html = useMemo(
    () => renderMarkdown(draftContent, { linkTargets }),
    [draftContent, linkTargets]
  );

  const stats = useMemo(() => docStats(draftContent), [draftContent]);

  const outline = useMemo(() => {
    const out: { level: number; text: string }[] = [];
    for (const line of draftContent.split('\n')) {
      const m = line.match(/^(#{1,4})\s+(.*)$/);
      if (m) out.push({ level: m[1].length, text: m[2].trim() });
    }
    return out;
  }, [draftContent]);

  /** 预览区点击 [[链接]] / #标签 */
  const onPreviewClick = useCallback(
    (e: React.MouseEvent) => {
      const el = e.target as HTMLElement;
      const wl = el.closest('[data-wikilink]') as HTMLElement | null;
      if (wl) {
        e.preventDefault();
        const target = wl.dataset.wikilink!;
        void openDoc(target);
        return;
      }
      const tag = el.closest('[data-tag]') as HTMLElement | null;
      if (tag) {
        e.preventDefault();
        setKeyword(`#${tag.dataset.tag}`);
      }
    },
    [openDoc]
  );

  const toggleFolder = (path: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      next.has(path) ? next.delete(path) : next.add(path);
      return next;
    });
  };

  return (
    <div className="vault">
      {/* -------------------------------------------------------- 文件树 */}
      <aside className="vault-side">
        <div className="vault-side__head">
          <span className="vault-side__title">笔记库</span>
          <div className="vault-side__btns">
            <button type="button" className="btn-icon" title="新建笔记 (Ctrl+N)" onClick={() => createDoc()}>
              <IconPlus size={15} />
            </button>
            <button
              type="button"
              className="btn-icon"
              title="重新索引（同步外部编辑器改动）"
              onClick={async () => {
                const n = await docsApi.reindex();
                await refreshTree();
                onFlash(`已重建索引（${n} 篇）`);
              }}
            >
              <IconRefresh size={15} />
            </button>
            <button
              type="button"
              className="btn-icon"
              title="打开笔记库目录"
              onClick={() => ext.openDir('vault')}
            >
              <IconFolder size={15} />
            </button>
          </div>
        </div>

        <div className="vault-side__search">
          <IconSearch size={14} />
          <input
            type="search"
            placeholder="搜索笔记…"
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
          />
          {keyword && (
            <button type="button" className="btn-icon" title="清除" onClick={() => setKeyword('')}>
              <IconClose size={12} />
            </button>
          )}
        </div>

        <div className="vault-side__body">
          {results ? (
            <>
              <div className="vault-sec">搜索结果 · {results.length}</div>
              {results.map((d) => (
                <button
                  key={d.id}
                  type="button"
                  className={`vault-item ${active?.id === d.id ? 'is-active' : ''}`}
                  onClick={() => openDoc(d.id)}
                >
                  <IconNote size={14} />
                  <span className="vault-item__name">{d.title}</span>
                  <span className="vault-item__meta">{d.wordCount}</span>
                </button>
              ))}
            </>
          ) : (
            <>
              {tree.length === 0 && (
                <div className="vault-empty">
                  <IconNote size={26} />
                  <p>还没有笔记</p>
                  <button type="button" className="btn-primary btn-primary--sm" onClick={() => createDoc()}>
                    新建第一篇
                  </button>
                </div>
              )}
              {tree.map((n) => (
                <TreeNode
                  key={n.path}
                  node={n}
                  depth={0}
                  activeId={active?.id ?? null}
                  collapsed={collapsed}
                  onToggle={toggleFolder}
                  onOpen={openDoc}
                  onRename={(id, current) => setRenaming({ id, value: current })}
                  renaming={renaming}
                  setRenaming={setRenaming}
                  commitRename={commitRename}
                />
              ))}
              {info && info.tags.length > 0 && (
                <>
                  <div className="vault-sec">标签 · {info.tags.length}</div>
                  <div className="vault-tags">
                    {info.tags.slice(0, 30).map(([t, c]) => (
                      <button
                        key={t}
                        type="button"
                        className={`vault-tag ${keyword === `#${t}` ? 'is-active' : ''}`}
                        onClick={() => setKeyword(keyword === `#${t}` ? '' : `#${t}`)}
                      >
                        #{t}
                        <span>{c}</span>
                      </button>
                    ))}
                  </div>
                </>
              )}
            </>
          )}
        </div>

        {info && (
          <div className="vault-side__foot">
            <span>{info.docCount} 篇笔记</span>
            <span>{info.folderCount} 个文件夹</span>
            {info.orphans > 0 && <span title="没有任何链接进出的笔记">{info.orphans} 篇孤立</span>}
          </div>
        )}
      </aside>

      {/* ---------------------------------------------------------- 主区 */}
      {active ? (
        <>
          <main className="vault-main">
            <header className="vault-main__head">
              <div className="vault-main__title">
                <h2>{active.title}</h2>
                <span className="vault-main__path">{active.path}</span>
              </div>
              <div className="vault-main__actions">
                <div className="segmented segmented--sm">
                  {(
                    [
                      ['edit', '编辑'],
                      ['split', '分栏'],
                      ['read', '阅读'],
                    ] as const
                  ).map(([v, label]) => (
                    <button
                      key={v}
                      type="button"
                      className={`segmented__btn ${view === v ? 'is-active' : ''}`}
                      onClick={() => setView(v)}
                    >
                      {label}
                    </button>
                  ))}
                </div>
                <button
                  type="button"
                  className="btn-secondary btn-secondary--sm"
                  onClick={() => void flushSave()}
                  disabled={!dirty}
                >
                  {dirty ? '保存中…' : '已保存'}
                </button>
                <button
                  type="button"
                  className="btn-icon btn-icon--danger"
                  title="删除笔记"
                  onClick={() => deleteDoc(active)}
                >
                  <IconTrash size={15} />
                </button>
              </div>
            </header>

            <div className={`vault-editor vault-editor--${view}`}>
              {view !== 'read' && (
                <div className="vault-editor__pane">
                  <textarea
                    ref={taRef}
                    className="vault-textarea"
                    value={draftContent}
                    spellCheck={false}
                    placeholder="开始写点什么…支持 [[双向链接]] 与 #标签"
                    onChange={(e) => {
                      onEdit(e.target.value);
                      checkSuggest(e.target.value, e.target.selectionStart);
                    }}
                    onClick={(e) =>
                      checkSuggest(draftContent, (e.target as HTMLTextAreaElement).selectionStart)
                    }
                  />
                  {showSuggest && (
                    <ul className="vault-suggest">
                      {suggestItems.map((s) => (
                        <li key={s.path}>
                          <button type="button" onMouseDown={() => applySuggest(s.name)}>
                            {s.name}
                          </button>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              )}
              {view !== 'edit' && (
                <div
                  className="vault-editor__pane vault-preview"
                  onClick={onPreviewClick}
                  dangerouslySetInnerHTML={{ __html: html }}
                />
              )}
            </div>

            <footer className="vault-main__foot">
              <span>{stats.words} 字</span>
              <span>{stats.chars} 字符</span>
              <span>{stats.lines} 行</span>
              <span>约 {stats.minutes} 分钟</span>
              <span className="vault-main__foot-spacer" />
              <span>{active.tags.map((t) => `#${t}`).join(' ')}</span>
            </footer>
          </main>

          {/* ------------------------------------------------------ 右栏 */}
          <aside className="vault-right">
            <SideSection title="大纲" count={outline.length}>
              {outline.length === 0 ? (
                <p className="vault-muted">没有标题</p>
              ) : (
                <ul className="vault-outline">
                  {outline.map((o, i) => (
                    <li key={i} style={{ paddingLeft: (o.level - 1) * 12 }}>
                      {o.text}
                    </li>
                  ))}
                </ul>
              )}
            </SideSection>

            <SideSection title="反向链接" count={backlinks.length}>
              {backlinks.length === 0 ? (
                <p className="vault-muted">还没有笔记引用它</p>
              ) : (
                backlinks.map((b) => (
                  <button key={b.id} type="button" className="vault-linkcard" onClick={() => openDoc(b.id)}>
                    <span className="vault-linkcard__title">
                      <IconLink size={12} />
                      {b.title}
                    </span>
                    <span className="vault-linkcard__ctx">{b.context}</span>
                  </button>
                ))
              )}
            </SideSection>

            <SideSection title="出链" count={outlinks.length}>
              {outlinks.length === 0 ? (
                <p className="vault-muted">
                  没有引用其它笔记
                  {active.links.length > 0 && (
                    <>
                      <br />
                      待创建：{active.links.filter((l) => !linkTargets.has(l)).join('、')}
                    </>
                  )}
                </p>
              ) : (
                outlinks.map((d) => (
                  <button key={d.id} type="button" className="vault-linkcard" onClick={() => openDoc(d.id)}>
                    <span className="vault-linkcard__title">
                      <IconCheck size={12} />
                      {d.title}
                    </span>
                    <span className="vault-linkcard__ctx">{d.folder || '根目录'}</span>
                  </button>
                ))
              )}
            </SideSection>

            <SideSection title="属性" count={Object.keys(active.frontmatter ?? {}).length}>
              {Object.keys(active.frontmatter ?? {}).length === 0 ? (
                <p className="vault-muted">在正文顶部写 --- 可添加 frontmatter</p>
              ) : (
                <dl className="vault-props">
                  {Object.entries(active.frontmatter).map(([k, v]) => (
                    <div key={k}>
                      <dt>{k}</dt>
                      <dd>{Array.isArray(v) ? v.join(', ') : String(v)}</dd>
                    </div>
                  ))}
                </dl>
              )}
            </SideSection>
          </aside>
        </>
      ) : (
        <div className="vault-welcome">
          <IconNote size={44} />
          <h2>笔记库</h2>
          <p>
            这里是一个标准的 Markdown 文件夹，可以直接用 Obsidian、VS Code 或 Typora 打开。
            <br />
            支持 <code>[[双向链接]]</code>、<code>#标签</code>、YAML frontmatter 与实时预览。
          </p>
          <div className="vault-welcome__actions">
            <button type="button" className="btn-primary" onClick={() => createDoc()}>
              <IconPlus size={15} />
              新建笔记
            </button>
            <button type="button" className="btn-secondary" onClick={() => ext.openDir('vault')}>
              打开笔记库目录
            </button>
            <button
              type="button"
              className="btn-secondary"
              onClick={async () => {
                const list = await docsApi.list();
                if (list.length) {
                  void openDoc(list[0].id);
                } else {
                  onFlash('笔记库还是空的，先新建一篇');
                }
              }}
            >
              打开第一篇
            </button>
          </div>
          <div className="vault-welcome__hint">
            <kbd>Ctrl</kbd>+<kbd>N</kbd> 新建 · <kbd>Ctrl</kbd>+<kbd>S</kbd> 保存 ·{' '}
            <kbd>Ctrl</kbd>+<kbd>E</kbd> 切换阅读 · 输入 <code>[[</code> 触发笔记补全
          </div>
          {info && (
            <div className="vault-welcome__stats">
              <span>{info.docCount} 篇笔记</span>
              <span>{info.folderCount} 个文件夹</span>
              <span>{info.tags.length} 个标签</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/* ========================================================== 子组件 */

function SideSection({
  title,
  count,
  children,
}: {
  title: string;
  count?: number;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(true);
  return (
    <section className="vault-sec-card">
      <button type="button" className="vault-sec-card__head" onClick={() => setOpen((v) => !v)}>
        <span className={`vault-caret ${open ? 'is-open' : ''}`} />
        <span>{title}</span>
        {count != null && <span className="vault-sec-card__count">{count}</span>}
      </button>
      {open && <div className="vault-sec-card__body">{children}</div>}
    </section>
  );
}

function TreeNode({
  node,
  depth,
  activeId,
  collapsed,
  onToggle,
  onOpen,
  onRename,
  renaming,
  setRenaming,
  commitRename,
}: {
  node: DocNode;
  depth: number;
  activeId: string | null;
  collapsed: Set<string>;
  onToggle: (p: string) => void;
  onOpen: (id: string) => void;
  onRename: (id: string, current: string) => void;
  renaming: { id: string; value: string } | null;
  setRenaming: (v: { id: string; value: string } | null) => void;
  commitRename: () => void;
}) {
  const pad = { paddingLeft: 8 + depth * 14 };

  if (node.isDir) {
    const isCollapsed = collapsed.has(node.path);
    return (
      <div className="vault-node">
        <button type="button" className="vault-item vault-item--dir" style={pad} onClick={() => onToggle(node.path)}>
          <span className={`vault-caret ${isCollapsed ? '' : 'is-open'}`} />
          <span className="vault-item__name">{node.name}</span>
          <span className="vault-item__meta">{node.children.length}</span>
        </button>
        {!isCollapsed &&
          node.children.map((c) => (
            <TreeNode
              key={c.path}
              node={c}
              depth={depth + 1}
              activeId={activeId}
              collapsed={collapsed}
              onToggle={onToggle}
              onOpen={onOpen}
              onRename={onRename}
              renaming={renaming}
              setRenaming={setRenaming}
              commitRename={commitRename}
            />
          ))}
      </div>
    );
  }

  // 文件节点：双击触发重命名
  return (
    <div className="vault-node">
      <button
        type="button"
        className={`vault-item ${activeId === node.path ? 'is-active' : ''}`}
        style={pad}
        onClick={() => onOpen(node.path)}
        onDoubleClick={() => onRename(node.path, node.name)}
        title="双击重命名"
      >
        <IconNote size={14} />
        <span className="vault-item__name">{node.name}</span>
      </button>
    </div>
  );
}

/** 把树拍平成文件列表 */
function flatten(nodes: DocNode[], out: DocNode[] = []): DocNode[] {
  for (const n of nodes) {
    if (n.isDir) flatten(n.children, out);
    else out.push(n);
  }
  return out;
}

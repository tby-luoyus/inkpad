/**
 * 拓展面板
 *
 * 四类拓展接口的统一控制台：
 *  1. 插件     —— 启停、查看权限与订阅事件、打开插件目录
 *  2. 主题包   —— 浏览/重载，查看 CSS 变量与配色
 *  3. Agent    —— 命令列表（可复制调用示例），在线试跑
 *  4. 联动     —— 本地 HTTP 接口信息 + Webhook 管理 + 实时事件流
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  ext,
  onEvent,
  type CommandSpec,
  type PluginRuntime,
  type ThemePack,
  type AppEvent,
  type WebhookConfig,
  type ApiStatus,
  type PathsInfo,
} from '../lib/api';
import {
  IconPuzzle,
  IconPalette,
  IconBolt,
  IconLink,
  IconRefresh,
  IconPlus,
  IconTrash,
  IconCheck,
  IconPlay,
  IconStop,
  IconPulse,
  IconSparkle,
  IconEye,
} from './Icons';
import './ExtensionPanel.css';

type Tab = 'plugins' | 'themes' | 'agent' | 'integration';

interface Props {
  onFlash: (msg: string) => void;
}

/** 给任意 Promise 套超时，超时按 reject 处理，避免界面永远停在加载态 */
function withTimeout<T>(p: Promise<T>, ms: number, label: string): Promise<T> {
  return Promise.race([
    p,
    new Promise<never>((_, reject) =>
      setTimeout(() => reject(new Error(`${label}读取超时（>${ms}ms）`)), ms)
    ),
  ]);
}

export function ExtensionPanel({ onFlash }: Props) {
  const [tab, setTab] = useState<Tab>('plugins');
  const [plugins, setPlugins] = useState<PluginRuntime[]>([]);
  const [themes, setThemes] = useState<ThemePack[]>([]);
  const [commands, setCommands] = useState<CommandSpec[]>([]);
  const [paths, setPaths] = useState<PathsInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string>('');

  const loadAll = useCallback(async () => {
    setLoading(true);
    setLoadError('');
    // 任一请求卡住时不能让整页永远停在「正在读取…」：给每个请求套一个超时，
    // 超时按失败处理并显示在页面上，同时保留「重试」入口。
    const results = await Promise.allSettled([
      withTimeout(ext.plugins(), 8000, '插件'),
      withTimeout(ext.themes(), 8000, '主题'),
      withTimeout(ext.commands(), 8000, '命令'),
      withTimeout(ext.paths(), 8000, '路径'),
    ]);
    const errs: string[] = [];
    const [p, t, c, pa] = results;
    if (p.status === 'fulfilled') setPlugins(p.value);
    else errs.push(`插件: ${String(p.reason).slice(0, 80)}`);
    if (t.status === 'fulfilled') setThemes(t.value);
    else errs.push(`主题: ${String(t.reason).slice(0, 80)}`);
    if (c.status === 'fulfilled') setCommands(c.value);
    else errs.push(`命令: ${String(c.reason).slice(0, 80)}`);
    if (pa.status === 'fulfilled') setPaths(pa.value);
    else errs.push(`路径: ${String(pa.reason).slice(0, 80)}`);

    setLoadError(errs.join('；'));
    setLoading(false);
  }, []);

  useEffect(() => {
    loadAll();
  }, [loadAll]);

  const tabs: { key: Tab; label: string; icon: React.ReactNode; badge?: number }[] = [
    { key: 'plugins', label: '插件', icon: <IconPuzzle size={14} />, badge: plugins.length },
    { key: 'themes', label: '主题包', icon: <IconPalette size={14} />, badge: themes.length },
    { key: 'agent', label: 'Agent 命令', icon: <IconBolt size={14} />, badge: commands.length },
    { key: 'integration', label: 'App 联动', icon: <IconLink size={14} /> },
  ];

  if (loading) {
    return (
      <div className="panel">
        <div className="panel__loading">正在读取拓展信息…</div>
      </div>
    );
  }

  if (loadError && !plugins.length && !themes.length && !commands.length) {
    return (
      <div className="panel">
        <header className="panel__head">
          <h2 className="panel__title">拓展</h2>
        </header>
        <div className="empty-state">
          <p className="empty-state__title">拓展信息读取失败</p>
          <p className="empty-state__desc">{loadError}</p>
          <button type="button" className="btn-primary" onClick={() => loadAll()}>
            重试
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="panel">
      <header className="panel__head">
        <h2 className="panel__title">拓展</h2>
        <div className="ext-tabs">
          {tabs.map((t) => (
            <button
              key={t.key}
              type="button"
              className={`ext-tab ${tab === t.key ? 'is-active' : ''}`}
              onClick={() => setTab(t.key)}
            >
              {t.icon}
              <span>{t.label}</span>
              {!!t.badge && <span className="ext-tab__badge">{t.badge}</span>}
            </button>
          ))}
        </div>
        <button
          type="button"
          className="btn-icon"
          title="重新扫描"
          onClick={async () => {
            await loadAll();
            onFlash('已重新扫描');
          }}
        >
          <IconRefresh size={15} />
        </button>
      </header>

      <div className="panel__scroll">
        {tab === 'plugins' && (
          <PluginsTab plugins={plugins} paths={paths} onChanged={loadAll} onFlash={onFlash} />
        )}
        {tab === 'themes' && (
          <ThemesTab
            themes={themes}
            paths={paths}
            onChanged={async () => {
              setThemes(await ext.reloadThemes());
            }}
            onFlash={onFlash}
          />
        )}
        {tab === 'agent' && <AgentTab commands={commands} onFlash={onFlash} />}
        {tab === 'integration' && <IntegrationTab paths={paths} onFlash={onFlash} />}
      </div>
    </div>
  );
}

/* ============================================================== 插件 */

function PluginsTab({
  plugins,
  paths,
  onChanged,
  onFlash,
}: {
  plugins: PluginRuntime[];
  paths: PathsInfo | null;
  onChanged: () => Promise<void>;
  onFlash: (m: string) => void;
}) {
  // 正在查看日志的插件 id
  const [logOpen, setLogOpen] = useState<string | null>(null);
  const [logText, setLogText] = useState('');
  const [logPath, setLogPath] = useState('');
  const [logMissing, setLogMissing] = useState(false);

  const openLog = async (id: string) => {
    try {
      const r = await ext.pluginLog(id, 200);
      setLogOpen(id);
      setLogText(r.text || '(日志为空)');
      setLogPath(r.path);
      setLogMissing(!r.exists);
    } catch (e) {
      onFlash(`读取日志失败：${String(e).slice(0, 60)}`);
    }
  };

  return (
    <>
      <div className="hint-card">
        <p>
          插件通过 HTTP 与便签通信，可用任意语言编写。在插件目录新建
          <code>&lt;plugin-id&gt;/plugin.json</code> 即可被识别。
        </p>
        <p className="hint-card__note">
          提示：插件后端通过本机 HTTP 接口读写便签。若插件报「无法连接 127.0.0.1」，
          请到「设置 → 开发者」确认已启用本地 HTTP API。
        </p>
        <div className="hint-card__actions">
          <button
            type="button"
            className="btn-secondary"
            onClick={async () => {
              await ext.openDir('plugins');
              onFlash('已打开插件目录');
            }}
          >
            打开插件目录
          </button>
          <button
            type="button"
            className="btn-secondary"
            onClick={async () => {
              await ext.reloadPlugins();
              await onChanged();
              onFlash('插件已重载');
            }}
          >
            <IconRefresh size={14} />
            重新加载
          </button>
        </div>
        {paths && <code className="path-line">{paths.plugins}</code>}
      </div>

      {plugins.length === 0 ? (
        <div className="empty-block">
          <IconPuzzle size={30} />
          <p>还没有插件</p>
          <span>在插件目录放入 plugin.json 后点击「重新加载」</span>
        </div>
      ) : (
        plugins.map((p) => (
          <section key={p.id} className="card">
            <div className="card-head">
              <div className="card-head__text">
                <h3 className="card__title">{p.name}</h3>
                <span className="card__sub">
                  {p.id} · v{p.version}
                  {p.author && ` · ${p.author}`}
                </span>
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={p.enabled}
                className={`switch ${p.enabled ? 'is-on' : ''}`}
                onClick={async () => {
                  await ext.setPluginEnabled(p.id, !p.enabled);
                  await onChanged();
                  onFlash(p.enabled ? '插件已停用' : '插件已启用');
                }}
              >
                <span className="switch__knob" />
              </button>
            </div>

            {p.description && <p className="card__hint">{p.description}</p>}

            <dl className="kv">
              <dt>事件订阅</dt>
              <dd>
                {p.events.length ? (
                  <span className="tag-row">
                    {p.events.map((e) => (
                      <span key={e} className="tag">
                        {e}
                      </span>
                    ))}
                  </span>
                ) : (
                  <span className="kv__empty">无</span>
                )}
              </dd>
              <dt>注册命令</dt>
              <dd>
                {p.commands.length ? (
                  <span className="tag-row">
                    {p.commands.map((c) => (
                      <span key={c.name} className="tag tag--cmd">
                        {c.name}
                      </span>
                    ))}
                  </span>
                ) : (
                  <span className="kv__empty">无</span>
                )}
              </dd>
              <dt>权限声明</dt>
              <dd>
                {p.permissions.length ? (
                  <span className="tag-row">
                    {p.permissions.map((x) => (
                      <span key={x} className="tag tag--perm">
                        {x}
                      </span>
                    ))}
                  </span>
                ) : (
                  <span className="kv__empty">未声明</span>
                )}
              </dd>
              <dt>后端地址</dt>
              <dd>
                <code>{p.endpoint ?? '（纯前端插件）'}</code>
              </dd>
              <dt>进程状态</dt>
              <dd>
                {p.autoStart ? (
                  <span className="proc-line">
                    <span
                      className={`dot ${p.processRunning ? 'is-ok' : 'is-off'}`}
                      title={p.processRunning ? '进程运行中' : '进程未运行'}
                    />
                    <span className="proc-line__text">
                      {p.processRunning ? '运行中' : '未运行'}
                    </span>
                    <code className="proc-line__cmd">
                      {p.autoStart.command} {(p.autoStart.args ?? []).join(' ')}
                    </code>
                  </span>
                ) : (
                  <span className="kv__empty">未配置 autoStart（需手动启动服务）</span>
                )}
              </dd>
            </dl>

            <div className="card-actions">
              {p.autoStart && (
                <>
                  <button
                    type="button"
                    className="btn-secondary"
                    disabled={p.processRunning}
                    title="启动插件进程"
                    onClick={async () => {
                      try {
                        const ok = await ext.startPlugin(p.id);
                        await onChanged();
                        onFlash(ok ? '插件进程已启动' : '启动失败，请查看日志');
                      } catch (e) {
                        onFlash(`启动失败：${String(e).slice(0, 60)}`);
                      }
                    }}
                  >
                    <IconPlay size={14} />
                    启动
                  </button>
                  <button
                    type="button"
                    className="btn-secondary"
                    disabled={!p.processRunning}
                    title="终止插件进程"
                    onClick={async () => {
                      try {
                        await ext.stopPlugin(p.id);
                        await onChanged();
                        onFlash('插件进程已停止');
                      } catch (e) {
                        onFlash(`停止失败：${String(e).slice(0, 60)}`);
                      }
                    }}
                  >
                    <IconStop size={14} />
                    停止
                  </button>
                </>
              )}
              {p.endpoint && (
                <button
                  type="button"
                  className="btn-secondary"
                  title="发送健康检查请求"
                  onClick={async () => {
                    try {
                      const r = await ext.pingPlugin(p.id);
                      onFlash(
                        r.reachable
                          ? `插件响应正常（HTTP ${r.status ?? 200}）`
                          : `无响应：${r.reason ?? '连接失败'}`
                      );
                    } catch (e) {
                      onFlash(`探测失败：${String(e).slice(0, 60)}`);
                    }
                  }}
                >
                  <IconPulse size={14} />
                  探测
                </button>
              )}
              {p.autoStart && (
                <button
                  type="button"
                  className="btn-secondary"
                  title="查看插件后端进程的输出日志"
                  onClick={() => openLog(p.id)}
                >
                  <IconEye size={14} />
                  查看日志
                </button>
              )}
              <button
                type="button"
                className="btn-secondary"
                title="在拓展 → Agent 中调用该插件的命令"
                onClick={() => onFlash(`可在「Agent」页签调用${p.commands[0]?.name ?? '该插件的命令'}`)}
              >
                <IconSparkle size={14} />
                如何调用
              </button>
            </div>

            {logOpen === p.id && (
              <div className="plugin-log">
                <div className="plugin-log__head">
                  <span className="plugin-log__title">
                    后端日志{logMissing && '（暂无输出）'}
                  </span>
                  <div className="plugin-log__actions">
                    <button
                      type="button"
                      className="link-btn"
                      onClick={() => openLog(p.id)}
                      title="重新读取"
                    >
                      刷新
                    </button>
                    <button
                      type="button"
                      className="link-btn"
                      onClick={async () => {
                        await ext.openDir('logs');
                        onFlash('已打开日志目录');
                      }}
                    >
                      打开目录
                    </button>
                    <button
                      type="button"
                      className="link-btn"
                      onClick={() => setLogOpen(null)}
                    >
                      收起
                    </button>
                  </div>
                </div>
                <pre className="plugin-log__body">{logText}</pre>
                <code className="plugin-log__path">{logPath}</code>
              </div>
            )}
          </section>
        ))
      )}
    </>
  );
}

/* ============================================================== 主题包 */

function ThemesTab({
  themes,
  paths,
  onChanged,
  onFlash,
}: {
  themes: ThemePack[];
  paths: PathsInfo | null;
  onChanged: () => Promise<void>;
  onFlash: (m: string) => void;
}) {
  return (
    <>
      <div className="hint-card">
        <p>
          主题包放在主题目录下的 <code>&lt;theme-id&gt;/theme.json</code>，可覆盖六色配色与任意 CSS
          变量，还可指定附加样式表与自定义卡片渲染器。
        </p>
        <div className="hint-card__actions">
          <button
            type="button"
            className="btn-secondary"
            onClick={async () => {
              await ext.openDir('themes');
              onFlash('已打开主题目录');
            }}
          >
            打开主题目录
          </button>
          <button
            type="button"
            className="btn-secondary"
            onClick={async () => {
              await onChanged();
              onFlash('主题已重载');
            }}
          >
            <IconRefresh size={14} />
            重新加载
          </button>
        </div>
        {paths && <code className="path-line">{paths.themes}</code>}
      </div>

      {themes.map((t) => (
        <section key={t.id} className="card">
          <div className="card-head">
            <div className="card-head__text">
              <h3 className="card__title">
                {t.name}
                {t.builtin && <span className="pill">内置</span>}
              </h3>
              <span className="card__sub">
                {t.id} · v{t.version} · {t.appearance}
              </span>
            </div>
          </div>
          {t.description && <p className="card__hint">{t.description}</p>}

          <div className="swatch-strip">
            {Object.entries(t.colors ?? {}).map(([k, v]) => (
              <span
                key={k}
                className="swatch-strip__item"
                style={{ background: v }}
                title={`${k} ${v}`}
              />
            ))}
          </div>

          {Object.keys(t.cssVars ?? {}).length > 0 && (
            <details className="details">
              <summary>CSS 变量（{Object.keys(t.cssVars).length}）</summary>
              <pre>{Object.entries(t.cssVars).map(([k, v]) => `${k}: ${v};`).join('\n')}</pre>
            </details>
          )}

          {t.renderer && (
            <p className="card__sub">
              自定义渲染器：<code>{t.renderer}</code>
            </p>
          )}
        </section>
      ))}
    </>
  );
}

/* ============================================================== Agent */

function AgentTab({
  commands,
  onFlash,
}: {
  commands: CommandSpec[];
  onFlash: (m: string) => void;
}) {
  const [filter, setFilter] = useState('');
  const [selected, setSelected] = useState<CommandSpec | null>(null);
  const [argsText, setArgsText] = useState('{}');
  const [running, setRunning] = useState(false);
  const [output, setOutput] = useState<string | null>(null);

  const groups = useMemo(() => {
    const kw = filter.trim().toLowerCase();
    const list = commands.filter(
      (c) =>
        !kw ||
        c.name.toLowerCase().includes(kw) ||
        c.description.toLowerCase().includes(kw)
    );
    const map = new Map<string, CommandSpec[]>();
    for (const c of list) {
      const key = c.category.split(':')[0];
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(c);
    }
    return [...map.entries()];
  }, [commands, filter]);

  const categoryLabel: Record<string, string> = {
    notes: '便签',
    window: '窗口',
    system: '系统',
    theme: '主题',
    plugin: '插件',
  };

  const run = async () => {
    if (!selected) return;
    setRunning(true);
    setOutput(null);
    try {
      let args: Record<string, unknown> = {};
      try {
        args = JSON.parse(argsText || '{}');
      } catch {
        throw new Error('参数不是合法 JSON');
      }
      const r = await ext.execute(selected.name, args);
      setOutput(JSON.stringify(r, null, 2));
    } catch (e) {
      setOutput(`错误：${String(e)}`);
    } finally {
      setRunning(false);
    }
  };

  return (
    <>
      <div className="hint-card">
        <p>
          所有能力都收敛到一个命令入口。Agent、插件、外部脚本调用方式完全一致，
          保证行为可预测。
        </p>
        <code className="path-line">
          IPC: invoke(&quot;ext_execute&quot;, {'{'} command, args {'}'})
          {pathsLine()}
        </code>
      </div>

      <div className="agent-layout">
        <div className="agent-list">
          <input
            className="field-input"
            placeholder="筛选命令…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          {groups.map(([cat, list]) => (
            <div key={cat} className="agent-group">
              <span className="agent-group__title">{categoryLabel[cat] ?? cat}</span>
              {list.map((c) => (
                <button
                  key={c.name}
                  type="button"
                  className={`agent-cmd ${selected?.name === c.name ? 'is-active' : ''}`}
                  onClick={() => {
                    setSelected(c);
                    setOutput(null);
                    const tmpl = templateFor(c);
                    setArgsText(JSON.stringify(tmpl, null, 2));
                  }}
                >
                  <code>{c.name}</code>
                  <span>{c.description}</span>
                </button>
              ))}
            </div>
          ))}
        </div>

        <div className="agent-run">
          {selected ? (
            <>
              <h3 className="card__title">
                <code>{selected.name}</code>
              </h3>
              <p className="card__hint">{selected.description}</p>
              <label className="field-label">参数（JSON）</label>
              <textarea
                className="field-textarea"
                value={argsText}
                spellCheck={false}
                rows={8}
                onChange={(e) => setArgsText(e.target.value)}
              />
              <div className="agent-run__actions">
                <button
                  type="button"
                  className="btn-primary"
                  onClick={run}
                  disabled={running}
                >
                  {running ? '执行中…' : '执行'}
                </button>
                <button
                  type="button"
                  className="btn-secondary"
                  onClick={async () => {
                    const snippet = `await window.__sticky.execute(${JSON.stringify(
                      selected.name
                    )}, ${argsText})`;
                    await navigator.clipboard.writeText(snippet);
                    onFlash('已复制调用代码');
                  }}
                >
                  复制调用代码
                </button>
              </div>
              {output && <pre className="agent-out">{output}</pre>}
            </>
          ) : (
            <div className="empty-block empty-block--sm">
              <IconBolt size={26} />
              <p>选择左侧命令查看详情</p>
              <span>可直接在右侧试跑，验证参数与返回结构</span>
            </div>
          )}
        </div>
      </div>
    </>
  );
}

function pathsLine() {
  return null;
}

function templateFor(c: CommandSpec): Record<string, unknown> {
  const p = (c.params ?? {}) as Record<string, string>;
  const out: Record<string, unknown> = {};
  for (const [k, type] of Object.entries(p)) {
    const optional = typeof type === 'string' && type.endsWith('?');
    if (optional) continue;
    if (type === 'string') out[k] = '';
    else if (type === 'bool') out[k] = false;
    else if (type === 'number') out[k] = 0;
    else out[k] = {};
  }
  // 便捷默认：便签类命令给个占位 id
  if (typeof p.id === 'string' && !p.id.endsWith('?')) out.id = '<noteId>';
  return out;
}

/* ============================================================ App 联动 */

function IntegrationTab({
  paths,
  onFlash,
}: {
  paths: PathsInfo | null;
  onFlash: (m: string) => void;
}) {
  const [status, setStatus] = useState<ApiStatus | null>(null);
  const [port, setPort] = useState(43919);
  const [hooks, setHooks] = useState<WebhookConfig[]>([]);
  const [events, setEvents] = useState<AppEvent[]>([]);
  const [url, setUrl] = useState('');
  const [pattern, setPattern] = useState('note.*');
  const liveRef = useRef<AppEvent[]>([]);

  const refresh = useCallback(async () => {
    const [s, h, e] = await Promise.all([
      ext.apiStatus(),
      ext.webhooks(),
      ext.eventsRecent(40),
    ]);
    setStatus(s);
    setHooks(h);
    setEvents(e);
    if (s.port) setPort(s.port);
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // 实时事件流
  useEffect(() => {
    let un: (() => void) | undefined;
    (async () => {
      un = await onEvent<AppEvent>('__noop__', () => {});
      // 订阅所有已知便签事件（前端无法用通配符，逐个订阅常用事件）
      const names = [
        'note.created',
        'note.updated',
        'note.trashed',
        'note.restored',
        'note.purged',
        'note.colorChanged',
        'note.pinnedChanged',
        'note.focused',
        'note.closed',
        'trash.emptied',
        'settings.changed',
        'theme.changed',
        'plugin.toggled',
        'data.imported',
        'api.started',
      ];
      const { onEvents } = await import('../lib/api');
      const un2 = await onEvents(names, (_n, payload) => {
        const ev = payload as AppEvent;
        liveRef.current = [ev, ...liveRef.current].slice(0, 60);
        setEvents([...liveRef.current]);
      });
      un?.();
      un = un2;
    })();
    return () => un?.();
  }, []);

  const base = status?.baseUrl ?? `http://127.0.0.1:${port}/api/v1`;

  return (
    <>
      {/* 服务状态 */}
      <section className="card">
        <div className="card-head">
          <div className="card-head__text">
            <h3 className="card__title">本地 HTTP 接口</h3>
            <span className="card__sub">多 App 联动的主通道，仅监听 127.0.0.1</span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={!!status?.running}
            className={`switch ${status?.running ? 'is-on' : ''}`}
            onClick={async () => {
              const s = await ext.setApiEnabled(!status?.running, port);
              setStatus(s);
              onFlash(s.running ? `接口已启动于 ${s.port}` : '接口已停止');
            }}
          >
            <span className="switch__knob" />
          </button>
        </div>

        <div className="row">
          <div className="row__text">
            <span className="row__label">端口</span>
            <span className="row__desc">修改后需重新开关接口</span>
          </div>
          <input
            className="field-num"
            type="number"
            min={1024}
            max={65535}
            value={port}
            onChange={(e) => setPort(Number(e.target.value) || 43919)}
          />
        </div>

        <div className="api-hint">
          <div className="api-hint__line">
            <span className="api-hint__dot" data-on={status?.running ? 'yes' : 'no'} />
            <code>{base}</code>
          </div>
          {status?.token && (
            <div className="api-hint__line api-hint__line--muted">
              <span>Token</span>
              <code>{status.token}</code>
            </div>
          )}
          <pre className="api-hint__code">{`GET    ${base}/notes            # 列出便签
POST   ${base}/notes            # 新建便签
PATCH  ${base}/notes/:id        # 更新便签
POST   ${base}/notes/:id/append # 追加内容
DELETE ${base}/notes/:id        # 移入回收站
GET    ${base}/search?q=关键词  # 搜索
POST   ${base}/execute          # 通用命令入口
GET    ${base}/events/subscribe # SSE 实时事件流
GET    ${base}/capabilities     # 全部能力清单`}</pre>
        </div>
      </section>

      {/* Webhook */}
      <section className="card">
        <h3 className="card__title">事件 Webhook</h3>
        <p className="card__hint">
          便签发生变更时，把事件 POST 到你配置的地址。事件名支持前缀通配，如
          <code>note.*</code> 或 <code>*</code>。
        </p>

        <div className="hook-form">
          <input
            className="field-input"
            placeholder="https://your-service/hook"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
          />
          <input
            className="field-input field-input--sm"
            placeholder="note.*"
            value={pattern}
            onChange={(e) => setPattern(e.target.value)}
          />
          <button
            type="button"
            className="btn-primary"
            disabled={!url.trim()}
            onClick={async () => {
              const cfg: WebhookConfig = {
                id: `wh-${Date.now().toString(36)}`,
                name: new URL(url).host || 'webhook',
                url: url.trim(),
                events: pattern
                  .split(',')
                  .map((s) => s.trim())
                  .filter(Boolean),
                enabled: true,
                headers: {},
              };
              await ext.upsertWebhook(cfg);
              setUrl('');
              await refresh();
              onFlash('Webhook 已添加');
            }}
          >
            <IconPlus size={14} />
            添加
          </button>
        </div>

        {hooks.length === 0 ? (
          <p className="kv__empty">暂无 Webhook</p>
        ) : (
          <ul className="hook-list">
            {hooks.map((h) => (
              <li key={h.id} className="hook-item">
                <div className="hook-item__text">
                  <code>{h.url}</code>
                  <span className="tag-row">
                    {h.events.map((e) => (
                      <span key={e} className="tag">
                        {e}
                      </span>
                    ))}
                  </span>
                </div>
                <button
                  type="button"
                  className="btn-icon btn-icon--danger"
                  title="删除"
                  onClick={async () => {
                    await ext.removeWebhook(h.id);
                    await refresh();
                    onFlash('Webhook 已删除');
                  }}
                >
                  <IconTrash size={14} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      {/* 实时事件 */}
      <section className="card">
        <div className="card-head">
          <div className="card-head__text">
            <h3 className="card__title">实时事件流</h3>
            <span className="card__sub">便签一切变更都会出现在这里</span>
          </div>
          <button type="button" className="btn-icon" title="刷新" onClick={refresh}>
            <IconRefresh size={15} />
          </button>
        </div>

        {events.length === 0 ? (
          <p className="kv__empty">暂无事件</p>
        ) : (
          <ul className="event-list">
            {events.map((e, i) => (
              <li key={`${e.at}-${i}`} className="event-item">
                <span className="event-item__name">
                  <IconCheck size={11} />
                  {e.name}
                </span>
                <span className="event-item__src">{e.source}</span>
                <time className="event-item__time">
                  {new Date(e.at).toLocaleTimeString('zh-CN')}
                </time>
                <code className="event-item__payload">
                  {JSON.stringify(e.payload).slice(0, 120)}
                </code>
              </li>
            ))}
          </ul>
        )}
      </section>

      {paths && (
        <section className="card">
          <h3 className="card__title">路径</h3>
          <dl className="kv">
            <dt>数据目录</dt>
            <dd>
              <code>{paths.data}</code>
            </dd>
            <dt>数据库</dt>
            <dd>
              <code>{paths.database}</code>
            </dd>
            <dt>插件</dt>
            <dd>
              <code>{paths.plugins}</code>
            </dd>
            <dt>主题</dt>
            <dd>
              <code>{paths.themes}</code>
            </dd>
            <dt>笔记库</dt>
            <dd>
              <code>{paths.vault}</code>
            </dd>
            <dt>插件日志</dt>
            <dd>
              <code>{paths.logs}</code>
            </dd>
            <dt>备份</dt>
            <dd>
              <code>{paths.backups}</code>
            </dd>
          </dl>
          <div className="card-actions">
            <button
              type="button"
              className="btn-secondary"
              onClick={async () => {
                await ext.openDir('vault');
                onFlash('已打开笔记库');
              }}
            >
              打开笔记库
            </button>
            <button
              type="button"
              className="btn-secondary"
              onClick={async () => {
                await ext.openDir('plugins');
                onFlash('已打开插件目录');
              }}
            >
              打开插件目录
            </button>
          </div>
        </section>
      )}
    </>
  );
}

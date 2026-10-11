/**
 * 设置面板
 *
 * 分组呈现：外观 / 行为 / 数据 / 开发者。所有改动即时保存。
 */

import { useEffect, useState } from 'react';
import {
  settings as settingsApi,
  backup,
  ext,
  COLORS,
  COLOR_LABELS,
  type Settings,
  type ThemePack,
} from '../lib/api';
import { IconRefresh, IconArchive, IconBolt } from './Icons';
import './SettingsPanel.css';

interface Props {
  settings: Settings | null;
  themes: ThemePack[];
  /** 首屏加载是否已结束（用于区分「还在加载」和「加载失败」） */
  ready?: boolean;
  onSave: (s: Settings) => Promise<void>;
  onReloadThemes: () => Promise<void>;
  onFlash: (msg: string) => void;
}

export function SettingsPanel({
  settings,
  themes,
  ready = true,
  onSave,
  onReloadThemes,
  onFlash,
}: Props) {
  const [draft, setDraft] = useState<Settings | null>(settings);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setDraft(settings);
  }, [settings]);

  if (!draft) {
    return (
      <div className="panel">
        <div className="panel__loading">
          {ready ? '设置读取失败，请在上方点击「重试」' : '正在读取设置…'}
        </div>
      </div>
    );
  }

  /** 改动即保存（桌面应用的常规体验） */
  const set = async <K extends keyof Settings>(key: K, value: Settings[K]) => {
    const next = { ...draft, [key]: value };
    setDraft(next);
    setSaving(true);
    try {
      await onSave(next);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="panel">
      <header className="panel__head">
        <h2 className="panel__title">设置</h2>
        <span className={`panel__saving ${saving ? 'is-on' : ''}`}>
          {saving ? '保存中…' : '已保存'}
        </span>
      </header>

      <div className="panel__scroll">
        {/* ---------------------------------------------------- 外观 */}
        <section className="card">
          <h3 className="card__title">外观</h3>

          <div className="row">
            <div className="row__text">
              <span className="row__label">界面主题</span>
              <span className="row__desc">跟随系统，或强制亮色 / 暗色</span>
            </div>
            <div className="segmented">
              {(
                [
                  ['system', '跟随系统'],
                  ['light', '亮色'],
                  ['dark', '暗色'],
                ] as const
              ).map(([v, label]) => (
                <button
                  key={v}
                  type="button"
                  className={`segmented__btn ${draft.theme === v ? 'is-active' : ''}`}
                  onClick={() => set('theme', v)}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>

          <div className="row">
            <div className="row__text">
              <span className="row__label">便签主题包</span>
              <span className="row__desc">
                决定便签配色；把自定义主题放进主题目录即可扩展
              </span>
            </div>
            <div className="row__ctrl">
              <select
                className="field-select"
                value={draft.themePack}
                onChange={(e) => set('themePack', e.target.value)}
              >
                {themes.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.name}
                    {t.builtin ? '' : '（自定义）'}
                  </option>
                ))}
              </select>
              <button
                type="button"
                className="btn-icon"
                title="重新扫描主题目录"
                onClick={onReloadThemes}
              >
                <IconRefresh size={15} />
              </button>
            </div>
          </div>

          {/* 主题色预览 */}
          <div className="row row--stack">
            <span className="row__label">配色预览</span>
            <div className="swatch-strip">
              {COLORS.map((c) => {
                const pack = themes.find((t) => t.id === draft.themePack);
                const hex = pack?.colors?.[c];
                return (
                  <span
                    key={c}
                    className="swatch-strip__item"
                    data-swatch={c}
                    style={hex ? { background: hex } : undefined}
                    title={COLOR_LABELS[c]}
                  />
                );
              })}
            </div>
          </div>

          <div className="row">
            <div className="row__text">
              <span className="row__label">新建便签默认颜色</span>
            </div>
            <div className="palette-row">
              {COLORS.map((c) => (
                <button
                  key={c}
                  type="button"
                  className={`pal-dot ${draft.defaultColor === c ? 'is-active' : ''}`}
                  data-swatch={c}
                  title={COLOR_LABELS[c]}
                  onClick={() => set('defaultColor', c)}
                />
              ))}
            </div>
          </div>

          <div className="row">
            <div className="row__text">
              <span className="row__label">便签默认宽度</span>
              <span className="row__desc">像素</span>
            </div>
            <input
              className="field-num"
              type="number"
              min={180}
              max={1200}
              value={draft.defaultNoteWidth}
              onChange={(e) => set('defaultNoteWidth', Number(e.target.value) || 320)}
            />
          </div>

          <div className="row">
            <div className="row__text">
              <span className="row__label">便签默认高度</span>
              <span className="row__desc">像素</span>
            </div>
            <input
              className="field-num"
              type="number"
              min={140}
              max={1400}
              value={draft.defaultNoteHeight}
              onChange={(e) => set('defaultNoteHeight', Number(e.target.value) || 360)}
            />
          </div>
        </section>

        {/* ---------------------------------------------------- 行为 */}
        <section className="card">
          <h3 className="card__title">行为</h3>

          <Toggle
            label="启动时恢复上次打开的便签"
            desc="重新打开应用时，之前贴在桌面上的便签会自动回来"
            checked={draft.restoreOnStart}
            onChange={(v) => set('restoreOnStart', v)}
          />

          <Toggle
            label="关闭主窗口时最小化到托盘"
            desc="关闭面板不退出应用，便签继续贴在桌面上"
            checked={draft.minimizeToTray}
            onChange={(v) => set('minimizeToTray', v)}
          />

          <div className="row">
            <div className="row__text">
              <span className="row__label">回收站保留天数</span>
              <span className="row__desc">超期自动清理；填 0 表示永久保留</span>
            </div>
            <input
              className="field-num"
              type="number"
              min={0}
              max={365}
              value={draft.trashRetentionDays}
              onChange={(e) => set('trashRetentionDays', Number(e.target.value) || 0)}
            />
          </div>
        </section>

        {/* ---------------------------------------------------- 数据 */}
        <section className="card">
          <h3 className="card__title">数据</h3>

          <div className="row">
            <div className="row__text">
              <span className="row__label">导出便签</span>
              <span className="row__desc">导出为 JSON 备份文件，包含全部便签与设置</span>
            </div>
            <div className="row__ctrl">
              <button
                type="button"
                className="btn-secondary"
                onClick={async () => {
                  try {
                    const p = await backup.exportDialog();
                    onFlash(`已导出到 ${p.split(/[\\/]/).pop()}`);
                  } catch (e) {
                    onFlash('导出失败');
                  }
                }}
              >
                <IconArchive size={15} />
                <span>导出</span>
              </button>
              <button
                type="button"
                className="btn-secondary"
                onClick={async () => {
                  try {
                    const r = await backup.importDialog();
                    onFlash(r.message);
                  } catch (e) {
                    onFlash('导入失败');
                  }
                }}
              >
                <span>导入</span>
              </button>
            </div>
          </div>

          <div className="row">
            <div className="row__text">
              <span className="row__label">数据目录</span>
              <span className="row__desc">数据库、插件与主题的存放位置</span>
            </div>
            <button
              type="button"
              className="btn-secondary"
              onClick={async () => {
                const p = await ext.paths();
                await ext.openDir('data');
                onFlash(p.data);
              }}
            >
              打开
            </button>
          </div>

          <div className="row">
            <div className="row__text">
              <span className="row__label">笔记库目录</span>
              <span className="row__desc">
                纯 Markdown 文件，可直接用 Obsidian / VS Code / Typora 打开
              </span>
            </div>
            <button
              type="button"
              className="btn-secondary"
              onClick={async () => {
                const p = await ext.paths();
                await ext.openDir('vault');
                onFlash(p.vault);
              }}
            >
              打开
            </button>
          </div>

          <div className="row">
            <div className="row__text">
              <span className="row__label">插件日志目录</span>
              <span className="row__desc">
                插件后端进程的输出，插件启动失败时先看这里
              </span>
            </div>
            <button
              type="button"
              className="btn-secondary"
              onClick={async () => {
                const p = await ext.paths();
                await ext.openDir('logs');
                onFlash(p.logs);
              }}
            >
              打开
            </button>
          </div>
        </section>

        {/* ------------------------------------------------ 开发者 / 拓展 */}
        <section className="card card--accent">
          <h3 className="card__title">
            <IconBolt size={15} />
            开发者
          </h3>
          <p className="card__hint">
            开启后，任意本机程序都能通过 HTTP 接口读写便签，也可把事件推送到你的服务。
          </p>

          <Toggle
            label="启用本地 HTTP API"
            desc="仅监听 127.0.0.1，不对局域网暴露"
            checked={draft.httpApiEnabled}
            onChange={(v) => set('httpApiEnabled', v)}
          />

          <div className="row">
            <div className="row__text">
              <span className="row__label">监听端口</span>
            </div>
            <input
              className="field-num"
              type="number"
              min={1024}
              max={65535}
              value={draft.httpApiPort}
              onChange={(e) => set('httpApiPort', Number(e.target.value) || 43919)}
            />
          </div>

          <Toggle
            label="启用插件"
            desc="允许插件订阅事件、注册命令；插件目录在数据目录下的 plugins/"
            checked={draft.pluginsEnabled}
            onChange={(v) => set('pluginsEnabled', v)}
          />

          <ApiHint enabled={draft.httpApiEnabled} port={draft.httpApiPort} />
        </section>
      </div>
    </div>
  );
}

/** 开关 */
function Toggle({
  label,
  desc,
  checked,
  onChange,
}: {
  label: string;
  desc?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="row">
      <div className="row__text">
        <span className="row__label">{label}</span>
        {desc && <span className="row__desc">{desc}</span>}
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        className={`switch ${checked ? 'is-on' : ''}`}
        onClick={() => onChange(!checked)}
      >
        <span className="switch__knob" />
      </button>
    </div>
  );
}

/** HTTP API 状态与调用示例 */
function ApiHint({ enabled, port }: { enabled: boolean; port: number }) {
  const [status, setStatus] = useState<{ running: boolean; baseUrl: string | null; token: string } | null>(
    null
  );

  useEffect(() => {
    ext.apiStatus().then(setStatus).catch(() => setStatus(null));
  }, [enabled, port]);

  if (!enabled) return null;

  const base = status?.baseUrl ?? `http://127.0.0.1:${port}/api/v1`;

  return (
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
      <pre className="api-hint__code">{`# 新建便签
curl -X POST ${base}/notes \\
  -H "Content-Type: application/json" \\
  -d '{"content":"来自脚本的便签"}'

# 通用命令入口（Agent 推荐）
curl -X POST ${base}/execute \\
  -H "Content-Type: application/json" \\
  -d '{"command":"notes.append","args":{"id":"<noteId>","text":"追加一行"}}'

# 订阅实时事件
curl -N ${base}/events/subscribe`}</pre>
    </div>
  );
}

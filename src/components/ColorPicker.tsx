/**
 * 颜色选择器
 *
 * 复刻 Windows 便笺的交互：点击色块弹出六色圆点横排，
 * 当前色带对勾，悬停放大。键盘可操作。
 */

import { useEffect, useRef, useState } from 'react';
import { COLORS, COLOR_LABELS } from '../lib/api';
import { IconPalette, IconCheck } from './Icons';
import './ColorPicker.css';

interface Props {
  value: string;
  onChange: (color: string) => void;
  /** 主题包提供的自定义色表，覆盖内置六色 */
  colorMap?: Record<string, string>;
  variant?: 'note' | 'panel';
  align?: 'left' | 'right';
}

export function ColorPicker({
  value,
  onChange,
  colorMap,
  variant = 'note',
  align = 'right',
}: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  // 点击外部 / Esc 关闭
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        setOpen(false);
      }
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey, true);
    };
  }, [open]);

  const palette = COLORS.map((c) => ({
    id: c,
    label: COLOR_LABELS[c],
    hex: colorMap?.[c],
  }));

  return (
    <div className="color-picker" ref={rootRef}>
      <button
        type="button"
        className={`cp-trigger cp-trigger--${variant}`}
        onClick={() => setOpen((v) => !v)}
        title="更改颜色"
        aria-label="更改颜色"
        aria-expanded={open}
        data-tauri-drag-region={undefined}
      >
        <IconPalette size={variant === 'note' ? 17 : 16} />
      </button>

      {open && (
        <div className={`cp-popover cp-popover--${align}`} role="listbox" aria-label="便签颜色">
          {palette.map((p) => (
            <button
              key={p.id}
              type="button"
              role="option"
              aria-selected={p.id === value}
              className={`cp-swatch ${p.id === value ? 'is-active' : ''}`}
              style={{ background: p.hex ? `var(--sn-${p.id}, ${p.hex})` : undefined }}
              data-swatch={p.id}
              title={p.label}
              onClick={() => {
                onChange(p.id);
                setOpen(false);
              }}
            >
              {p.id === value && <IconCheck size={13} strokeWidth={2.6} />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * 图标集
 *
 * 全部内联 SVG，避免图标字体 / 外部资源依赖，也便于跟随主题变色。
 * 图形语义与 Windows 11 / 便笺 保持一致的视觉语言：2px 描边、圆角端点。
 */

interface IconProps {
  size?: number;
  className?: string;
  strokeWidth?: number;
}

const base = (size: number, strokeWidth: number) => ({
  width: size,
  height: size,
  viewBox: '0 0 24 24',
  fill: 'none' as const,
  stroke: 'currentColor',
  strokeWidth,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
});

export function IconPlus({ size = 16, className, strokeWidth = 1.9 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

export function IconSearch({ size = 16, className, strokeWidth = 1.9 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <circle cx="11" cy="11" r="6.5" />
      <path d="M16 16l4.5 4.5" />
    </svg>
  );
}

export function IconClose({ size = 14, className, strokeWidth = 1.9 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M6 6l12 12M18 6L6 18" />
    </svg>
  );
}

export function IconMinimize({ size = 14, className, strokeWidth = 1.9 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M6 12h12" />
    </svg>
  );
}

export function IconMaximize({ size = 14, className, strokeWidth = 1.9 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <rect x="6" y="6" width="12" height="12" rx="1.5" />
    </svg>
  );
}

export function IconRestore({ size = 14, className, strokeWidth = 1.9 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <rect x="4.5" y="7.5" width="11" height="11" rx="1.5" />
      <path d="M8 7.5V6a1.5 1.5 0 011.5-1.5H18A1.5 1.5 0 0119.5 6v8.5A1.5 1.5 0 0118 16h-1.5" />
    </svg>
  );
}

export function IconPin({ size = 16, className, strokeWidth = 1.8, filled = false }: IconProps & { filled?: boolean }) {
  return (
    <svg {...base(size, strokeWidth)} className={className} fill={filled ? 'currentColor' : 'none'} aria-hidden>
      <path d="M9 3h6l-.7 5.2 3 2.6-.6 1.9H6.3l-.6-1.9 3-2.6L9 3z" />
      <path d="M12 13v8" />
    </svg>
  );
}

export function IconTrash({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M4 6.5h16" />
      <path d="M9.5 6.5V5a1.5 1.5 0 011.5-1.5h2A1.5 1.5 0 0114.5 5v1.5" />
      <path d="M6.5 6.5l.9 12a1.5 1.5 0 001.5 1.4h6.2a1.5 1.5 0 001.5-1.4l.9-12" />
      <path d="M10.5 10.5v6M13.5 10.5v6" />
    </svg>
  );
}

export function IconMore({ size = 16, className, strokeWidth = 2 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <circle cx="5.5" cy="12" r="1.2" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1.2" fill="currentColor" stroke="none" />
      <circle cx="18.5" cy="12" r="1.2" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** 便笺工具栏的「…」是纵排的 */
export function IconMoreVertical({ size = 16, className, strokeWidth = 2 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <circle cx="12" cy="5.5" r="1.2" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1.2" fill="currentColor" stroke="none" />
      <circle cx="12" cy="18.5" r="1.2" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function IconCopy({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <rect x="8.5" y="8.5" width="11" height="11" rx="2" />
      <path d="M15.5 5.5h-9a2 2 0 00-2 2v9" />
    </svg>
  );
}

export function IconCheck({ size = 16, className, strokeWidth = 2.1 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M5 12.5l4.5 4.5L19 7" />
    </svg>
  );
}

export function IconGrid({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <rect x="4" y="4" width="6.5" height="6.5" rx="1.4" />
      <rect x="13.5" y="4" width="6.5" height="6.5" rx="1.4" />
      <rect x="4" y="13.5" width="6.5" height="6.5" rx="1.4" />
      <rect x="13.5" y="13.5" width="6.5" height="6.5" rx="1.4" />
    </svg>
  );
}

export function IconList({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M4 6.5h16M4 12h16M4 17.5h10" />
    </svg>
  );
}

export function IconSettings({ size = 16, className, strokeWidth = 1.7 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 00.34 1.87l.06.06a2 2 0 11-2.83 2.83l-.06-.06a1.7 1.7 0 00-1.87-.34 1.7 1.7 0 00-1.03 1.56V21a2 2 0 11-4 0v-.1A1.7 1.7 0 008.9 19.3a1.7 1.7 0 00-1.87.34l-.06.06a2 2 0 11-2.83-2.83l.06-.06A1.7 1.7 0 004.54 15a1.7 1.7 0 00-1.56-1.03H3a2 2 0 110-4h.1A1.7 1.7 0 004.66 8.9a1.7 1.7 0 00-.34-1.87l-.06-.06a2 2 0 112.83-2.83l.06.06A1.7 1.7 0 009 4.54 1.7 1.7 0 0010.03 3V3a2 2 0 114 0v.1A1.7 1.7 0 0015.06 4.66a1.7 1.7 0 001.87-.34l.06-.06a2 2 0 112.83 2.83l-.06.06A1.7 1.7 0 0019.46 9a1.7 1.7 0 001.56 1.03H21a2 2 0 110 4h-.1a1.7 1.7 0 00-1.5 1.03z" />
    </svg>
  );
}

export function IconPuzzle({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M9.5 4.5a1.8 1.8 0 013.6 0V6h2.4a1.5 1.5 0 011.5 1.5V10h1.5a1.8 1.8 0 010 3.6H17v2.4a1.5 1.5 0 01-1.5 1.5h-2.4v1.5a1.8 1.8 0 01-3.6 0V17.5H7A1.5 1.5 0 015.5 16v-2.4H4a1.8 1.8 0 010-3.6h1.5V7.5A1.5 1.5 0 017 6h2.5V4.5z" />
    </svg>
  );
}

export function IconPalette({ size = 16, className, strokeWidth = 1.7 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M12 3a9 9 0 000 18 2.2 2.2 0 001.7-3.6 2.2 2.2 0 011.7-3.6H18a3.5 3.5 0 003.5-3.5C21.5 6.4 17.3 3 12 3z" />
      <circle cx="7.6" cy="11" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="10.4" cy="7.3" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="15.2" cy="7.9" r="1.1" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function IconLink({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M10.5 13.5a3.5 3.5 0 005 0l3-3a3.54 3.54 0 00-5-5l-1 1" />
      <path d="M13.5 10.5a3.5 3.5 0 00-5 0l-3 3a3.54 3.54 0 005 5l1-1" />
    </svg>
  );
}

export function IconBolt({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M13 3L5.5 13.5H11l-1 7.5L18.5 10H13l0-7z" />
    </svg>
  );
}

export function IconArchive({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <rect x="3.5" y="4.5" width="17" height="4" rx="1.3" />
      <path d="M5.5 8.5v9.2A1.8 1.8 0 007.3 19.5h9.4a1.8 1.8 0 001.8-1.8V8.5" />
      <path d="M10 12.5h4" />
    </svg>
  );
}

export function IconNote({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M6 3.5h9L19 7.5v13a1 1 0 01-1 1H6a1 1 0 01-1-1v-16a1 1 0 011-1z" />
      <path d="M14.5 3.5V8h4.5" />
      <path d="M8.5 12.5h7M8.5 16h4.5" />
    </svg>
  );
}

/** 笔记库（打开的书） */
export function IconBook({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M4 4.5A1.5 1.5 0 015.5 3H10a2 2 0 012 2v14a1.6 1.6 0 00-1.6-1.6H5.5A1.5 1.5 0 014 15.9V4.5z" />
      <path d="M20 4.5A1.5 1.5 0 0018.5 3H14a2 2 0 00-2 2v14a1.6 1.6 0 011.6-1.6h4.9A1.5 1.5 0 0020 15.9V4.5z" />
    </svg>
  );
}

/** 文件夹 */
export function IconFolder({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M3.5 6.5a1.5 1.5 0 011.5-1.5h4l2 2.4h6.5a1.5 1.5 0 011.5 1.5v8.6a1.5 1.5 0 01-1.5 1.5H5a1.5 1.5 0 01-1.5-1.5V6.5z" />
    </svg>
  );
}

/** 侧栏展开箭头 */
export function IconChevron({ size = 16, className, strokeWidth = 2 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M9 6l6 6-6 6" />
    </svg>
  );
}

/** 分栏视图 */
export function IconColumns({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <rect x="3.5" y="4.5" width="17" height="15" rx="1.8" />
      <path d="M12 4.5v15" />
    </svg>
  );
}

/** 铅笔（编辑） */
export function IconPencil({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M4 20h4L19.5 8.5a1.8 1.8 0 00-2.5-2.5L5.5 17.5 4 20z" />
      <path d="M15.5 6.5l2.5 2.5" />
    </svg>
  );
}

/** 眼睛（阅读） */
export function IconEye({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z" />
      <circle cx="12" cy="12" r="2.8" />
    </svg>
  );
}

/** 双向链接（两条互指的箭头） */
export function IconBidirectional({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M4 8.5h13M14 5l3.5 3.5L14 12" />
      <path d="M20 15.5H7M10 12l-3.5 3.5L10 19" />
    </svg>
  );
}

/** 标签 */
export function IconTag({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M11 3.5H5.5A2 2 0 003.5 5.5V11a2 2 0 00.6 1.4l7 7a2 2 0 002.8 0l6-6a2 2 0 000-2.8l-7-7A2 2 0 0011 3.5z" />
      <circle cx="8" cy="8" r="1.3" />
    </svg>
  );
}

export function IconRefresh({ size = 16, className, strokeWidth = 1.9 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M20 12a8 8 0 10-2.6 5.9" />
      <path d="M20 5.5V12h-6.5" />
    </svg>
  );
}

export function IconEyeOff({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M4 4l16 16" />
      <path d="M9.9 5.2A9.6 9.6 0 0112 5c5 0 8.5 4.2 9.6 6.2a1.6 1.6 0 010 1.6 15 15 0 01-2.4 3" />
      <path d="M6.3 7.7A15.6 15.6 0 002.4 11.2a1.6 1.6 0 000 1.6C3.5 14.8 7 19 12 19a9.4 9.4 0 004.2-1" />
      <path d="M10 10.2a2.6 2.6 0 003.7 3.6" />
    </svg>
  );
}

/** 星芒 —— 用于「AI / 智能」类动作（汇总成笔记、Agent 调用） */
export function IconSparkle({ size = 16, className, strokeWidth = 1.7 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9L12 3z" />
      <path d="M18.5 16.5l.7 1.8 1.8.7-1.8.7-.7 1.8-.7-1.8-1.8-.7 1.8-.7.7-1.8z" />
    </svg>
  );
}

/** 播放 —— 启动插件进程 */
export function IconPlay({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M7 4.5l12 7.5-12 7.5V4.5z" />
    </svg>
  );
}

/** 停止 —— 终止插件进程 */
export function IconStop({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <rect x="6" y="6" width="12" height="12" rx="2.2" />
    </svg>
  );
}

/** 心跳 —— ping 插件 */
export function IconPulse({ size = 16, className, strokeWidth = 1.8 }: IconProps) {
  return (
    <svg {...base(size, strokeWidth)} className={className} aria-hidden>
      <path d="M2.5 12h4l2-5 3.5 10 2.5-5h7" />
    </svg>
  );
}

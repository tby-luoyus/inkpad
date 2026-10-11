/**
 * 轻量 Markdown 渲染器（对标 Obsidian 的阅读视图）
 *
 * 设计取舍：
 *  - 不引入 marked / remark 等重依赖，保持应用体积与启动速度
 *  - 覆盖 Obsidian 常用语法：标题、粗斜体、删除线、行内代码、代码块、
 *    引用、列表、任务清单、分隔线、表格、链接、图片
 *  - 扩展语法：[[双向链接]]、![[嵌入]]、#标签、==高亮==、> [!note] 标注
 *  - 输出 HTML 字符串，由组件以 dangerouslySetInnerHTML 渲染；
 *    所有文本先做 HTML 转义，避免 XSS。
 */

/** HTML 转义：任何来自用户的文本都必须先过这一步 */
export function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/** 行内标记：代码 → 高亮 → 粗斜 → 链接 → wikilink → 标签 */
function inline(raw: string, opts: { linkTargets?: Set<string> } = {}): string {
  // 先把行内代码抠出来占位，避免其中的符号被后续规则误伤
  const codes: string[] = [];
  let s = raw.replace(/`([^`]+)`/g, (_m, c: string) => {
    codes.push(c);
    return `\u0000CODE${codes.length - 1}\u0000`;
  });

  s = escapeHtml(s);

  // ==高亮==
  s = s.replace(/==([^=]+)==/g, '<mark>$1</mark>');
  // 粗体 / 斜体 / 删除线
  s = s.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  s = s.replace(/(^|[^*])\*([^*\n]+)\*/g, '$1<em>$2</em>');
  s = s.replace(/~~([^~]+)~~/g, '<del>$1</del>');

  // [[目标|别名]] / [[目标]]
  s = s.replace(/\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g, (_m, target: string, alias?: string) => {
    const t = target.split('#')[0].trim();
    const label = escapeHtml((alias ?? target).trim());
    const known = opts.linkTargets?.has(t) ? ' is-known' : ' is-new';
    return `<a class="md-wikilink${known}" data-wikilink="${escapeHtml(t)}" title="${escapeHtml(
      t
    )}">${label}</a>`;
  });

  // #标签（避免命中已生成的 HTML 属性里的 #）
  s = s.replace(/(^|\s)#([\w\u4e00-\u9fff][\w\u4e00-\u9fff/-]*)/g, (_m, sp: string, tag: string) => {
    return `${sp}<a class="md-tag" data-tag="${escapeHtml(tag)}">#${escapeHtml(tag)}</a>`;
  });

  // 图片 ![alt](url) —— 必须在普通链接之前
  s = s.replace(
    /!\[([^\]]*)\]\(([^)\s]+)\)/g,
    '<img class="md-img" src="$2" alt="$1" loading="lazy" />'
  );
  // 链接 [text](url)
  s = s.replace(
    /\[([^\]]+)\]\(([^)\s]+)\)/g,
    '<a class="md-link" href="$2" target="_blank" rel="noreferrer">$1</a>'
  );
  // 裸 URL
  s = s.replace(
    /(^|[\s(])(https?:\/\/[^\s<)]+)/g,
    '$1<a class="md-link" href="$2" target="_blank" rel="noreferrer">$2</a>'
  );

  // 还原行内代码
  s = s.replace(/\u0000CODE(\d+)\u0000/g, (_m, i: string) => `<code class="md-code">${escapeHtml(codes[+i])}</code>`);
  return s;
}

interface RenderOpts {
  /** 当前笔记库里已知的笔记名，用于给 [[链接]] 标记「已存在 / 待创建」 */
  linkTargets?: Set<string>;
  /** 点击 [[链接]] / #标签 时回调 */
  onNavigate?: (kind: 'wikilink' | 'tag', value: string) => void;
}

/**
 * 渲染 Markdown 为 HTML
 */
export function renderMarkdown(src: string, opts: RenderOpts = {}): string {
  const lines = src.replace(/\r\n?/g, '\n').split('\n');
  const out: string[] = [];
  let i = 0;

  const closeList = (stack: string[]) => {
    while (stack.length) out.push(`</${stack.pop()}>`);
  };
  const listStack: string[] = [];

  while (i < lines.length) {
    const line = lines[i];

    // ---------------------------------------------------------- 代码块
    const fence = line.match(/^\s*(```|~~~)(.*)$/);
    if (fence) {
      closeList(listStack);
      const lang = fence[2].trim();
      const buf: string[] = [];
      i++;
      while (i < lines.length && !/^\s*(```|~~~)\s*$/.test(lines[i])) {
        buf.push(lines[i]);
        i++;
      }
      i++; // 跳过结束围栏
      out.push(
        `<pre class="md-pre"><code class="md-pre__code"${
          lang ? ` data-lang="${escapeHtml(lang)}"` : ''
        }>${escapeHtml(buf.join('\n'))}</code></pre>`
      );
      continue;
    }

    // ---------------------------------------------------------- 标题
    const h = line.match(/^(#{1,6})\s+(.*)$/);
    if (h) {
      closeList(listStack);
      out.push(`<h${h[1].length} class="md-h">${inline(h[2], opts)}</h${h[1].length}>`);
      i++;
      continue;
    }

    // ---------------------------------------------------------- 分隔线
    if (/^\s*([-*_])\s*(\1\s*){2,}$/.test(line)) {
      closeList(listStack);
      out.push('<hr class="md-hr" />');
      i++;
      continue;
    }

    // ---------------------------------------------------------- 表格
    if (/^\s*\|.*\|\s*$/.test(line) && i + 1 < lines.length && /^\s*\|[\s:|-]+\|\s*$/.test(lines[i + 1])) {
      closeList(listStack);
      const cells = (l: string) =>
        l
          .trim()
          .replace(/^\||\|$/g, '')
          .split('|')
          .map((c) => c.trim());
      const head = cells(line);
      const align = cells(lines[i + 1]).map((a) =>
        a.startsWith(':') && a.endsWith(':') ? 'center' : a.endsWith(':') ? 'right' : 'left'
      );
      i += 2;
      const body: string[][] = [];
      while (i < lines.length && /^\s*\|.*\|\s*$/.test(lines[i])) {
        body.push(cells(lines[i]));
        i++;
      }
      const th = head
        .map((c, k) => `<th style="text-align:${align[k] ?? 'left'}">${inline(c, opts)}</th>`)
        .join('');
      const trs = body
        .map(
          (r) =>
            `<tr>${r
              .map((c, k) => `<td style="text-align:${align[k] ?? 'left'}">${inline(c, opts)}</td>`)
              .join('')}</tr>`
        )
        .join('');
      out.push(`<table class="md-table"><thead><tr>${th}</tr></thead><tbody>${trs}</tbody></table>`);
      continue;
    }

    // ---------------------------------------------------------- 引用 / 标注
    if (/^\s*>\s?/.test(line)) {
      closeList(listStack);
      const buf: string[] = [];
      while (i < lines.length && /^\s*>\s?/.test(lines[i])) {
        buf.push(lines[i].replace(/^\s*>\s?/, ''));
        i++;
      }
      // Obsidian 标注：> [!note] 标题
      const callout = buf[0]?.match(/^\[!(\w+)\]\s*(.*)$/);
      if (callout) {
        const kind = callout[1].toLowerCase();
        const title = callout[2] || kind;
        const rest = buf.slice(1).join('\n');
        out.push(
          `<div class="md-callout md-callout--${escapeHtml(kind)}"><div class="md-callout__title">${escapeHtml(
            title
          )}</div><div class="md-callout__body">${renderMarkdown(rest, opts)}</div></div>`
        );
      } else {
        out.push(`<blockquote class="md-quote">${renderMarkdown(buf.join('\n'), opts)}</blockquote>`);
      }
      continue;
    }

    // ---------------------------------------------------------- 列表
    // 任务清单 - [ ] / - [x]
    const task = line.match(/^(\s*)[-*+]\s+\[([ xX])\]\s+(.*)$/);
    if (task) {
      if (!listStack.length) {
        out.push('<ul class="md-ul md-ul--task">');
        listStack.push('ul');
      }
      const checked = task[2].toLowerCase() === 'x';
      out.push(
        `<li class="md-task ${checked ? 'is-done' : ''}"><span class="md-task__box"${
          checked ? ' data-checked="1"' : ''
        }></span>${inline(task[3], opts)}</li>`
      );
      i++;
      continue;
    }

    const ul = line.match(/^(\s*)[-*+]\s+(.*)$/);
    const ol = line.match(/^(\s*)\d+[.)]\s+(.*)$/);
    if (ul || ol) {
      const want = ul ? 'ul' : 'ol';
      const text = (ul ? ul[2] : ol![2]) ?? '';
      if (listStack[listStack.length - 1] !== want) {
        // 嵌套情况简化处理：同级切换时重开列表
        if (listStack.length && listStack[listStack.length - 1] !== want) {
          out.push(`</${listStack.pop()}>`);
        }
        if (!listStack.length) {
          out.push(want === 'ul' ? '<ul class="md-ul">' : '<ol class="md-ol">');
          listStack.push(want);
        }
      }
      out.push(`<li class="md-li">${inline(text, opts)}</li>`);
      i++;
      continue;
    }

    // ---------------------------------------------------------- 空行
    if (!line.trim()) {
      closeList(listStack);
      i++;
      continue;
    }

    // ---------------------------------------------------------- 段落
    closeList(listStack);
    const buf: string[] = [line];
    i++;
    while (
      i < lines.length &&
      lines[i].trim() &&
      !/^(#{1,6})\s/.test(lines[i]) &&
      !/^\s*(```|~~~)/.test(lines[i]) &&
      !/^\s*>/.test(lines[i]) &&
      !/^(\s*)[-*+]\s+/.test(lines[i]) &&
      !/^(\s*)\d+[.)]\s+/.test(lines[i])
    ) {
      buf.push(lines[i]);
      i++;
    }
    out.push(`<p class="md-p">${inline(buf.join('<br />'), opts)}</p>`);
  }

  closeList(listStack);
  return out.join('\n');
}

/** 从正文里抽出所有 [[链接]] 目标，用于编辑器补全 */
export function extractWikilinks(content: string): string[] {
  const out: string[] = [];
  const re = /\[\[([^\]|]+)(?:\|[^\]]+)?\]\]/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(content))) {
    const t = m[1].split('#')[0].trim();
    if (t) out.push(t);
  }
  return [...new Set(out)];
}

/** 文档统计：字数 / 字符 / 行数 / 阅读时长 */
export function docStats(content: string) {
  const chars = content.length;
  const words = content.replace(/\s/g, '').length;
  const lines = content ? content.split('\n').length : 0;
  // 中文按 300 字/分钟，英文按 200 词/分钟，这里统一按字符估算
  const minutes = Math.max(1, Math.round(words / 300));
  return { chars, words, lines, minutes };
}

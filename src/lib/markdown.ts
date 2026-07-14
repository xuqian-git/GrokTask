/**
 * Safe subset Markdown → HTML for final assistant replies and expanded thoughts.
 * GFM-ish: paragraphs, headings, lists, task lists, blockquotes, tables,
 * links, inline code, fenced code. No raw HTML, script, or dangerous URLs.
 */

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/**
 * Strip ASCII whitespace/control chars for protocol detection only.
 * Obfuscations like `java\nscript:` / `java\tscript:` must still be rejected.
 */
function schemeForProtocolCheck(url: string): string {
  let out = "";
  for (let i = 0; i < url.length; i++) {
    const code = url.charCodeAt(i);
    // Strip C0 controls and space (U+0000–U+0020)
    if (code <= 0x20) continue;
    out += url[i];
  }
  return out.toLowerCase();
}

/** Exported for unit tests of protocol detection. */
export function sanitizeUrl(url: string): string | null {
  const t = url.trim();
  if (!t) return null;

  const scheme = schemeForProtocolCheck(t);
  if (
    scheme.startsWith("javascript:") ||
    scheme.startsWith("data:") ||
    scheme.startsWith("vbscript:")
  ) {
    return null;
  }

  const lower = t.toLowerCase();
  if (
    lower.startsWith("http://") ||
    lower.startsWith("https://") ||
    lower.startsWith("mailto:") ||
    t.startsWith("#") ||
    t.startsWith("/")
  ) {
    return t;
  }

  // Relative paths ok (no scheme). Reject unknown schemes; also reject if
  // control-stripped form looks like a scheme (e.g. `java\nscript:…`).
  if (
    !/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(t) &&
    !/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(scheme)
  ) {
    return t;
  }
  return null;
}

/** Inline markdown (bold, italic, code, links, strikethrough) on raw text. */
function renderInline(raw: string): string {
  let s = escapeHtml(raw);
  // code first so markers inside code stay literal
  s = s.replace(/`([^`]+)`/g, "<code>$1</code>");
  // bold
  s = s.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  // italic
  s = s.replace(/(?<!\*)\*([^*]+)\*(?!\*)/g, "<em>$1</em>");
  // strikethrough
  s = s.replace(/~~([^~]+)~~/g, "<del>$1</del>");
  // links [text](url)
  s = s.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_m, text, url) => {
    const safe = sanitizeUrl(url);
    if (!safe) return text;
    return `<a href="${escapeHtml(safe)}" rel="noopener noreferrer" target="_blank">${text}</a>`;
  });
  return s;
}

function isTableSeparator(line: string): boolean {
  // |---|:---| or ---|---
  const t = line.trim();
  if (!t.includes("-")) return false;
  return /^\|?[\s:|-]+\|[\s:|-]*\|?$/.test(t) && /-+/.test(t);
}

function splitTableRow(line: string): string[] {
  let t = line.trim();
  if (t.startsWith("|")) t = t.slice(1);
  if (t.endsWith("|")) t = t.slice(0, -1);
  return t.split("|").map((c) => c.trim());
}

function isTaskListItem(line: string): RegExpMatchArray | null {
  return line.match(/^[-*+]\s+\[([ xX])\]\s+(.*)$/);
}

/**
 * Convert a GFM-ish subset to safe HTML.
 * Streaming mode can call this on partial text; unclosed fences render as pre.
 */
export function renderMarkdown(source: string): string {
  if (!source) return "";
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const out: string[] = [];
  let i = 0;
  let inCode = false;
  let codeLang = "";
  let codeBuf: string[] = [];
  let listType: "ul" | "ol" | "task" | null = null;

  const closeList = () => {
    if (listType === "task") {
      out.push("</ul>");
    } else if (listType) {
      out.push(listType === "ul" ? "</ul>" : "</ol>");
    }
    listType = null;
  };

  while (i < lines.length) {
    const line = lines[i];

    if (line.startsWith("```")) {
      if (!inCode) {
        closeList();
        inCode = true;
        codeLang = line.slice(3).trim();
        codeBuf = [];
      } else {
        const body = escapeHtml(codeBuf.join("\n"));
        const cls = codeLang ? ` class="language-${escapeHtml(codeLang)}"` : "";
        out.push(`<pre><code${cls}>${body}</code></pre>`);
        inCode = false;
        codeLang = "";
        codeBuf = [];
      }
      i += 1;
      continue;
    }
    if (inCode) {
      codeBuf.push(line);
      i += 1;
      continue;
    }

    // Tables: header + separator + body rows
    if (
      i + 1 < lines.length &&
      line.includes("|") &&
      isTableSeparator(lines[i + 1])
    ) {
      closeList();
      const headers = splitTableRow(line);
      i += 2; // skip separator
      const bodyRows: string[][] = [];
      while (i < lines.length && lines[i].includes("|") && lines[i].trim() !== "") {
        bodyRows.push(splitTableRow(lines[i]));
        i += 1;
      }
      const thead = headers
        .map((h) => `<th>${renderInline(h)}</th>`)
        .join("");
      const tbody = bodyRows
        .map((cells) => {
          const tds = headers
            .map((_, idx) => `<td>${renderInline(cells[idx] ?? "")}</td>`)
            .join("");
          return `<tr>${tds}</tr>`;
        })
        .join("");
      out.push(
        `<table><thead><tr>${thead}</tr></thead><tbody>${tbody}</tbody></table>`,
      );
      continue;
    }

    if (/^#{1,6}\s+/.test(line)) {
      closeList();
      const level = line.match(/^#+/)![0].length;
      const text = line.replace(/^#{1,6}\s+/, "");
      out.push(`<h${level}>${renderInline(text)}</h${level}>`);
      i += 1;
      continue;
    }

    // Multi-line blockquote
    if (/^>\s?/.test(line)) {
      closeList();
      const quoteLines: string[] = [];
      while (i < lines.length && /^>\s?/.test(lines[i])) {
        quoteLines.push(lines[i].replace(/^>\s?/, ""));
        i += 1;
      }
      // Blank line inside quote (">") continues
      out.push(
        `<blockquote>${quoteLines.map((ql) => `<p>${renderInline(ql)}</p>`).join("")}</blockquote>`,
      );
      continue;
    }

    // Task list
    const task = isTaskListItem(line);
    if (task) {
      if (listType !== "task") {
        closeList();
        out.push('<ul class="task-list">');
        listType = "task";
      }
      const checked = task[1].toLowerCase() === "x";
      out.push(
        `<li class="task-list-item"><input type="checkbox" disabled${checked ? " checked" : ""}/> ${renderInline(task[2])}</li>`,
      );
      i += 1;
      continue;
    }

    const ul = line.match(/^[-*+]\s+(.*)$/);
    if (ul) {
      if (listType !== "ul") {
        closeList();
        out.push("<ul>");
        listType = "ul";
      }
      out.push(`<li>${renderInline(ul[1])}</li>`);
      i += 1;
      continue;
    }

    const ol = line.match(/^\d+\.\s+(.*)$/);
    if (ol) {
      if (listType !== "ol") {
        closeList();
        out.push("<ol>");
        listType = "ol";
      }
      out.push(`<li>${renderInline(ol[1])}</li>`);
      i += 1;
      continue;
    }

    if (line.trim() === "") {
      closeList();
      i += 1;
      continue;
    }

    // Horizontal rule
    if (/^(-{3,}|\*{3,}|_{3,})$/.test(line.trim())) {
      closeList();
      out.push("<hr/>");
      i += 1;
      continue;
    }

    closeList();
    out.push(`<p>${renderInline(line)}</p>`);
    i += 1;
  }

  if (inCode) {
    out.push(`<pre><code>${escapeHtml(codeBuf.join("\n"))}</code></pre>`);
  }
  closeList();
  return out.join("\n");
}

/** Forbidden protocol tokens that must never appear as visible primary UI. */
export const FORBIDDEN_UI_TOKENS = [
  "session/update",
  "tool_call_update",
  "agent_thought_chunk",
  "_x.ai",
  '"jsonrpc"',
] as const;

/** True if a display string looks like raw protocol JSON we must not show. */
export function looksLikeRawAcpJson(s: string): boolean {
  const t = s.trim();
  if (!t.startsWith("{") && !t.startsWith("[")) return false;
  return containsForbiddenUiToken(t);
}

/**
 * True when text embeds raw ACP / protocol control labels even if it is not
 * full JSON (e.g. "ACP 通知：_x.ai/session_notification", "session/update").
 */
export function containsForbiddenUiToken(s: string): boolean {
  const t = s.trim();
  if (!t) return false;
  for (const token of FORBIDDEN_UI_TOKENS) {
    if (t.includes(token)) return true;
  }
  return false;
}

/** True when a string must not be used as primary UI copy. */
export function isUnsafePrimaryUiText(s: string): boolean {
  const t = s.trim();
  if (!t) return false;
  return looksLikeRawAcpJson(t) || containsForbiddenUiToken(t);
}

/**
 * Return human-facing text, or `fallback` when the candidate is empty or
 * contains protocol/control labels / raw ACP JSON.
 */
export function safeDisplayLine(
  candidate: string | undefined | null,
  fallback: string,
): string {
  const t = (candidate ?? "").trim();
  if (!t || isUnsafePrimaryUiText(t)) return fallback;
  return t;
}

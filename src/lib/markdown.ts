/**
 * Safe subset Markdown → HTML for final assistant replies.
 * No raw HTML passthrough, no script, no javascript: URLs.
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
  if (!/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(t) && !/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(scheme)) {
    return t;
  }
  return null;
}

/** Inline markdown (bold, italic, code, links) on already-escaped-or-raw text. */
function renderInline(raw: string): string {
  let s = escapeHtml(raw);
  // code
  s = s.replace(/`([^`]+)`/g, "<code>$1</code>");
  // bold
  s = s.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  // italic
  s = s.replace(/(?<!\*)\*([^*]+)\*(?!\*)/g, "<em>$1</em>");
  // links [text](url)
  s = s.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_m, text, url) => {
    const safe = sanitizeUrl(url);
    if (!safe) return text;
    return `<a href="${escapeHtml(safe)}" rel="noopener noreferrer" target="_blank">${text}</a>`;
  });
  return s;
}

/**
 * Convert a GFM-ish subset to safe HTML.
 * Streaming mode can call this on partial text.
 */
export function renderMarkdown(source: string): string {
  if (!source) return "";
  const lines = source.replace(/\r\n/g, "\n").split("\n");
  const out: string[] = [];
  let i = 0;
  let inCode = false;
  let codeLang = "";
  let codeBuf: string[] = [];
  let listType: "ul" | "ol" | null = null;

  const closeList = () => {
    if (listType) {
      out.push(listType === "ul" ? "</ul>" : "</ol>");
      listType = null;
    }
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

    if (/^#{1,6}\s+/.test(line)) {
      closeList();
      const level = line.match(/^#+/)![0].length;
      const text = line.replace(/^#{1,6}\s+/, "");
      out.push(`<h${level}>${renderInline(text)}</h${level}>`);
      i += 1;
      continue;
    }

    if (/^>\s?/.test(line)) {
      closeList();
      out.push(`<blockquote>${renderInline(line.replace(/^>\s?/, ""))}</blockquote>`);
      i += 1;
      continue;
    }

    const ul = line.match(/^[-*]\s+(.*)$/);
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

    closeList();
    out.push(`<p>${renderInline(line)}</p>`);
    i += 1;
  }

  if (inCode) {
    // Unclosed fence while streaming — show as pre
    out.push(`<pre><code>${escapeHtml(codeBuf.join("\n"))}</code></pre>`);
  }
  closeList();
  return out.join("\n");
}

/** True if a display string looks like raw protocol JSON we must not show. */
export function looksLikeRawAcpJson(s: string): boolean {
  const t = s.trim();
  if (!t.startsWith("{") && !t.startsWith("[")) return false;
  if (
    t.includes("session/update") ||
    t.includes("tool_call_update") ||
    t.includes("agent_thought_chunk") ||
    t.includes('"jsonrpc"')
  ) {
    return true;
  }
  return false;
}

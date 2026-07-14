/**
 * Reasoning stage summary (conversation-stream.md §4.2).
 * Priority: stageTitle → first Markdown heading → first complete sentence
 * → first non-empty ~80-grapheme plain text → “思考过程”.
 */

const FALLBACK = "思考过程";
const MAX_TITLE_GRAPHEMES = 80;

/** Minimal Segmenter shape for environments where lib types omit it. */
type GraphemeSegmenter = {
  segment(input: string): Iterable<{ segment: string }>;
};

function createGraphemeSegmenter(): GraphemeSegmenter | null {
  const IntlAny = Intl as unknown as {
    Segmenter?: new (
      locales?: string | string[],
      options?: { granularity?: string },
    ) => GraphemeSegmenter;
  };
  if (typeof Intl === "undefined" || typeof IntlAny.Segmenter !== "function") {
    return null;
  }
  return new IntlAny.Segmenter(undefined, { granularity: "grapheme" });
}

/** Approximate grapheme count via Intl.Segmenter when available. */
export function graphemeSlice(text: string, max: number): string {
  if (!text) return "";
  const seg = createGraphemeSegmenter();
  if (seg) {
    let out = "";
    let n = 0;
    for (const { segment } of seg.segment(text)) {
      if (n >= max) break;
      out += segment;
      n += 1;
    }
    return out;
  }
  return [...text].slice(0, max).join("");
}

export function graphemeLength(text: string): number {
  if (!text) return 0;
  const seg = createGraphemeSegmenter();
  if (seg) {
    return [...seg.segment(text)].length;
  }
  return [...text].length;
}

/** Strip common Markdown markers for plain-text fallbacks. */
export function stripMarkdownLight(src: string): string {
  return src
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/!\[[^\]]*]\([^)]*\)/g, " ")
    .replace(/\[([^\]]+)]\([^)]*\)/g, "$1")
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/^>\s?/gm, "")
    .replace(/^[-*+]\s+/gm, "")
    .replace(/^\d+\.\s+/gm, "")
    .replace(/[*_~]+/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

function firstMarkdownHeading(text: string): string | null {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  for (const line of lines) {
    const m = line.match(/^#{1,6}\s+(.+)$/);
    if (m) {
      const t = m[1].trim();
      if (t) return t;
    }
  }
  return null;
}

/** First complete sentence ending with 。！？.! ? (CJK + Latin). */
function firstCompleteSentence(text: string): string | null {
  const plain = stripMarkdownLight(text);
  if (!plain) return null;
  const m = plain.match(/^(.+?[.!?。！？])(?:\s|$)/u);
  if (m && m[1].trim()) return m[1].trim();
  return null;
}

function firstNonEmptyFragment(text: string, maxGraphemes: number): string | null {
  const plain = stripMarkdownLight(text);
  if (!plain) return null;
  const sliced = graphemeSlice(plain, maxGraphemes).trim();
  return sliced || null;
}

/**
 * Concise stage title for a completed (or auto) reasoning segment.
 * Caps at 80 graphemes.
 */
export function thoughtStageSummary(opts: {
  stageTitle?: string;
  message?: string;
  text?: string;
}): string {
  const candidates: Array<string | null | undefined> = [
    opts.stageTitle?.trim(),
    opts.message?.trim() && opts.message.trim() !== opts.text?.trim()
      ? opts.message.trim()
      : null,
    opts.text ? firstMarkdownHeading(opts.text) : null,
    opts.text ? firstCompleteSentence(opts.text) : null,
    opts.text ? firstNonEmptyFragment(opts.text, MAX_TITLE_GRAPHEMES) : null,
  ];

  for (const c of candidates) {
    if (c) {
      const capped = graphemeSlice(c, MAX_TITLE_GRAPHEMES).trim();
      if (capped) return capped;
    }
  }
  return FALLBACK;
}

/** Last N non-empty lines for streaming preview (default 3). */
export function thoughtPreviewLines(text: string, maxLines = 3): string {
  if (!text) return "";
  const lines = text
    .replace(/\r\n/g, "\n")
    .split("\n")
    .map((l) => l.trimEnd())
    .filter((l) => l.trim().length > 0);
  if (lines.length <= maxLines) return lines.join("\n");
  return lines.slice(-maxLines).join("\n");
}

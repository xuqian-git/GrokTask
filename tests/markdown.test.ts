import { describe, expect, it } from "vitest";
import {
  looksLikeRawAcpJson,
  renderMarkdown,
  sanitizeUrl,
} from "../src/lib/markdown";

describe("markdown final rendering", () => {
  it("renders bold lists and code safely", () => {
    const html = renderMarkdown(
      "Hello **world**\n\n- one\n- two\n\n```ts\nconst x = 1\n```\n",
    );
    expect(html).toContain("<strong>world</strong>");
    expect(html).toContain("<ul>");
    expect(html).toContain("<li>");
    expect(html).toContain("<pre><code");
    expect(html).toContain("const x = 1");
    expect(html).not.toContain("<script");
  });

  it("escapes raw HTML", () => {
    const html = renderMarkdown(`<script>alert(1)</script>\n\n<img onerror=alert(1)>`);
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
    // Attribute text may remain after escape; tags must not be real DOM nodes.
    expect(html).toContain("&lt;img");
    expect(html).not.toMatch(/<img\b/i);
  });

  it("blocks javascript urls", () => {
    const html = renderMarkdown(`[x](javascript:alert(1))`);
    expect(html).not.toContain("javascript:");
    expect(html).toContain("x");
  });

  it("blocks obfuscated javascript/data/vbscript protocol urls", () => {
    // Direct sanitizeUrl: newlines inside scheme (renderMarkdown splits lines first)
    expect(sanitizeUrl("java\nscript:alert(1)")).toBeNull();
    expect(sanitizeUrl("java\tscript:alert(1)")).toBeNull();
    expect(sanitizeUrl("java\r\nscript:alert(1)")).toBeNull();
    expect(sanitizeUrl("data\n:text/html,hi")).toBeNull();
    expect(sanitizeUrl("vb\tscript:msgbox(1)")).toBeNull();
    expect(sanitizeUrl("\u0000javascript:alert(1)")).toBeNull();

    // Single-line markdown path (tab / NUL stay inside one line)
    for (const md of [
      "[x](java\tscript:alert(1))",
      "[x](vb\tscript:msgbox(1))",
      "[x](\u0000javascript:alert(1))",
    ]) {
      const html = renderMarkdown(md);
      expect(html).not.toMatch(/href=/i);
      expect(html).not.toMatch(/javascript:/i);
      expect(html).not.toMatch(/vbscript:/i);
      expect(html).toContain("x");
    }
  });

  it("allows safe http https mailto anchors and paths", () => {
    expect(renderMarkdown(`[a](https://example.com)`)).toContain('href="https://example.com"');
    expect(renderMarkdown(`[a](http://example.com)`)).toContain('href="http://example.com"');
    expect(renderMarkdown(`[a](mailto:a@b.com)`)).toContain('href="mailto:a@b.com"');
    expect(renderMarkdown(`[a](#section)`)).toContain('href="#section"');
    expect(renderMarkdown(`[a](/abs/path)`)).toContain('href="/abs/path"');
    expect(renderMarkdown(`[a](rel/path.md)`)).toContain('href="rel/path.md"');
  });

  it("detects raw ACP JSON that must not be primary UI", () => {
    expect(
      looksLikeRawAcpJson(
        `{"jsonrpc":"2.0","method":"session/update","params":{}}`,
      ),
    ).toBe(true);
    expect(looksLikeRawAcpJson("Read src/server.ts")).toBe(false);
  });
});

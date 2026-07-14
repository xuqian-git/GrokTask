import { describe, expect, it } from "vitest";
import {
  containsForbiddenUiToken,
  isUnsafePrimaryUiText,
  looksLikeRawAcpJson,
  renderMarkdown,
  safeDisplayLine,
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
    const html = renderMarkdown(
      `<script>alert(1)</script>\n\n<img onerror=alert(1)>`,
    );
    expect(html).not.toContain("<script>");
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("&lt;img");
    expect(html).not.toMatch(/<img\b/i);
  });

  it("blocks javascript urls", () => {
    const html = renderMarkdown(`[x](javascript:alert(1))`);
    expect(html).not.toContain("javascript:");
    expect(html).toContain("x");
  });

  it("blocks obfuscated javascript/data/vbscript protocol urls", () => {
    expect(sanitizeUrl("java\nscript:alert(1)")).toBeNull();
    expect(sanitizeUrl("java\tscript:alert(1)")).toBeNull();
    expect(sanitizeUrl("java\r\nscript:alert(1)")).toBeNull();
    expect(sanitizeUrl("data\n:text/html,hi")).toBeNull();
    expect(sanitizeUrl("vb\tscript:msgbox(1)")).toBeNull();
    expect(sanitizeUrl("\u0000javascript:alert(1)")).toBeNull();

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
    expect(renderMarkdown(`[a](https://example.com)`)).toContain(
      'href="https://example.com"',
    );
    expect(renderMarkdown(`[a](http://example.com)`)).toContain(
      'href="http://example.com"',
    );
    expect(renderMarkdown(`[a](mailto:a@b.com)`)).toContain(
      'href="mailto:a@b.com"',
    );
    expect(renderMarkdown(`[a](#section)`)).toContain('href="#section"');
    expect(renderMarkdown(`[a](/abs/path)`)).toContain('href="/abs/path"');
    expect(renderMarkdown(`[a](rel/path.md)`)).toContain('href="rel/path.md"');
  });

  it("renders tables", () => {
    const html = renderMarkdown(
      `| Rule | Behavior |\n| --- | --- |\n| Order | Arrival |\n| Merge | toolCallId |\n`,
    );
    expect(html).toContain("<table>");
    expect(html).toContain("<th>");
    expect(html).toContain("Order");
    expect(html).toContain("Arrival");
    expect(html).not.toContain("<script");
  });

  it("renders task lists", () => {
    const html = renderMarkdown(`- [x] Done\n- [ ] Todo\n`);
    expect(html).toContain('class="task-list"');
    expect(html).toContain("checked");
    expect(html).toContain("Done");
    expect(html).toContain("Todo");
  });

  it("renders multi-line blockquotes and headings", () => {
    const html = renderMarkdown(`> first\n> second\n\n### Heading\n`);
    expect(html).toContain("<blockquote>");
    expect(html).toContain("first");
    expect(html).toContain("second");
    expect(html).toContain("<h3>");
    expect(html).toContain("Heading");
  });

  it("detects raw ACP JSON that must not be primary UI", () => {
    expect(
      looksLikeRawAcpJson(
        `{"jsonrpc":"2.0","method":"session/update","params":{}}`,
      ),
    ).toBe(true);
    expect(looksLikeRawAcpJson("Read src/server.ts")).toBe(false);
  });

  it("flags non-JSON protocol labels as forbidden UI tokens", () => {
    expect(containsForbiddenUiToken("session/update")).toBe(true);
    expect(containsForbiddenUiToken("tool_call_update")).toBe(true);
    expect(
      containsForbiddenUiToken("ACP 通知：_x.ai/session_notification"),
    ).toBe(true);
    expect(containsForbiddenUiToken("agent_thought_chunk")).toBe(true);
    expect(containsForbiddenUiToken("Read src/server.ts")).toBe(false);
    expect(containsForbiddenUiToken("权限请求 · 已拒绝")).toBe(false);
  });

  it("safeDisplayLine replaces protocol labels with semantic fallbacks", () => {
    expect(safeDisplayLine("session/update", "状态提示")).toBe("状态提示");
    expect(safeDisplayLine("tool_call_update", "权限请求")).toBe("权限请求");
    expect(
      safeDisplayLine("ACP 通知：_x.ai/session_notification", "状态提示"),
    ).toBe("状态提示");
    expect(
      safeDisplayLine(
        `{"jsonrpc":"2.0","method":"session/update"}`,
        "Update",
      ),
    ).toBe("Update");
    expect(safeDisplayLine("Read src/server.ts", "状态提示")).toBe(
      "Read src/server.ts",
    );
    expect(safeDisplayLine("", "状态提示")).toBe("状态提示");
    expect(safeDisplayLine(undefined, "状态提示")).toBe("状态提示");
  });

  it("isUnsafePrimaryUiText covers JSON and human-ish ACP labels", () => {
    expect(isUnsafePrimaryUiText("session/update")).toBe(true);
    expect(
      isUnsafePrimaryUiText(
        `{"jsonrpc":"2.0","method":"tool_call_update"}`,
      ),
    ).toBe(true);
    expect(isUnsafePrimaryUiText("Explored src/lib/markdown.ts")).toBe(false);
  });
});

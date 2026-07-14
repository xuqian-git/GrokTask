import { describe, expect, it } from "vitest";
import {
  applyAutoOnly,
  disclosureKey,
  getExpansion,
  isExpanded,
  mergeServerExpansions,
  toggleUserExpansion,
} from "../src/lib/expansion";

describe("manual expansion preservation", () => {
  it("defaults to auto", () => {
    expect(getExpansion({}, "item-1")).toBe("auto");
  });

  it("user expand is preserved across auto complete", () => {
    let map = toggleUserExpansion({}, "thought-1");
    expect(getExpansion(map, "thought-1")).toBe("user-expanded");
    expect(isExpanded(getExpansion(map, "thought-1"), { streaming: false })).toBe(
      true,
    );

    // Stream completes / auto logic tries to reset
    map = applyAutoOnly(map, "thought-1", "auto");
    expect(getExpansion(map, "thought-1")).toBe("user-expanded");
  });

  it("user collapsed stays collapsed while streaming would auto-open", () => {
    let map = toggleUserExpansion({}, "tool-1"); // expand
    map = toggleUserExpansion(map, "tool-1"); // collapse
    expect(getExpansion(map, "tool-1")).toBe("user-collapsed");
    expect(
      isExpanded(getExpansion(map, "tool-1"), { streaming: true, kind: "tool_call" }),
    ).toBe(false);
  });

  it("auto shows streaming assistant content", () => {
    expect(
      isExpanded("auto", { streaming: true, kind: "assistant_segment" }),
    ).toBe(true);
    expect(
      isExpanded("auto", { streaming: false, kind: "reasoning_segment" }),
    ).toBe(false);
  });

  it("mergeServerExpansions does not overwrite user-expanded with server auto/collapsed", () => {
    const key = disclosureKey("thought-1");
    const local = { [key]: "user-expanded" as const };
    const server = {
      [key]: "user-collapsed" as const,
      [disclosureKey("other")]: "user-expanded" as const,
    };
    const merged = mergeServerExpansions(local, server);
    expect(merged[key]).toBe("user-expanded");
    expect(merged[disclosureKey("other")]).toBe("user-expanded");
  });

  it("mergeServerExpansions does not overwrite user-collapsed with server expanded", () => {
    const key = disclosureKey("tool-1");
    const local = { [key]: "user-collapsed" as const };
    const server = {
      [key]: "user-expanded" as const,
      [disclosureKey("missing-local")]: "auto" as const,
    };
    const merged = mergeServerExpansions(local, server);
    expect(merged[key]).toBe("user-collapsed");
    expect(merged[disclosureKey("missing-local")]).toBe("auto");
  });

  it("mergeServerExpansions overwrites local auto from server", () => {
    const key = disclosureKey("item-a");
    const local = { [key]: "auto" as const };
    const server = { [key]: "user-expanded" as const };
    expect(mergeServerExpansions(local, server)[key]).toBe("user-expanded");
  });
});

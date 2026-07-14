import { describe, expect, it } from "vitest";
import {
  thoughtPreviewLines,
  thoughtStageSummary,
} from "../src/lib/thoughtSummary";

describe("thought stage summary", () => {
  it("prefers stageTitle", () => {
    expect(
      thoughtStageSummary({
        stageTitle: "Checking event order",
        text: "# Other\n\nSomething else.",
      }),
    ).toBe("Checking event order");
  });

  it("falls back to first markdown heading", () => {
    expect(
      thoughtStageSummary({
        text: "## Design the reducer\n\nDetails here.",
      }),
    ).toBe("Design the reducer");
  });

  it("falls back to first complete sentence", () => {
    expect(
      thoughtStageSummary({
        text: "I will inspect the order carefully. More later.",
      }),
    ).toBe("I will inspect the order carefully.");
  });

  it("falls back to plain fragment then default", () => {
    expect(
      thoughtStageSummary({
        text: "no sentence ending yet just words",
      }),
    ).toBe("no sentence ending yet just words");

    expect(thoughtStageSummary({ text: "" })).toBe("思考过程");
    expect(thoughtStageSummary({})).toBe("思考过程");
  });

  it("preview keeps last three non-empty lines", () => {
    const text = "a\n\nb\nc\nd\ne";
    expect(thoughtPreviewLines(text, 3)).toBe("c\nd\ne");
  });
});

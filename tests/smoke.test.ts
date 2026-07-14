import { describe, expect, it } from "vitest";
import { defaultHealth } from "../src/lib/ipc";

describe("frontend smoke", () => {
  it("exposes offline connection health by default", () => {
    expect(defaultHealth()).toEqual({ status: "offline" });
  });
});

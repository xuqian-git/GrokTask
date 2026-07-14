import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import SettingsView from "../src/views/SettingsView.vue";
import * as settings from "../src/lib/settings";

describe("Settings UI (Phase 5)", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    settings.resetSettingsMocksForTests();
  });

  it("renders General tray mode controls from current config", async () => {
    settings.resetSettingsMocksForTests();
    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(w.find('[data-testid="settings-shell"]').exists()).toBe(true);
    expect(w.find('[data-testid="section-general"]').exists()).toBe(true);
    const radios = w.findAll('[data-testid="tray-mode-controls"] input');
    expect(radios.length).toBe(3);
    const checked = radios.find(
      (r) => (r.element as HTMLInputElement).checked,
    );
    expect((checked?.element as HTMLInputElement | undefined)?.value).toBe(
      "off",
    );

    w.unmount();
  });

  it("Integration cards show status, path, binary, and action buttons", async () => {
    settings.resetSettingsMocksForTests();
    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    await w.find('[data-testid="tab-integrations"]').trigger("click");
    await w.vm.$nextTick();

    const codex = w.find('[data-testid="agent-card-codex"]');
    expect(codex.exists()).toBe(true);
    expect(codex.find('[data-testid="agent-status"]').text()).toMatch(
      /Not installed/i,
    );
    expect(codex.find('[data-testid="agent-config-path"]').text()).toContain(
      "config.toml",
    );
    expect(codex.find('[data-testid="agent-binary-path"]').text()).toContain(
      "GrokTask",
    );
    expect(codex.find('[data-testid="agent-install"]').exists()).toBe(true);
    expect(codex.find('[data-testid="agent-remove"]').exists()).toBe(true);
    expect(codex.find('[data-testid="agent-reminder"]').exists()).toBe(true);

    w.unmount();
  });

  it("install refreshes displayed status", async () => {
    settings.resetSettingsMocksForTests();
    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    await w.find('[data-testid="tab-integrations"]').trigger("click");
    await w.vm.$nextTick();

    await w
      .find('[data-testid="agent-card-codex"] [data-testid="agent-install"]')
      .trigger("click");
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(
      w.find('[data-testid="agent-card-codex"] [data-testid="agent-status"]')
        .text(),
    ).toMatch(/Installed/i);
    expect(w.find('[data-testid="action-result"]').text().length).toBeGreaterThan(
      0,
    );

    w.unmount();
  });

  it("invalid/unavailable state disables write actions with clear reason", async () => {
    settings.resetSettingsMocksForTests();
    settings.setMockAgentStatus({
      agent: "claude",
      status: "invalid_config",
      configPath: "~/.claude.json",
      binaryPath: "/mock/GrokTask",
      detail: "invalid JSON: unexpected token",
      canWrite: false,
      canRemove: false,
    });

    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    await w.find('[data-testid="tab-integrations"]').trigger("click");
    await w.vm.$nextTick();

    const card = w.find('[data-testid="agent-card-claude"]');
    expect(
      (card.find('[data-testid="agent-install"]').element as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    expect(
      (card.find('[data-testid="agent-remove"]').element as HTMLButtonElement)
        .disabled,
    ).toBe(true);
    expect(card.find('[data-testid="agent-disabled-reason"]').text()).toMatch(
      /invalid JSON/i,
    );

    w.unmount();
  });

  it("opens Integrations when section=integrations query is set", async () => {
    const original = window.location.search;
    // jsdom: replace search via history
    window.history.replaceState({}, "", "?view=settings&section=integrations");

    settings.resetSettingsMocksForTests();
    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(w.find('[data-testid="section-integrations"]').exists()).toBe(true);

    w.unmount();
    window.history.replaceState({}, "", original || "?");
  });

  it("Diagnostics shows Grok CLI and tray capability", async () => {
    settings.resetSettingsMocksForTests();
    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    await w.find('[data-testid="tab-diagnostics"]').trigger("click");
    await w.vm.$nextTick();

    expect(w.find('[data-testid="section-diagnostics"]').exists()).toBe(true);
    expect(w.find('[data-testid="grok-state"]').exists()).toBe(true);
    expect(w.find('[data-testid="tray-capability"]').exists()).toBe(true);

    w.unmount();
  });
});

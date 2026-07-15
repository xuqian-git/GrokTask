import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import SettingsView from "../src/views/SettingsView.vue";
import * as settings from "../src/lib/settings";

describe("Settings UI (Phase 7)", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    settings.resetSettingsMocksForTests();
    window.history.replaceState({}, "", "?");
  });

  it("renders Chinese General tray mode controls", async () => {
    settings.resetSettingsMocksForTests();
    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(w.find('[data-testid="settings-shell"]').exists()).toBe(true);
    expect(w.find('[data-testid="section-general"]').exists()).toBe(true);
    expect(w.text()).toMatch(/通用|菜单栏/);
    const radios = w.findAll('[data-testid="tray-mode-controls"] input');
    expect(radios.length).toBe(3);
    const checked = radios.find((r) => (r.element as HTMLInputElement).checked);
    expect((checked?.element as HTMLInputElement | undefined)?.value).toBe(
      "active",
    );

    w.unmount();
  });

  it("tools page shows MCP and workflow switches separately", async () => {
    settings.resetSettingsMocksForTests();
    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    await w.find('[data-testid="tab-integrations"]').trigger("click");
    await w.vm.$nextTick();

    expect(w.find('[data-testid="section-integrations"]').exists()).toBe(true);
    expect(w.text()).toMatch(/工具开关/);

    const codex = w.find('[data-testid="agent-card-codex"]');
    expect(codex.exists()).toBe(true);
    expect(codex.find('[data-testid="mcp-layer"]').exists()).toBe(true);
    expect(codex.find('[data-testid="workflow-layer"]').exists()).toBe(true);
    expect(codex.find('[data-testid="agent-status"]').text()).toMatch(/未安装/);
    expect(codex.find('[data-testid="workflow-status"]').text()).toMatch(
      /未启用/,
    );
    expect(codex.find('[data-testid="agent-config-path"]').text()).toContain(
      "config.toml",
    );
    const workflowPath = codex.find('[data-testid="workflow-path"]').text();
    expect(workflowPath).toContain("AGENTS.md");
    expect(workflowPath).toMatch(/\.codex/);
    expect(workflowPath).not.toContain("/mock/workspace");
    expect(w.text()).toMatch(/指令文件（全局）/);
    expect(w.text()).toMatch(/自动触发指令/);
    expect(w.text()).not.toMatch(/协作指令/);
    expect(codex.find('[data-testid="agent-install"]').exists()).toBe(true);
    expect(codex.find('[data-testid="workflow-enable"]').exists()).toBe(true);
    expect(codex.find('[data-testid="agent-reminder"]').exists()).toBe(true);
    expect(codex.find('[data-testid="workflow-reminder"]').text()).not.toMatch(
      /全局指令注入尚未支持/,
    );

    w.unmount();
  });

  it("Settings tab click updates section and URL without second click", async () => {
    settings.resetSettingsMocksForTests();
    window.history.replaceState({}, "", "?view=settings&section=integrations");
    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(w.find('[data-testid="section-integrations"]').exists()).toBe(true);

    await w.find('[data-testid="tab-diagnostics"]').trigger("click");
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(w.find('[data-testid="section-diagnostics"]').exists()).toBe(true);
    expect(w.find('[data-testid="section-integrations"]').exists()).toBe(false);
    expect(window.location.search).toContain("section=diagnostics");
    // Single click was enough — diagnostics is visible immediately.
    expect(w.find('[data-testid="grok-state"]').exists()).toBe(true);

    w.unmount();
  });

  it("initial mount does not fetch doctor report", async () => {
    settings.resetSettingsMocksForTests();
    const spy = vi.spyOn(settings, "fetchDoctorReport");

    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(spy).not.toHaveBeenCalled();
    expect(w.find('[data-testid="section-general"]').exists()).toBe(true);
    expect(w.find('[data-testid="settings-loading"]').exists()).toBe(false);

    w.unmount();
  });

  it("selecting Diagnostics lazy-loads doctor report and avoids duplicate fetches", async () => {
    settings.resetSettingsMocksForTests();
    const spy = vi.spyOn(settings, "fetchDoctorReport");

    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();
    expect(spy).not.toHaveBeenCalled();

    await w.find('[data-testid="tab-diagnostics"]').trigger("click");
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(spy).toHaveBeenCalledTimes(1);
    expect(w.find('[data-testid="section-diagnostics"]').exists()).toBe(true);
    expect(w.find('[data-testid="grok-state"]').exists()).toBe(true);
    expect(w.find('[data-testid="tray-capability"]').exists()).toBe(true);
    expect(w.find('[data-testid="daemon-status"]').exists()).toBe(true);

    // Leave and re-enter Diagnostics — should not re-probe.
    await w.find('[data-testid="tab-general"]').trigger("click");
    await w.vm.$nextTick();
    await w.find('[data-testid="tab-diagnostics"]').trigger("click");
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();
    expect(spy).toHaveBeenCalledTimes(1);

    // Refresh forces a new doctor fetch.
    await w.find('[data-testid="refresh-doctor"]').trigger("click");
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();
    expect(spy).toHaveBeenCalledTimes(2);

    w.unmount();
  });

  it("section=diagnostics deep-link loads doctor after essentials", async () => {
    settings.resetSettingsMocksForTests();
    const spy = vi.spyOn(settings, "fetchDoctorReport");
    window.history.replaceState({}, "", "?view=settings&section=diagnostics");

    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(spy).toHaveBeenCalled();
    expect(w.find('[data-testid="section-diagnostics"]').exists()).toBe(true);
    expect(w.find('[data-testid="grok-state"]').exists()).toBe(true);

    w.unmount();
  });

  it("install refreshes displayed MCP status", async () => {
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
      w
        .find('[data-testid="agent-card-codex"] [data-testid="agent-status"]')
        .text(),
    ).toMatch(/已安装/);
    expect(
      w.find('[data-testid="action-result"]').text().length,
    ).toBeGreaterThan(0);

    w.unmount();
  });

  it("workflow enable updates workflow status independently of MCP", async () => {
    settings.resetSettingsMocksForTests();
    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    await w.find('[data-testid="tab-integrations"]').trigger("click");
    await w.vm.$nextTick();

    await w
      .find('[data-testid="agent-card-codex"] [data-testid="workflow-enable"]')
      .trigger("click");
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    const card = w.find('[data-testid="agent-card-codex"]');
    expect(card.find('[data-testid="workflow-status"]').text()).toMatch(
      /已启用/,
    );
    // MCP remains not installed when only workflow is enabled.
    expect(card.find('[data-testid="agent-status"]').text()).toMatch(/未安装/);

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
      workflowStatus: "invalid_file",
      workflowPath: "/mock/home/.claude/CLAUDE.md",
      workflowDetail: "malformed GrokTask managed block markers",
      canWriteWorkflow: false,
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
    expect(
      (
        card.find('[data-testid="workflow-enable"]')
          .element as HTMLButtonElement
      ).disabled,
    ).toBe(true);

    w.unmount();
  });

  it("opens 工具开关 when section=integrations query is set", async () => {
    const original = window.location.search;
    window.history.replaceState({}, "", "?view=settings&section=integrations");

    settings.resetSettingsMocksForTests();
    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(w.find('[data-testid="section-integrations"]').exists()).toBe(true);
    expect(w.text()).toMatch(/工具开关|MCP/);

    w.unmount();
    window.history.replaceState({}, "", original || "?");
  });

  it("tools page is global-only and hides project workspace copy", async () => {
    settings.resetSettingsMocksForTests();
    settings.setMockWorkspaceCwd("");

    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    await w.find('[data-testid="tab-integrations"]').trigger("click");
    await w.vm.$nextTick();

    expect(w.find('[data-testid="workspace-cwd"]').exists()).toBe(false);
    expect(w.find('[data-testid="workspace-cwd-missing"]').exists()).toBe(
      false,
    );
    expect(w.text()).not.toMatch(/当前工作区|项目工作区|GrokTask setup/);

    const card = w.find('[data-testid="agent-card-codex"]');
    const workflowPath = card.find('[data-testid="workflow-path"]').text();
    expect(workflowPath).toMatch(/\.codex\/AGENTS\.md/);
    expect(workflowPath).not.toContain("/mock/workspace");
    // Workflow buttons stay enabled without workspace (only canWriteWorkflow/busy gate).
    expect(
      (
        card.find('[data-testid="workflow-enable"]')
          .element as HTMLButtonElement
      ).disabled,
    ).toBe(false);
    expect(
      (
        card.find('[data-testid="workflow-disable"]')
          .element as HTMLButtonElement
      ).disabled,
    ).toBe(false);
    expect(card.find('[data-testid="workflow-disabled-reason"]').exists()).toBe(
      false,
    );

    w.unmount();
  });

  it("Diagnostics shows Grok CLI and tray capability", async () => {
    settings.resetSettingsMocksForTests();
    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    await w.find('[data-testid="tab-diagnostics"]').trigger("click");
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(w.find('[data-testid="section-diagnostics"]').exists()).toBe(true);
    expect(w.find('[data-testid="grok-state"]').exists()).toBe(true);
    expect(w.find('[data-testid="tray-capability"]').exists()).toBe(true);
    expect(w.text()).toMatch(/诊断/);

    w.unmount();
  });

  it("history settings can edit retention limit and clear history", async () => {
    settings.resetSettingsMocksForTests();
    const limitSpy = vi.spyOn(settings, "setHistoryLimit");
    const clearSpy = vi.spyOn(settings, "clearHistory");
    vi.spyOn(window, "confirm").mockReturnValue(true);

    const w = mount(SettingsView, { attachTo: document.body });
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    await w.find('[data-testid="tab-history"]').trigger("click");
    await w.vm.$nextTick();

    const input = w.find('[data-testid="history-limit-input"]');
    expect(input.exists()).toBe(true);
    await input.setValue("123");
    await w.find('[data-testid="history-limit-form"]').trigger("submit");
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(limitSpy).toHaveBeenCalledWith(123);
    expect(w.find('[data-testid="action-result"]').text()).toMatch(/123/);

    await w.find('[data-testid="clear-history"]').trigger("click");
    await new Promise((r) => setTimeout(r, 20));
    await w.vm.$nextTick();

    expect(clearSpy).toHaveBeenCalled();
    expect(w.find('[data-testid="action-result"]').text()).toMatch(/清空/);

    w.unmount();
  });
});

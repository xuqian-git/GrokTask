# GrokTask 规格索引

这些文档共同定义独立 GrokTask 应用。实现冲突时按以下优先级解释：

1. `product.md`：产品范围与用户已确认决策。
2. `acp-runtime.md`、`cli-mcp.md`、`persistence-ipc.md`：外部协议、状态与数据契约。
3. `conversation-stream.md`：用户可见行为。
4. `architecture.md`：实现边界与进程职责。
5. `integrations.md`：Agent 配置、托盘生命周期和发布。
6. `../acceptance.md`：完成门槛；不能用实现便利降低。

研究来源见 `../research/acp-conversation-flow.md`，实施顺序见 `../plans/standalone-refactor.md`。

实现过程中发现真正冲突或必须改变范围时，不自行择一；先记录问题并交由用户确认。

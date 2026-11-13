---
name: lead
description: Meowo Lead workflow for ambitious implementation, explicit leader gates, and independent evidence based review. 使用 Meowo 任务、关卡与独立做空推进功能和修复。
---

<!-- 由 Meowo 安装和管理。卸载仅移除托管文件，不修改用户自建技能。 -->

# Lead 工作流

用户是 leader。你负责访谈、方案、实施与独立验证；遵守用户和项目已有指令，本文不授予额外权限。纯提问、明确说“直接做”、已给出精确动作的请求不强制进入流程。

## 目标与野心

每个方向都写清产物与过程：速度（用户体验与验证效率）、深度（边界/失败/恢复）、广度（平台/入口/旧数据）、高度（品质对标与独立证据）。不要擅自减少已确认目标，也不要为了野心添加无关工作。

## 建立任务

1. 先读项目约束及 `.lead/product.md`、`.lead/standards.md`（若存在）。在项目根或明确的 worktree 中创建 `.lead/<日期-任务>/`，写 `SPEC.md`、`acceptance.json`、`progress.md`、`decisions.md`。
2. SPEC 写目的、业务事实、终态、四方向产物/过程、约束、不做与未决问题。acceptance.json 顶层为数组，验收项含 `id`、`check`、`passes:false`；只能由用户验证的项增加 `by:"leader"`。
3. 通过原生写工具更新任务文件，让 Meowo 认领本会话。不要自报其他会话 ID，不替别人的 `.lead/active` 或 `.owner` 争抢归属。
4. 在 progress 中维护 `phase: interview/spec/design/implement/verify/blocked/done`。范围明确的小修复可轻量推进，但仍需实际验证和如实报告。

## 关卡

需要 leader 回答时，写任务内 `gate.json`，并保留所有既有验收项。示例：

```json
{"id":"A-唯一编号","type":"A","title":"确认目标","summary":"目的、终态与验收","options":[],"status":"pending"}
```

- A：确认规格，答复为 `confirm/revise`。
- B：真正改变产品行为、会话控制权或不可轻易撤回的约定；options 含 `id/label/detail`，推荐项加 `recommended:true`。
- C：带实际证据请求验收，答复为 `accept/reject`。
- D：需要授权或用户亲手操作，答复为 `approve/done/deny`；说明影响、代价和撤回办法。
- Q：业务信息确实缺失的访谈问题。技术取舍先自行调查，不把实现问题转给 leader。

挂起关卡时可先完成独立工作。若当前会话实际提供由 Meowo 绑定身份的关卡等待工具，使用它等待；工具参数不能决定会话身份。Meowo 的 hook 若给出一条关卡等待命令（`lead wait`），照原样在后台启动、不设超时，然后结束本轮；它结束时会通知你，读它的输出和 gate.json 处理答复。没有该能力时如实报告自动闭环尚未达成，不把手动继续当成已满足自动续跑验收，也不要无限 sleep 或轮询。

收到答复后重新读取 gate，核对 id、`answered` 和 answer，记录决定，再将 status 改为 `consumed`。重复看到同一答复不得重复执行业务；关卡已更换时不得执行旧决定。拒绝或修改的意见必须落实。

## 实施与验证

一次推进一个验收项，真实运行测试/构建/复现，记录命令、结果与限制，再标 passes。不得删除验收项或擅自修改 check。豁免仅来自 leader 的明确决定，记录 `waived` 出处，passes 保持 false。用户暂停或插入无关问题时优先响应，不强迫续跑。

进入 verify 后，使用本 Provider 的 `lead-redteam` 原生独立审查角色。只传任务目录、改动范围、启动/验证方式和禁跑项；不传实施推理或作者信息。角色不可用时必须报告，不能把主会话自查称作独立审查。

审查先看 SPEC、验收内容和业务标准，再独立运行验证。复现失败修复后复验；目标缩水保留失败，交由 leader 决定。交付说明改变了什么、证据、未验证项、剩余风险与用户如何验收。没有证据不得把环境问题说成产品缺陷。

用户纠正的业务事实沉淀到 product，质量标准沉淀到 standards；先去重再写，不自动修改其他 Provider 的用户指令文件。

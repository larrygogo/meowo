<!-- 草稿：发版时并入 vX.Y.Z.md（release.yml 只认与 tag 同名的文件）。英文段落供官网和英文 README 使用。 -->

这一版加了 Lead 工作流：你只说想要什么结果，agent 自己访谈你、写规格、出几种方案、动手实施，再找一个独立的审查者专门挑毛病。它只在三个地方停下来等你：确认规格、拍板不可逆的决定、看证据验收。这三个关卡在手机上也能答，隔几个小时再答也不会丢。

## Lead 工作流

- 设置 → Lead 工作流，打开后装进 Claude Code，关掉卸载干净。不会动你自己的 hook 和 skill
- 不用输入命令：你描述想要的功能或报一个 bug，agent 自己判断走完整流程、轻量流程，还是直接做。说「直接做」就跳过
- 停靠栏多了 Lead 入口：阶段、验收通过了几条、规格、决策记录、进度，桌面和手机都能看
- 关卡卡片：确认规格、选方案、验收或驳回，可以附意见。答完 agent 自动接着做，不用回终端
- 关卡在等你时电脑不休眠，方便在手机上作答。设置里可以单独关
- 实施阶段验收没做完，agent 不能自己停；你插话、提问、说「停一下」都不受影响
- 规则和进度写在项目的 `.lead/` 目录里，上下文压缩、重开会话后会自动带回去

暂时只支持 Claude Code，其他 agent 在后续版本加。

---

## Lead workflow (English)

Describe the outcome you want. The agent interviews you, writes a spec, lays out options, builds it, and has an independent reviewer try to break it. It only stops for you at three gates: confirm the spec, make the irreversible calls, and accept the result with evidence. You can answer gates from your phone, hours later, and nothing gets lost.

- Settings → Lead workflow installs it into Claude Code; turning it off removes it cleanly. Your own hooks and skills are left alone
- No commands to type: describe a feature or report a bug and the agent picks the full flow, a light flow, or just does it. Say "just do it" to skip
- A new Lead entry in the dock shows the phase, how many checks pass, the spec, decisions and progress, on desktop and mobile
- Gate cards: confirm the spec, pick an option, accept or reject, with an optional note. The agent picks up on its own once you answer
- While a gate waits for you, the computer stays awake so you can answer from your phone. You can turn this off separately
- During implementation the agent can't stop with checks still failing; asking a question, chiming in, or saying "stop" still works
- Rules and progress live in the project's `.lead/` folder and come back after context compaction or a new session

Claude Code only for now; other agents come later.

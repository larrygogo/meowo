# decisions

- D0：沿用 Meowo 现有原生子 WebView + MCP 链路，不引入第二套浏览器运行时。
- D0：用户现有“接管/交还”是安全与控制权底线，任何对标 Kimi 的输入锁定都不能移除人工恢复出口。
- D1（leader，关卡 A-20261116-browser-spec）：确认按 SPEC 推进研究、实施与 A1–A7 验证，保留人工接管出口，不重写浏览器、不扩展到手机面板。
- D2：采纳输入隔离、单页签串行执行、在途操作响应接管；多页签显式控制、快照协议改版、设备模拟、完整下载管理器不纳入本轮。依据与未确认项见 research.md。
- D3：不宣称复刻 Kimi 原生输入锁。Kimi 主进程只证实键鼠监听与控制 ACK。Meowo 在 Windows 使用每个子 WebView 独立 HWND 的原生 `EnableWindow`，不暴露 JS 控制接口；macOS 逐 WKWebView 原生隔离尚未实现，仅锁定工具栏并如实报告限制。
- D4（leader，关卡 B-20261116-browser-control）：选择 control-reliability，批准实施输入隔离、页签串行、在途接管中止，并完成前后端、视觉、真实链路与独立审查验证。
- D5（leader，关卡 D-20261116-browser-security-prompt）：approve；核对 Windows 安全中心提示后授权继续复跑真实点击 E2E。Agent 不修改 Windows 安全设置。
- D6（leader，关卡 C-20261116-browser-agent-final）：reject，意见「为什么无法验收」。不接受当前 6/8 状态作为完成；需说明 A1 证据缺口与 A5 真实点击阻断，保持两项未通过。
- D7（leader，关卡 C-20261116-browser-agent-final-2）：accept；A5 真实链路补齐后接受本轮交付。A1 保持 passes=false 并作为明确豁免的研究限制；macOS 逐 WKWebView 输入隔离限制保留。

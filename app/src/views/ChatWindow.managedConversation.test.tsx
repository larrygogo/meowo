import { describe, expect, it } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke, adapter, terminalMounts } from "./chatWindow.harness";
import { ChatWindow } from "./ChatWindow";
import { chatUi } from "../test/agents";
import { zh } from "../i18n/zh";

function setup(id:number,phase="ready",extra:Record<string,unknown>={}) {
 window.history.replaceState({},"",`/?sessionId=${id}`);
 const frame={...adapter.history(id,"kimi",phase==="ready"?"idle":"running"),ccSessionId:"native-root",ptyManaged:false,endable:true,managedConversation:true,managedPhase:phase,managedInteraction:null,managedModels:[],managedModes:[],managedDeliveries:[],...extra};
 const sendCalls:Record<string,unknown>[]=[];
 invoke.mockImplementation((command:string,args?:Record<string,unknown>)=>{
  if(command==="get_chat_history")return Promise.resolve(frame);
  if(command==="agent_chat_ui")return Promise.resolve(chatUi("kimi"));
  if(command==="managed_conversation_send"){sendCalls.push(args!);return Promise.resolve({sessionId:id,deliveryId:args!.deliveryId,state:"queued"});}
  if(command==="managed_conversation_control")return Promise.resolve({sessionId:id,phase:"ready"});
  if(command==="managed_conversation_resolve"||command==="managed_conversation_set_model"||command==="managed_conversation_set_mode")return Promise.resolve();
  if(command==="pending_interaction")return Promise.resolve({approval:null,question:null});
  return Promise.resolve();
 });
 return {frame,sendCalls};
}
const writes=()=>invoke.mock.calls.filter(([command])=>["write_managed_terminal","start_managed_terminal","managed_terminal_frame","managed_terminal_snapshot"].includes(command));

describe("protocol managed ChatWindow",()=>{
 it("normal and queued messages use durable conversation commands and never inspect or write a PTY",async()=>{
  const {sendCalls}=setup(901,"running");const mounted=terminalMounts.count;render(<ChatWindow/>);
  await screen.findByText("适配-901");fireEvent.change(adapter.box(),{target:{value:"Keep this exact message"}});fireEvent.keyDown(adapter.box(),{key:"Enter"});
  await waitFor(()=>expect(sendCalls).toHaveLength(1));expect(sendCalls[0]).toMatchObject({sessionId:901,text:"Keep this exact message",attachments:[],gateRef:null});
  expect(typeof sendCalls[0].deliveryId).toBe("string");await waitFor(()=>expect(adapter.box().value).toBe(""));
  expect(writes()).toEqual([]);expect(terminalMounts.count).toBe(mounted);
 });
 // 验收 #9「自动送达不覆盖草稿」：手机作答后的 Lead 答复由后端直接投进托管会话，用户正在输入框里写的
 // 草稿必须原样保留，也不能被顺手发出去。兄弟场景：投递进入运行中、投递完成两次刷新都不动草稿。
 it("a Lead answer delivered in the background never clears or sends the composer draft",async()=>{
  const {frame,sendCalls}=setup(910);render(<ChatWindow/>);await screen.findByText("适配-910");
  fireEvent.change(adapter.box(),{target:{value:"my unsent draft"}});
  const lead={sessionId:910,deliveryId:"lead-1",ordinal:1,text:"[lead] leader 已在 Meowo 里回答关卡",origin:"user",attachments:[],gateRef:{taskDir:"C:/p/.lead/t",gateId:"Q-1"},state:"accepted",nativeMessageId:null,lastError:null,acceptedAt:1,consumedAt:null,completedAt:null,createdAt:1,updatedAt:1};
  Object.assign(frame,{managedPhase:"running",managedDeliveries:[lead]});
  const polls=()=>invoke.mock.calls.filter(([command])=>command==="get_chat_history").length;
  const before=polls();await waitFor(()=>expect(polls()).toBeGreaterThan(before+1),{timeout:4000});
  expect(adapter.box().value).toBe("my unsent draft");
  Object.assign(frame,{managedPhase:"ready",managedDeliveries:[{...lead,state:"completed",consumedAt:2,completedAt:3}]});
  const again=polls();await waitFor(()=>expect(polls()).toBeGreaterThan(again+1),{timeout:4000});
  expect(adapter.box().value).toBe("my unsent draft");
  expect(sendCalls).toEqual([]);expect(writes()).toEqual([]);
 });
 it("a network retry of the same draft reuses its stable delivery ID",async()=>{
  setup(902);const original=invoke.getMockImplementation()!;let fails=true;
  invoke.mockImplementation((command,args)=>command==="managed_conversation_send"&&fails?(fails=false,Promise.reject("conversation/transport_error")):original(command,args));
  render(<ChatWindow/>);await screen.findByText("适配-902");fireEvent.change(adapter.box(),{target:{value:"Do not send twice"}});fireEvent.keyDown(adapter.box(),{key:"Enter"});
  await waitFor(()=>expect(invoke.mock.calls.filter(([cmd])=>cmd==="managed_conversation_send")).toHaveLength(1));
  await waitFor(()=>expect(adapter.box().value).toBe("Do not send twice"));
  fireEvent.keyDown(adapter.box(),{key:"Enter"});
  await waitFor(()=>expect(invoke.mock.calls.filter(([cmd])=>cmd==="managed_conversation_send")).toHaveLength(2));
  const calls=invoke.mock.calls.filter(([cmd])=>cmd==="managed_conversation_send");expect(calls[0][1].deliveryId).toBe(calls[1][1].deliveryId);expect(writes()).toEqual([]);
 });
 it("reload after a lost send response queries the persisted ID and never creates a duplicate request", async () => {
  setup(906); const original = invoke.getMockImplementation()!; let deliveryId: unknown;
  invoke.mockImplementation((command, args) => {
   if (command === "managed_conversation_send") { deliveryId = args!.deliveryId; return Promise.reject("conversation/transport_error"); }
   if (command === "managed_conversation_deliveries") return Promise.resolve(deliveryId ? [{ deliveryId, state: "completed" }] : []);
   return original(command, args);
  });
  const first = render(<ChatWindow/>); await screen.findByText("适配-906");
  fireEvent.change(adapter.box(), { target: { value: "Saved by server before disconnect" } }); fireEvent.keyDown(adapter.box(), { key: "Enter" });
  await waitFor(() => expect(deliveryId).toBeTypeOf("string"));
  await waitFor(() => expect(document.querySelector(".chat-send-error")).toBeTruthy());
  first.unmount(); render(<ChatWindow/>); await screen.findByText("适配-906");
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("managed_conversation_deliveries", { sessionId: 906 }));
  await waitFor(() => expect(adapter.box().value).toBe(""));
  expect(invoke.mock.calls.filter(([command]) => command === "managed_conversation_send")).toHaveLength(1);
 });
 it("blocked permission cannot consume ordinary text as an answer; explicit option uses native resolve",async()=>{
  setup(903,"blocked",{pendingReview:"approval",managedInteraction:{interactionId:"epoch:permission",title:"Native permission",detail:"Approve exact request",choices:[{id:"native-reject",label:"Reject exact request",kind:"reject_once"}],questions:[]}});
  render(<ChatWindow/>);await screen.findByText("适配-903");await screen.findByText("Native permission");
  expect(invoke.mock.calls.some(([cmd])=>cmd==="managed_conversation_resolve")).toBe(false);
  fireEvent.change(adapter.box(),{target:{value:"Queue a separate message"}});fireEvent.keyDown(adapter.box(),{key:"Enter"});
  await waitFor(()=>expect(invoke).toHaveBeenCalledWith("managed_conversation_send",expect.objectContaining({text:"Queue a separate message"})));
  fireEvent.click(screen.getByRole("button",{name:"Reject exact request"}));
  await waitFor(()=>expect(invoke).toHaveBeenCalledWith("managed_conversation_resolve",{sessionId:903,interactionId:"epoch:permission",optionId:"native-reject",answers:null}));expect(writes()).toEqual([]);
 });
 it("native catalogs switch model and mode through structured commands with no slash menu",async()=>{
  setup(904,"ready",{model:"native-one",agentModes:[{dimension:"mode",value:"normal"}],managedModeDimension:"mode",managedModels:[{id:"native-one",label:"One",kind:"model"},{id:"native-two",label:"Two",kind:"model"}],managedModes:[{id:"normal",label:"Normal",kind:"mode"},{id:"plan",label:"Plan",kind:"mode"}]});
  render(<ChatWindow/>);await screen.findByText("适配-904");
  // 2026-11-13 改为与原生会话同款的菜单（不再是系统 <select>，leader 截图嫌丑）：按钮显示当前项名字。
  fireEvent.click(screen.getByRole("button",{name:`${zh.chat.engineModel}: One`}));
  fireEvent.click(screen.getByRole("menuitem",{name:"Two"}));
  fireEvent.click(screen.getByRole("button",{name:`${zh.chat.engineMode}: Normal`}));
  fireEvent.click(screen.getByRole("menuitem",{name:"Plan"}));
  await waitFor(()=>expect(invoke).toHaveBeenCalledWith("managed_conversation_set_model",{sessionId:904,model:"native-two"}));
  expect(invoke).toHaveBeenCalledWith("managed_conversation_set_mode",{sessionId:904,dimension:"mode",mode:"plan"});expect(writes()).toEqual([]);
 });
 // 兄弟场景：当前值不在清单里时按钮显示原生报的模型名而不是「请选择」；点当前项不发命令；菜单 Esc 关闭。
 it("model picker never shows a placeholder and re-picking the current item sends nothing",async()=>{
  setup(907,"ready",{model:"unlisted-native",managedModels:[{id:"native-one",label:"One",kind:"model"}],managedModes:[]});
  render(<ChatWindow/>);await screen.findByText("适配-907");
  const button=screen.getByRole("button",{name:`${zh.chat.engineModel}: unlisted-native`});
  expect(screen.queryByText(zh.chat.engineChooseAnswer)).toBeNull();
  fireEvent.click(button);expect(screen.getByRole("menu",{name:zh.chat.engineModel})).toBeTruthy();
  fireEvent.keyDown(screen.getByRole("menu",{name:zh.chat.engineModel}),{key:"Escape"});
  await waitFor(()=>expect(screen.queryByRole("menu",{name:zh.chat.engineModel})).toBeNull());
 });
 it("picking the current model closes the menu without a native command",async()=>{
  setup(909,"ready",{model:"native-one",managedModels:[{id:"native-one",label:"One",kind:"model"},{id:"native-two",label:"Two",kind:"model"}],managedModes:[]});
  render(<ChatWindow/>);await screen.findByText("适配-909");
  fireEvent.click(screen.getByRole("button",{name:`${zh.chat.engineModel}: One`}));
  fireEvent.click(screen.getByRole("menuitem",{name:"One"}));
  expect(screen.queryByRole("menu")).toBeNull();
  expect(invoke.mock.calls.some(([command])=>command==="managed_conversation_set_model")).toBe(false);
 });
 it("cancel goes directly to the manager without terminal interrupt keys",async()=>{
  setup(905,"running");render(<ChatWindow/>);await screen.findByText("适配-905");
  fireEvent.change(adapter.box(),{target:{value:"Next after cancel"}});fireEvent.keyDown(adapter.box(),{key:"Enter",ctrlKey:true});
  await waitFor(()=>expect(invoke).toHaveBeenCalledWith("managed_conversation_control",{sessionId:905,action:"cancel"}));
  await waitFor(()=>expect(invoke).toHaveBeenCalledWith("managed_conversation_send",expect.objectContaining({text:"Next after cancel"})));expect(writes()).toEqual([]);
 });
});

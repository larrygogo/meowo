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
  fireEvent.change(screen.getByRole("combobox",{name:zh.chat.engineModel}),{target:{value:"native-two"}});
  fireEvent.change(screen.getByRole("combobox",{name:zh.chat.engineMode}),{target:{value:"plan"}});
  await waitFor(()=>expect(invoke).toHaveBeenCalledWith("managed_conversation_set_model",{sessionId:904,model:"native-two"}));
  expect(invoke).toHaveBeenCalledWith("managed_conversation_set_mode",{sessionId:904,dimension:"mode",mode:"plan"});expect(writes()).toEqual([]);
 });
 it("cancel goes directly to the manager without terminal interrupt keys",async()=>{
  setup(905,"running");render(<ChatWindow/>);await screen.findByText("适配-905");
  fireEvent.change(adapter.box(),{target:{value:"Next after cancel"}});fireEvent.keyDown(adapter.box(),{key:"Enter",ctrlKey:true});
  await waitFor(()=>expect(invoke).toHaveBeenCalledWith("managed_conversation_control",{sessionId:905,action:"cancel"}));
  await waitFor(()=>expect(invoke).toHaveBeenCalledWith("managed_conversation_send",expect.objectContaining({text:"Next after cancel"})));expect(writes()).toEqual([]);
 });
});

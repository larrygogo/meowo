import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
const api=vi.hoisted(()=>({managedConversationResolve:vi.fn()}));
vi.mock("../../api",async(original)=>({...await original<typeof import("../../api")>(),...api}));
import { ManagedDeliveryStatus, ManagedInteraction } from "./ManagedInteraction";
import type { ManagedDeliveryDto, ManagedInteractionDto } from "../../api";
import { zh } from "../../i18n/zh";
const base:ManagedInteractionDto={interactionId:"runtime:permission-1",title:"Run approved command",detail:"Only this native request",choices:[{id:"native-no",label:"Deny this command",kind:"reject_once"},{id:"native-once",label:"Allow this command",kind:"allow_once"}],questions:[]};
beforeEach(()=>{api.managedConversationResolve.mockResolvedValue(undefined);});
afterEach(()=>{cleanup();vi.clearAllMocks();});

it("mount never approves; explicit choice keeps the original native request and option IDs",async()=>{
 const done=vi.fn();render(<ManagedInteraction sessionId={9} interaction={base} onResolved={done}/>);
 expect(api.managedConversationResolve).not.toHaveBeenCalled();
 fireEvent.click(screen.getByRole("button",{name:"Deny this command"}));
 await waitFor(()=>expect(api.managedConversationResolve).toHaveBeenCalledWith(9,"runtime:permission-1","native-no",null));
 await waitFor(()=>expect(done).toHaveBeenCalledOnce());
});

it("native form requires explicit single and multi-select answers and retains native field IDs",async()=>{
 const form:ManagedInteractionDto={...base,interactionId:"runtime:form-2",choices:[{id:"accept",label:"Submit answers",kind:"accept"},{id:"cancel",label:"Cancel form",kind:"cancel"}],questions:[{id:"q0",title:"Theme",text:"Pick theme",multiple:false,options:[{id:"dark",label:"Dark",kind:"option"}]},{id:"q1",title:"Features",text:"",multiple:true,options:[{id:"native-a",label:"Feature A",kind:"option"}]}]};
 render(<ManagedInteraction sessionId={9} interaction={form} onResolved={vi.fn()}/>);
 const submit=screen.getByRole("button",{name:"Submit answers"}) as HTMLButtonElement;
 expect(submit.disabled).toBe(true);
 fireEvent.change(screen.getByRole("combobox",{name:"Theme"}),{target:{value:"dark"}});
 fireEvent.click(screen.getByRole("checkbox",{name:"Feature A"}));
 expect(submit.disabled).toBe(false);fireEvent.click(submit);
 await waitFor(()=>expect(api.managedConversationResolve).toHaveBeenCalledWith(9,"runtime:form-2","accept",{q0:"dark",q1:["native-a"]}));
});

it("failed resolution retains the request card and allows an explicit retry",async()=>{
 api.managedConversationResolve.mockRejectedValue("conversation/interaction_stale");
 const done=vi.fn();render(<ManagedInteraction sessionId={9} interaction={base} onResolved={done}/>);
 fireEvent.click(screen.getByRole("button",{name:"Allow this command"}));
 expect(await screen.findByRole("alert")).toBeTruthy();expect(done).not.toHaveBeenCalled();
 expect(screen.getByText(base.detail)).toBeTruthy();
});

const delivery: ManagedDeliveryDto = { sessionId: 9, deliveryId: "delivery_1", ordinal: 1, text: "Private Lead continuation context", origin: "lead", attachments: [], gateRef: null, state: "uncertain", nativeMessageId: null, lastError: null, acceptedAt: null, consumedAt: null, completedAt: null, createdAt: 1, updatedAt: 1 };
it("internal Lead delivery identifies its source and cannot restore internal text as user draft", () => {
 const restore = vi.fn(); const retry = vi.fn(); const dismiss = vi.fn();
 render(<ManagedDeliveryStatus deliveries={[delivery]} engineLive onRestore={restore} onRetry={retry} onDismiss={dismiss}/>);
 expect(screen.getByText(zh.chat.engineLeadContinuation)).toBeTruthy();
 expect(screen.queryByText(delivery.text)).toBeNull();
 expect(screen.queryByRole("button", { name: zh.chat.engineRestoreDraft })).toBeNull();
 fireEvent.click(screen.getByRole("button", { name: zh.chat.engineStopRetry }));
 expect(retry).toHaveBeenCalledWith(delivery); expect(restore).not.toHaveBeenCalled();
 fireEvent.click(screen.getByRole("button", { name: zh.chat.engineStopDismiss }));
 expect(dismiss).toHaveBeenCalledWith(delivery);
});

it("user uncertain delivery preserves original text for an explicit draft restoration", () => {
 const user = { ...delivery, origin: "user", text: "My original message" }; const restore = vi.fn();
 render(<ManagedDeliveryStatus deliveries={[user]} onRestore={restore} onRetry={vi.fn()} onDismiss={vi.fn()}/>);
 fireEvent.click(screen.getByRole("button", { name: zh.chat.engineRestoreDraft }));
 expect(restore).toHaveBeenCalledWith(user);
});

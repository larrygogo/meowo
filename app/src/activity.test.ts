import { describe, expect, it } from "vitest";
import { activityTone } from "./activity";

describe("activityTone", () => {
  /// Lead 关卡等 leader 拍板时（后端关卡巡检写 pending_review=gate），看板/侧栏都算「待处理」，
  /// 即使 agent 本身已停下、屏幕 idle。
  it("Lead 关卡挂起算待处理", () => {
    expect(activityTone({ status: "waiting", pendingReview: "gate", screenState: "idle" })).toBe("pending");
    expect(activityTone({ status: "waiting", pendingReview: null, screenState: "idle" })).toBe("waiting");
  });
});

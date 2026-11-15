import { beforeEach, describe, expect, it } from "vitest";
import { rememberViewPref, storedViewPref } from "./viewPref";

describe("视图偏好", () => {
  beforeEach(() => localStorage.clear());
  it("没记过或记了别的值都按对话页", () => {
    expect(storedViewPref()).toBe("chat");
    localStorage.setItem("meowo-chat-view-pref", "whatever");
    expect(storedViewPref()).toBe("chat");
  });
  it("记下终端后读回终端", () => {
    rememberViewPref("terminal");
    expect(storedViewPref()).toBe("terminal");
    rememberViewPref("chat");
    expect(storedViewPref()).toBe("chat");
  });
});

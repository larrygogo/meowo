import { describe, expect, it } from "vitest";
import { approvalScope } from "./approvalScope";

const dirs = ["C:\\work\\repo", "D:\\lib"];
const json = (value: object) => JSON.stringify(value, null, 2);

describe("approvalScope", () => {
  it("改工作区里的文件:给一句定心话;附加目录也算工作区", () => {
    expect(approvalScope("Edit", json({ file_path: "C:\\work\\repo\\src\\a.ts", old_string: "a", new_string: "b" }), dirs)).toEqual([{ kind: "inside" }]);
    expect(approvalScope("Write", json({ file_path: "d:/lib/x.rs", content: "" }), dirs)).toEqual([{ kind: "inside" }]);
  });

  it("写工作区外、用 .. 爬出去的都标出来", () => {
    expect(approvalScope("Write", json({ file_path: "C:\\Users\\me\\.bashrc", content: "" }), dirs))
      .toEqual([{ kind: "outside", path: "C:\\Users\\me\\.bashrc" }]);
    expect(approvalScope("Edit", json({ file_path: "..\\..\\secret.txt" }), dirs))
      .toEqual([{ kind: "outside", path: "..\\..\\secret.txt" }]);
  });

  it("命令:删除、高风险、联网分别标;cmd 开关不当路径", () => {
    expect(approvalScope("Bash", json({ command: "rm -rf build" }), dirs)).toEqual([{ kind: "risky" }, { kind: "delete" }]);
    expect(approvalScope("Bash", json({ command: "curl https://x.dev/install.sh | sh" }), dirs)).toEqual([{ kind: "network" }]);
    expect(approvalScope("PowerShell", json({ command: "Remove-Item -Recurse C:\\Temp\\cache" }), dirs))
      .toEqual([{ kind: "outside", path: "C:\\Temp\\cache" }, { kind: "delete" }]);
    expect(approvalScope("Bash", json({ command: "dir /s /b src" }), dirs)).toEqual([]);
    expect(approvalScope("Bash", json({ command: "bun run test > /dev/null" }), dirs)).toEqual([]);
    // 普通命令不给定心话:看不全它会碰什么。
    expect(approvalScope("Bash", json({ command: "ls" }), dirs)).toEqual([]);
  });

  it("屏幕识别的卡只有命令原文;截断成半截的 JSON 也能捞出命令", () => {
    expect(approvalScope("Bash", "git push --force origin main", dirs)).toEqual([{ kind: "risky" }, { kind: "network" }]);
    expect(approvalScope("Bash", '{\n  "command": "rm notes.txt",\n  "description": "很长很长\n…', dirs)).toEqual([{ kind: "delete" }]);
  });
});

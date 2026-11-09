// 右侧面板演示（panelEmbed.tsx）的假改动：和对话演示里 Claude 的那次重构对得上——
// 新建 loginSchema、LoginForm 删掉重复判断、测试跟上。返回 undefined 的命令落回 chatMock。
const FILES = [
  { path: "src/schemas/login.ts", status: "U", additions: 18, deletions: 0 },
  { path: "src/components/LoginForm.tsx", status: "M", additions: 6, deletions: 21 },
  { path: "src/components/LoginForm.test.tsx", status: "M", additions: 9, deletions: 2 },
];

const DIFFS: Record<string, string> = {
  "src/schemas/login.ts": [
    "--- /dev/null",
    "+++ b/src/schemas/login.ts",
    "@@ -0,0 +1,18 @@",
    "+import { z } from \"zod\";",
    "+",
    "+export const loginSchema = z.object({",
    "+  email: z.string().trim().email(),",
    "+  password: z.string().min(8),",
    "+  remember: z.boolean().default(false),",
    "+});",
    "+",
    "+export type LoginInput = z.infer<typeof loginSchema>;",
    "+",
    "+export function validateLogin(input: unknown) {",
    "+  const result = loginSchema.safeParse(input);",
    "+  if (result.success) return { ok: true as const, data: result.data };",
    "+  return {",
    "+    ok: false as const,",
    "+    errors: result.error.flatten().fieldErrors,",
    "+  };",
    "+}",
  ].join("\n"),
  "src/components/LoginForm.tsx": [
    "--- a/src/components/LoginForm.tsx",
    "+++ b/src/components/LoginForm.tsx",
    "@@ -1,9 +1,8 @@",
    " import { useState } from \"react\";",
    "-import { isEmail } from \"../utils/isEmail\";",
    "+import { validateLogin } from \"../schemas/login\";",
    " ",
    " export function LoginForm({ onSubmit }: Props) {",
    "   const [errors, setErrors] = useState<Errors>({});",
    "   const submit = (values: FormValues) => {",
    "-    const next: Errors = {};",
    "-    if (!values.email.trim()) next.email = \"required\";",
    "-    else if (!isEmail(values.email)) next.email = \"invalid\";",
    "-    if (values.password.length < 8) next.password = \"too short\";",
    "-    if (Object.keys(next).length) return setErrors(next);",
    "-    onSubmit(values);",
    "+    const result = validateLogin(values);",
    "+    if (!result.ok) return setErrors(result.errors);",
    "+    onSubmit(result.data);",
    "   };",
  ].join("\n"),
  "src/components/LoginForm.test.tsx": [
    "--- a/src/components/LoginForm.test.tsx",
    "+++ b/src/components/LoginForm.test.tsx",
    "@@ -12,6 +12,13 @@ describe(\"LoginForm\", () => {",
    "   it(\"rejects a short password\", async () => {",
    "     await fill({ email: \"a@b.co\", password: \"123\" });",
    "-    expect(screen.getByText(\"too short\")).toBeVisible();",
    "+    expect(screen.getByRole(\"alert\")).toBeVisible();",
    "   });",
    "+",
    "+  it(\"trims the email before validating\", async () => {",
    "+    await fill({ email: \"  a@b.co \", password: \"12345678\" });",
    "+    expect(onSubmit).toHaveBeenCalledWith(",
    "+      expect.objectContaining({ email: \"a@b.co\" }),",
    "+    );",
    "+  });",
  ].join("\n"),
};

export function panelMock() {
  return (cmd: string, args: unknown): unknown => {
    switch (cmd) {
      case "git_diff_summary":
        return { gitAvailable: true, isRepo: true, branch: "refactor/login-schema", ahead: 0, behind: 0, files: FILES };
      case "git_file_diff": {
        const path = (args as { path?: string } | undefined)?.path ?? "";
        const file = FILES.find((f) => f.path === path);
        return { path, status: file?.status ?? "M", diff: DIFFS[path] ?? "", truncated: false };
      }
      case "git_pull_request":
        return null;
      case "list_shell_terminals":
      case "list_browser_panes":
      case "list_file_openers":
        return [];
      case "list_dir_entries": {
        const rel = (args as { rel?: string } | undefined)?.rel ?? "";
        const entry = (name: string, isDir: boolean) => ({ name, relPath: rel ? `${rel}/${name}` : name, isDir });
        if (rel === "") return [entry("src", true), entry("package.json", false), entry("README.md", false)];
        if (rel === "src") return [entry("components", true), entry("schemas", true)];
        return [];
      }
      default:
        return undefined;
    }
  };
}

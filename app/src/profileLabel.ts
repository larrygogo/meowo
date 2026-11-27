// 账号（profile）的显示名，桌面设置页、额度区、手机远程页共用一处。
// 照 Orca（getManagedAccountPrimaryLabel）：名字是可选的，「添加账号」直接登录不先问名字，
// 没起名的账号显示登录邮箱；默认账号显示「系统默认」。
import type { ProfileView } from "./api";
import type { Dict } from "./i18n/zh";

/** 账号的登录身份：邮箱 > 显示名 > 登录方式标签（codex 的 "API Key"、opencode 的一串 provider、kimi 的 userId）。 */
export function profileIdentity(p: Pick<ProfileView, "account">): string | null {
  return p.account?.email ?? p.account?.display_name ?? p.account?.login_label ?? null;
}

/** 卡片标题 / 「某某的用量」里的名字。 */
export function profileLabel(p: Pick<ProfileView, "id" | "name" | "account">, t: Dict): string {
  if (p.id === null) return t.account.systemDefault;
  return p.name.trim() || profileIdentity(p) || t.account.unnamedAccount;
}

/** 账号名和下面那行邮箱指的是不是同一个邮箱。大小写不计；Gmail 不认本地部分的点和 `+后缀`
 *  （`ar.b@gmail.com` 与 `arb@gmail.com` 是同一个收件箱），按 Gmail 的规则比。
 *  实拍：账号名起成 `ar.xxx@gmail.com`、登录邮箱是 `arxxx@gmail.com`，徽章重复一遍还被挤成「账…」。 */
export function sameAccountName(name: string, email: string): boolean {
  const norm = (raw: string) => {
    const s = raw.trim().toLowerCase();
    const at = s.lastIndexOf("@");
    if (at <= 0) return s;
    const domain = s.slice(at + 1);
    if (domain !== "gmail.com" && domain !== "googlemail.com") return s;
    return s.slice(0, at).split("+")[0].replace(/\./g, "") + "@gmail.com";
  };
  return norm(name) === norm(email);
}

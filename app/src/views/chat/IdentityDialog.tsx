// 提交 / 拉取合并报 git/identity 时的就地出路：填名字和邮箱，写进全局 git 配置（walkthrough P-1，2026-11-12）。
// 以前的提示是「先在终端里运行 git config 设置」，违反「别让用户去终端收拾」（手机上更是死路）。
// 保存成功后由调用方重做刚才失败的那一步（提交、拉取并推送），用户不用再点一次。
import { useState } from "react";
import { gitSetIdentity } from "../../api";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { AppModal } from "../AppModal";

export function IdentityDialog({ cwd, sessionCwd, onSaved, onClose }: {
  cwd: string;
  sessionCwd: string | null;
  onSaved: () => void;
  onClose: () => void;
}) {
  const t = useT();
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ready = !!name.trim() && /^[^\s@]+@[^\s@]+$/.test(email.trim());
  const save = async () => {
    // 保存中不用 disabled：按钮正拿着焦点，disabled 会把焦点踢到 body（第三轮设计准则）。
    if (saving || !ready) return;
    setSaving(true);
    setError(null);
    try {
      await gitSetIdentity(cwd, name, email, sessionCwd);
      onSaved();
    } catch (e) {
      setError(formatBackendError(e, t.locale));
      setSaving(false);
    }
  };
  return (
    // 保存中不许关：关掉了也看不到成没成。
    <AppModal label={t.forge.identityTitle} onClose={() => { if (!saving) onClose(); }}>
      <div className="chat-modal-title">{t.forge.identityTitle}</div>
      <label className="ns-field">
        <span className="ns-label">{t.forge.identityName}</span>
        <input className="ns-input" value={name} autoFocus autoComplete="name" onChange={(e) => setName(e.target.value)} />
      </label>
      <label className="ns-field">
        <span className="ns-label">{t.forge.identityEmail}</span>
        <input className="ns-input" type="email" value={email} autoComplete="email" onChange={(e) => setEmail(e.target.value)}
          onKeyDown={(e) => { if (e.key === "Enter" && !e.nativeEvent.isComposing) { e.preventDefault(); void save(); } }} />
      </label>
      <div className="scm-discard-repo">{t.forge.identityNote}</div>
      {error && <div className="scm-error" role="alert">{error}</div>}
      <div className="chat-modal-actions">
        <button type="button" className="ns-btn" aria-disabled={saving || undefined} onClick={() => { if (!saving) onClose(); }}>{t.newSession.cancel}</button>
        <button type="button" className="ns-btn btn-primary" data-testid="identity-save" aria-disabled={saving || !ready || undefined} onClick={() => void save()}>
          {saving ? t.forge.identitySaving : t.forge.identitySave}
        </button>
      </div>
    </AppModal>
  );
}

// 设置页「网络」分区顶部：手机远程访问（局域网/Tailscale + token）开关与扫码配对。
//
// token 是进门凭据，绝不上网线：二维码把它放 URL fragment（`#token=`，不进服务端日志
// 也不随请求发出），手机端首次打开即存 localStorage 并清掉 hash。开关/端口经 set_settings
// 落盘后由后端 remote::apply 热生效；token 由 remote_access_info 惰性生成（首次开启才产生）。
import { useCallback, useEffect, useState } from "react";
import { renderSVG } from "uqr";
import { regenerateRemoteToken, remoteAccessInfo, testBarkPush, type RemoteAccessInfo, type RemoteBindMode } from "../../api";
import { appConfirm } from "../../confirm";
import { useT } from "../../i18n";
import { formatBackendError } from "../../i18n/errors";
import { useSettingsState } from "./state";
import { Switch, SettingsError } from "./widgets";
import { Dropdown } from "../menu";

/// 端口输入：本地草稿 + 失焦/回车提交（同 NetworkSection 的 UrlInput，避免每键一存打后端）。
function PortInput({ value, onCommit, conflict }: {
  value: number;
  onCommit: (port: number) => void;
  /** 启动时这个端口被占用的说明：与「端口不合法」同一位置、同一红框（报错贴着出问题的那一项）。 */
  conflict?: string | null;
}) {
  const t = useT();
  const [text, setText] = useState(String(value));
  // 7S-6：非法输入此前**静默**回退成磁盘值（默认 18620），用户看着自己敲的 70000
  // 变回 18620、没有一个字解释。改成留住输入 + 行内说明，直到改对或清空。
  const [invalid, setInvalid] = useState(false);
  useEffect(() => { setText(String(value)); setInvalid(false); }, [value]);
  const commit = () => {
    const raw = text.trim();
    if (raw === "") { setText(String(value)); setInvalid(false); return; }
    const n = Number.parseInt(raw, 10);
    if (!Number.isInteger(n) || n < 1 || n > 65535 || String(n) !== raw) { setInvalid(true); return; }
    setInvalid(false);
    if (n !== value) onCommit(n);
  };
  return (
    <>
      <input
        className={"ns-input remote-port-input" + (invalid || conflict ? " is-invalid" : "")}
        type="text"
        inputMode="numeric"
        autoComplete="off"
        spellCheck={false}
        aria-invalid={invalid || !!conflict || undefined}
        value={text}
        onChange={(e) => { setText(e.target.value); setInvalid(false); }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
        }}
      />
      {invalid
        ? <div className="proxy-err remote-port-err" role="alert">{t.remote.portInvalid}</div>
        : conflict && <div className="proxy-err remote-port-err" role="alert">{conflict}</div>}
    </>
  );
}

/// 手机推送（Bark）一行：地址草稿失焦/回车才落盘（同 PortInput），旁边「发一条测试」直接用
/// 草稿测——填完先测通再走开，不用等保存。远程访问关着也能用：推送只要电脑能连上 Bark。
/// 填了地址后右侧多一个开关：手机设置面板能暂停推送，电脑这边得看得见、拨得回来。
function BarkPushRow({ value, paused, onCommit, onTogglePaused }: {
  value: string;
  paused: boolean;
  onCommit: (url: string) => void;
  onTogglePaused: () => void;
}) {
  const t = useT();
  const [text, setText] = useState(value);
  const [status, setStatus] = useState<{ kind: "testing" | "ok" } | { kind: "error"; message: string } | null>(null);
  useEffect(() => { setText(value); }, [value]);
  const commit = () => {
    const next = text.trim();
    if (next !== value) onCommit(next);
  };
  const test = () => {
    if (!text.trim() || status?.kind === "testing") return;
    setStatus({ kind: "testing" });
    testBarkPush(text.trim())
      .then(() => setStatus({ kind: "ok" }))
      .catch((error) => setStatus({ kind: "error", message: formatBackendError(error, t.locale) }));
  };
  return (
    <div className="row remote-push-row">
      <div className="row-text">
        <div className="row-label">{t.remote.pushLabel}</div>
        <div className="row-desc">{t.remote.pushDesc}</div>
        <div className="remote-url-row">
          <input
            className="ns-input remote-push-input"
            type="url"
            autoComplete="off"
            spellCheck={false}
            placeholder={t.remote.pushPlaceholder}
            aria-label={t.remote.pushLabel}
            value={text}
            onChange={(e) => { setText(e.target.value); setStatus(null); }}
            onBlur={commit}
            onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); }}
          />
          <button type="button" className="remote-copy" disabled={!text.trim() || status?.kind === "testing"} onClick={test}>
            {status?.kind === "testing" ? t.remote.pushTesting : t.remote.pushTest}
          </button>
        </div>
        {status?.kind === "ok" && <div className="row-desc" role="status">{t.remote.pushTestOk}</div>}
        {status?.kind === "error" && <div className="proxy-err" role="alert">{status.message}</div>}
        {/* 内容出本机的事实写在旁边：项目名、会话标题、在等什么都会经过 Bark 服务器。 */}
        {text.trim() && <div className="row-desc">{t.remote.pushPrivacy}</div>}
      </div>
      {value.trim() && <Switch checked={!paused} onChange={onTogglePaused} label={t.remote.pushToggle} />}
    </div>
  );
}

export function RemoteAccessCard() {
  const t = useT();
  // patchError 与兄弟分区同款：开关/端口落盘被拒时不能静默回滚（开关自己弹回去零解释）。
  const [settings, patch, patchError, clearPatchError] = useSettingsState();
  const [info, setInfo] = useState<RemoteAccessInfo | null>(null);
  const [selectedIp, setSelectedIp] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const refresh = useCallback(() => {
    remoteAccessInfo()
      .then((i) => {
        setInfo(i);
        // 保留用户已选地址；失效或首取时落到首位候选（后端 Tailscale 优先排序）。
        setSelectedIp((cur) => (cur && i.ips.some((c) => c.ip === cur) ? cur : (i.ips[0]?.ip ?? null)));
      })
      .catch(() => {});
  }, []);
  useEffect(refresh, [refresh]);

  const enabled = settings?.remote_access_enabled ?? false;
  const port = settings?.remote_access_port ?? 18620;
  // 持久化值可能是未知字符串(手改 settings.json / 旧数据损坏),直接传给 Dropdown 会
  // 显示空标签。未知值回退 "all",与后端 unknown→all 的兼容语义一致。
  const rawBind = settings?.remote_access_bind;
  const bind: RemoteBindMode = rawBind === "loopback" || rawBind === "tailscale" ? rawBind : "all";

  // 开关/端口落盘后重取信息：remote::apply 是 fire-and-forget,set_settings resolve 时
  // server 往往还没 bind 完——立刻 refresh 会读到 apply 前的 last_error(端口被占也显
  // 「一切正常」的假二维码)。再延迟补一拍,拿到 bind 结果后的真 last_error。
  const refreshSettled = () => {
    refresh();
    window.setTimeout(refresh, 600);
  };
  const toggle = () => {
    void patch({ remote_access_enabled: !enabled }).then(refreshSettled);
  };
  const changePort = (next: number) => {
    void patch({ remote_access_port: next }).then(refreshSettled);
  };
  // 换绑定模式同样走 apply 的差异比对热重启;tailscale 模式找不到接口会被后端拒启,
  // last_error 红字如实显示(不静默回退 0.0.0.0)。
  const changeBind = (next: RemoteBindMode) => {
    void patch({ remote_access_bind: next }).then(refreshSettled);
  };
  const bindOptions = [
    { value: "all" as const, label: t.remote.bindAll },
    { value: "loopback" as const, label: t.remote.bindLoopback },
    { value: "tailscale" as const, label: t.remote.bindTailscale },
  ];

  const url =
    enabled && selectedIp && info?.token
      ? // 二维码按本实例**实际监听**端口生成（boundPort）：多实例共享 settings.json 时
        // info.port 是被另一实例改过的配置值，照它生成会指向别的实例的 server（实拍：
        // dev 改 18622 后，安装版 QR 指向 18622 而它自己还听在 18621）。
        `http://${selectedIp}:${info.boundPort ?? info.port}/#token=${info.token}`
      : null;
  const svg = url ? renderSVG(url, { border: 2 }) : null;

  const copy = () => {
    if (!url) return;
    void navigator.clipboard
      ?.writeText(url)
      .then(() => {
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => {});
  };

  // 桌面端猜不出手机在哪个网,只能把每个候选「是什么、什么情况下能通」标清楚让用户选。
  const kindLabel = (kind: "tailscale" | "lan") =>
    kind === "tailscale" ? t.remote.netTailscale : t.remote.netLan;
  const candidates = info?.ips ?? [];
  const ipOptions = candidates.map((c) => ({ value: c.ip, label: `${kindLabel(c.kind)} · ${c.ip}` }));
  const selectedKind = candidates.find((c) => c.ip === selectedIp)?.kind ?? null;
  // 启动失败是端口被占：贴在端口行说（换个端口就是解法），不另起一块报错。
  const portConflict = info?.lastError === "remote/port_in_use" ? formatBackendError(info.lastError, t.locale) : null;

  return (
    <>
    <div className="row-card remote-access-card">
      <div className="row">
        <div className="row-text">
          <div className="row-label">{t.remote.enable}</div>
          <div className="row-desc">{t.remote.enableDesc}</div>
        </div>
        <Switch checked={enabled} onChange={toggle} label={t.remote.enable} />
      </div>

      <div className="row remote-port-row">
        <div className="row-text">
          <div className="row-label">{t.remote.port}</div>
          <div className="row-desc">{t.remote.portDesc}</div>
        </div>
        <PortInput value={port} onCommit={changePort} conflict={portConflict} />
      </div>

      <div className="row">
        <div className="row-text">
          <div className="row-label">{t.remote.bind}</div>
          {/* 选 all 时把明文风险顶到说明位——这是唯一把令牌暴露给整个同网段的模式。 */}
          <div className="row-desc">{bind === "all" ? t.remote.bindWarnAll : t.remote.bindDesc}</div>
        </div>
        <Dropdown value={bind} options={bindOptions} onChange={changeBind} />
      </div>


      {/* 配置与实际监听分叉（多实例共享 settings.json，另一实例改端口写盘而本实例
          listener 未跟随）：不是错误——本实例服务本身健康，但配置值已不可信，说明白。 */}
      {enabled && info?.boundPort != null && info.boundPort !== port && (
        <div className="row">
          <div className="row-text">
            <div className="row-desc">{t.remote.boundMismatch(port, info.boundPort)}</div>
          </div>
        </div>
      )}

      {enabled &&
        (url && svg ? (
          // 配对行与其余设置行同构:文字列居左,二维码当作行右侧的「控件」。
          <div className="row remote-pair-row">
            <div className="row-text">
              <div className="row-label">{t.remote.scan}</div>
              <div className="row-desc">{t.remote.scanHint}</div>
              {/* 不配文字标签:选项本身是「Tailscale · IP」已自解释,窄列里标签只会被挤成竖排。 */}
              {ipOptions.length > 1 && (
                <div className="remote-ip-pick">
                  <Dropdown value={selectedIp ?? ""} options={ipOptions} onChange={setSelectedIp} />
                </div>
              )}
              {/* 地址不明文上屏(那串 token 又长又吵还含凭据),只给复制按钮;二维码即是地址本体。 */}
              <div className="remote-url-row">
                <button type="button" className="remote-copy" onClick={copy}>
                  {copied ? t.remote.copied : t.remote.copy}
                </button>
                {/* 换发即吊销:apply 比对 token 差异重启 server,已配对手机全部 401 回配对页。
                    但无可逆破坏——重新扫码即可配对,不销毁任何数据,故不用 danger 红(S-14)。 */}
                <button
                  type="button"
                  className="remote-copy"
                  onClick={() => {
                    void appConfirm(t.remote.regenerateConfirm, { title: t.remote.regenerate, confirmLabel: t.remote.regenerate }).then((yes) => {
                      if (!yes) return;
                      void regenerateRemoteToken().then(refreshSettled).catch(() => {});
                    });
                  }}
                >
                  {t.remote.regenerate}
                </button>
              </div>
              {selectedKind && (
                <div className="row-desc">
                  {selectedKind === "tailscale" ? t.remote.hintTailscale : t.remote.hintLan}
                </div>
              )}
            </div>
            {/* renderSVG 产出完整 <svg> 字符串，本地生成、不含用户可注入内容。 */}
            <div className="remote-qr" dangerouslySetInnerHTML={{ __html: svg }} />
          </div>
        ) : (
          <div className="row">
            <div className="row-text">
              <div className="row-label">{t.remote.scan}</div>
              <div className="row-desc">{t.remote.noIp}</div>
            </div>
          </div>
        ))}
      <BarkPushRow
        value={settings?.bark_url ?? ""}
        paused={settings?.bark_paused ?? false}
        onCommit={(url) => { void patch({ bark_url: url }); }}
        onTogglePaused={() => { void patch({ bark_paused: !(settings?.bark_paused ?? false) }); }}
      />
    </div>
    {/* 报错放在卡片外、紧跟其后（此前夹在卡片两行之间，用户实拍「位置不对」）。端口被占贴在端口行
        （portConflict），这里只剩保存失败与其余启动失败。7S-7：错误统一走 SettingsError（带 ×、role=alert）。 */}
    <SettingsError error={patchError} onDismiss={clearPatchError} />
    {/* 自己 format 一次再套语境句，并告诉 SettingsError 别再 format——两边都做的话，
        「启动失败：绑定端口 18620 失败：…」这种双前缀就出来了（复核指出）。 */}
    {info?.lastError && !portConflict && (
      <SettingsError
        preformatted
        error={t.remote.startError(formatBackendError(info.lastError, t.locale))}
        onDismiss={() => setInfo((cur) => (cur ? { ...cur, lastError: null } : cur))}
      />
    )}
    </>
  );
}

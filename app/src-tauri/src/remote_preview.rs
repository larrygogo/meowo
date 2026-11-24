//! 手机预览：手机上点对话里的本机链接（AI 起的 dev server），经电脑上的代理打开。
//!
//! 场景：AI 起了 `npm run dev`，回复「打开 http://localhost:5173 看看」。手机上点这个链接打开的是
//! 手机自己的 localhost。这里在电脑上为该端口开一个代理端口，手机经它访问电脑上的 dev server。
//! leader 2026-11-24 关卡 B 选「远程访问开着就自动可用」；调研见 .lead/20261124-phone-preview。
//!
//! # 为什么每个预览一个端口
//! 按子路径转发（`/preview/5173/...`）要改写页面里所有绝对路径，热更新客户端还会按页面地址去连
//! websocket，社区里一再反馈这样会断。独立端口下页面看到的路径与在电脑上一模一样，只需改请求头。
//!
//! # 安全收口（审计从这里开始）
//! - 目标只认本机回环（127.0.0.1 / ::1）上的端口，映射在服务端，请求里不带目标；拒绝 CONNECT
//!   与绝对 URI 形式的请求，代理不能被当成跳板。
//! - 只代理「正在监听、而且监听进程在这个会话进程树下」的端口（[`listener_pids`] + 进程树）；被 `&`、
//!   nohup 甩出进程树的不认（认目录会把同仓库里别人的服务也开出去），
//!   电脑上别的服务（数据库管理页、别的项目）不会因为对话里出现了它的地址就被开出去。
//! - 配对令牌不进预览地址：远程页经 /rpc（带令牌）领一个 60 秒、一次性的码，预览端口凭码种一个
//!   按端口区分的 HttpOnly cookie，再 302 到干净地址。cookie 不区分端口，所以名字带端口、值是
//!   该代理独有的随机串；转发给 dev server 前删掉。
//! - 入站 Host 必须是 IP 字面量（防 DNS rebinding：攻击者的域名解析到本机时 Host 是域名）。
//! - 归属不是只在打开那一刻查：之后每 15 秒内第一个请求、连接失败后、换连另一个地址族时都重查，
//!   dev server 停了、端口被别的程序接手，预览立即不再转发（独立审查 2026-11-24 #3）。
//! - 只在远程桥所在的网卡上监听，入站 Host 必须是代理监听的那个地址；远程访问关闭 / 令牌重发
//!   （都会经 remote::apply 重启桥）、设置里关掉预览、端口不再属于会话、30 分钟没有热更新连接也
//!   没有请求时回收。回收连已升级的 websocket 一起断；打开与关闭交错时靠代次号作废在途的那次打开。

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::body::Incoming;
use hyper::header::{self, HeaderMap, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

type Body = BoxBody<Bytes, hyper::Error>;
/// 一个预览：(代理监听的地址, 目标端口)。手机经局域网和经 Tailscale 来时监听地址不同，各开各的。
type Key = (IpAddr, u16);

/// 一次性码的有效期：手机点链接到新页发出第一个请求，正常一两秒。
const CODE_TTL: Duration = Duration::from_secs(60);
/// 没有任何连接持续这么久就回收（热更新的 websocket 算连接，开着页面不会被回收）。
const IDLE_REAP: Duration = Duration::from_secs(30 * 60);
const REAP_EVERY: Duration = Duration::from_secs(60);
/// 换码的路径。带 __meowo 前缀，和 dev server 自己的路由撞不上。
const AUTH_PATH: &str = "/__meowo/preview";

// ═══ 状态 ═══

struct Proxy {
    port: u16,
    target: SocketAddr,
    /// 谁的端口：之后重查归属用。
    owner: Owner,
    shutdown: tokio::sync::watch::Sender<bool>,
    /// 当前开着的 websocket 隧道数（热更新连接，页面开着就一直在）；为 0 且闲置超时才回收。
    /// 普通 HTTP 连接不算：浏览器会把空闲的长连接挂好几分钟，算进来就永远回收不了。
    active: Arc<AtomicUsize>,
    /// 最近一次有请求的时刻（自 hub 创建起的秒数）。
    last_seen: Arc<AtomicU64>,
    /// 最近一次确认归属的时刻；[`NEEDS_CHECK`] = 下一个请求先重查。
    verified_at: Arc<AtomicU64>,
    /// 回收循环发现端口没人监听（或查不了监听表）的起始时刻：dev server 重启要几秒到几分钟，
    /// 这期间代理留着、请求给「预览没在运行」页，连续 [`MISSING_GRACE`] 都不在才回收。
    missing_since: Option<u64>,
}

/// 端口持续没人监听多久才回收代理（AI 重启 dev server、装依赖期间手机刷新看到的是说明页，不是浏览器的连接错误）。
const MISSING_GRACE: u64 = 5 * 60;

const NEEDS_CHECK: u64 = u64::MAX;
/// 归属确认后多久内不再重查（一次页面加载有上百个模块请求，不能每个都扫进程表）。
const VERIFY_EVERY: u64 = 15;

/// 端口归属：会话的根进程（托管 PTY 子进程、reporter 记下的 agent pid）。监听进程得在它们的进程树里。
#[derive(Clone, Debug)]
pub(crate) enum Owner {
    Session { roots: Vec<u32> },
    /// 测试与手动验证用：不查归属。
    #[cfg(test)]
    Anyone,
}

struct PendingCode {
    key: Key,
    path: String,
    expires: Instant,
}

#[derive(Default)]
struct HubInner {
    /// 远程桥当前监听的地址；None = 远程访问没开（或刚被关掉），不开新预览。
    bind: Option<IpAddr>,
    proxies: HashMap<Key, Proxy>,
    /// 上次用过的代理端口：目标端口本身被占（dev server 自己开在所有网卡上）时退到随机端口，
    /// 回收后再开先试它，手机上的地址不变，页面的 localStorage（按 来源=IP+端口 分）也还在。
    last_ports: HashMap<Key, u16>,
    codes: HashMap<String, PendingCode>,
    reaper: bool,
    /// 代次：桥起停、设置里关掉预览时加一。打开预览中途要等进程表，等回来时代次变了就作废，
    /// 不在桥已经停掉之后再开出一个代理、发出一个码（独立审查 #4）。
    generation: u64,
}

struct Hub {
    inner: Mutex<HubInner>,
    epoch: Instant,
}

fn hub() -> &'static Hub {
    static HUB: OnceLock<Hub> = OnceLock::new();
    HUB.get_or_init(|| Hub { inner: Mutex::new(HubInner::default()), epoch: Instant::now() })
}

fn lock() -> std::sync::MutexGuard<'static, HubInner> {
    hub().inner.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_secs() -> u64 {
    hub().epoch.elapsed().as_secs()
}

/// 远程桥起来了：记下它监听的地址，之后的预览端口开在同一张网卡上。
pub(crate) fn bridge_started(bind: IpAddr) {
    let stale = {
        let mut inner = lock();
        let stale = if inner.bind != Some(bind) { std::mem::take(&mut inner.proxies) } else { HashMap::new() };
        inner.bind = Some(bind);
        inner.generation += 1;
        stale
    };
    for (_, proxy) in stale {
        let _ = proxy.shutdown.send(true);
    }
}

/// 远程桥停了（关闭、改端口、重新生成令牌都会先停）：所有预览一起关，没换的码作废。
pub(crate) fn bridge_stopped() {
    let proxies = {
        let mut inner = lock();
        inner.bind = None;
        inner.codes.clear();
        inner.generation += 1;
        std::mem::take(&mut inner.proxies)
    };
    for (_, proxy) in proxies {
        let _ = proxy.shutdown.send(true);
    }
}

/// 设置里关掉了手机预览：已开的立即回收。
pub(crate) fn close_all() {
    let proxies = {
        let mut inner = lock();
        inner.codes.clear();
        inner.generation += 1;
        std::mem::take(&mut inner.proxies)
    };
    for (_, proxy) in proxies {
        let _ = proxy.shutdown.send(true);
    }
}

// ═══ 开预览（/rpc preview_open） ═══

#[derive(serde::Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewTicket {
    /// 手机该连的主机（代理监听的那个本机地址，IPv6 带方括号）。不用手机地址栏里的主机名：
    /// 经 MagicDNS / .local 名字连进来时，代理只认 IP（防 DNS rebinding）。
    pub host: String,
    /// 代理端口。
    pub port: u16,
    /// 换码地址（含一次性码）；换完跳到原链接的路径。
    pub path: String,
}

/// 原链接里的端口与路径。只认 http 与本机回环主机名；其余一律拒（错误码由前端 errors 表转成人话）。
pub(crate) fn parse_local_url(raw: &str) -> Result<(u16, String), String> {
    let url = url::Url::parse(raw.trim()).map_err(|_| "preview/bad_url".to_string())?;
    match url.scheme() {
        "http" => {}
        "https" => return Err("preview/https".into()),
        _ => return Err("preview/bad_url".into()),
    }
    let local = match url.host() {
        Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback() || ip.is_unspecified(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    if !local {
        return Err("preview/not_local".into());
    }
    let port = url.port().unwrap_or(80);
    let mut path = url.path().to_string();
    if let Some(q) = url.query() {
        path.push('?');
        path.push_str(q);
    }
    if let Some(f) = url.fragment() {
        path.push('#');
        path.push_str(f);
    }
    Ok((port, path))
}

/// 手机点了一个本机链接。`owner` 是该会话的进程与目录。
pub(crate) async fn open(url: String, owner: Owner, enabled: bool, phone_host: Option<String>) -> Result<PreviewTicket, String> {
    if !enabled {
        return Err("preview/disabled".into());
    }
    let (target_port, path) = parse_local_url(&url)?;
    let (bind, generation) = {
        let inner = lock();
        (inner.bind.ok_or_else(|| "preview/remote_off".to_string())?, inner.generation)
    };
    if bind.is_loopback() {
        // 远程桥只听本机：手机是经隧道（adb reverse、ssh -L）连进来的，新开的端口隧道里没有。
        return Err("preview/loopback_bind".into());
    }
    if remote_owned_port(target_port) {
        return Err("preview/not_session".into());
    }
    let listen = listen_ip(bind, phone_host.as_deref());
    if listen.is_unspecified() {
        // 桥开在所有网卡上，手机却是经主机名（MagicDNS、.local）连进来的：不知道该把代理开在哪张网卡、
        // 给手机哪个 IP。让用户改用二维码里的 IP 地址连。
        return Err("preview/hostname".into());
    }

    // 查谁在监听、是不是这个会话的进程：进程表与连接表都是阻塞系统调用。
    let check = owner.clone();
    let (target, same_port_ok) = tauri::async_runtime::spawn_blocking(move || resolve_target(target_port, &check))
        .await
        .map_err(|e| e.to_string())??;

    let key = (listen, target_port);
    let port = ensure_proxy(key, target, owner, same_port_ok, generation).await?;
    let code = crate::remote::generate_token()?;
    {
        let mut inner = lock();
        if inner.generation != generation {
            return Err("preview/remote_off".into());
        }
        let now = Instant::now();
        inner.codes.retain(|_, c| c.expires > now);
        inner.codes.insert(code.clone(), PendingCode { key, path, expires: now + CODE_TTL });
    }
    let host = match listen {
        IpAddr::V6(v6) => format!("[{v6}]"),
        IpAddr::V4(v4) => v4.to_string(),
    };
    Ok(PreviewTicket { host, port, path: format!("{AUTH_PATH}?c={code}") })
}

/// 代理开在哪个地址上：远程桥绑在具体网卡（Tailscale）就用它；绑在所有网卡时用手机这次访问远程页
/// 用的那个本机地址（远程页请求的 Host），这样代理端口能和 dev server 同号（见 [`ensure_proxy`]）。
/// Host 不是 IP、是回环或不是本机地址时退回桥的地址（随后同号端口绑不上就用随机端口，不出错）。
fn listen_ip(bind: IpAddr, phone_host: Option<&str>) -> IpAddr {
    if !bind.is_unspecified() {
        return bind;
    }
    phone_host
        .and_then(host_ip)
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified() && ip.is_ipv4() == bind.is_ipv4())
        .unwrap_or(bind)
}

/// Host 头（可带端口、IPv6 带方括号）里的 IP；域名返回 None。
fn host_ip(host: &str) -> Option<IpAddr> {
    let h = host.trim();
    if let Some(rest) = h.strip_prefix('[') {
        return rest.split_once(']').and_then(|(ip, _)| ip.parse::<Ipv6Addr>().ok()).map(IpAddr::V6);
    }
    h.rsplit_once(':').map(|(n, _)| n).unwrap_or(h).parse::<Ipv4Addr>().ok().map(IpAddr::V4)
}

/// 远程桥自己的端口不当预览目标（它不是 dev server，开出去只会多一个入口）。
fn remote_owned_port(port: u16) -> bool {
    crate::remote::running_port() == Some(port) || lock().proxies.values().any(|p| p.port == port)
}

/// 找到监听 `port` 的、属于会话的那个回环地址；第二项 = 能不能占同号端口（见 [`ensure_proxy`]）。
fn resolve_target(port: u16, owner: &Owner) -> Result<(SocketAddr, bool), String> {
    resolve_target_excluding(port, owner, None)
}

/// 同上；`own` 是代理自己占着同号端口时的监听地址，判断「dev server 是不是只听回环」时不算它。
fn resolve_target_excluding(port: u16, owner: &Owner, own: Option<IpAddr>) -> Result<(SocketAddr, bool), String> {
    let listeners = listener_pids(port).ok_or_else(|| "preview/owner_unknown".to_string())?;
    let same_port_ok = same_port_free(&listeners, own);
    // 只认回环能连到的监听：绑在 127.0.0.1 / 0.0.0.0 → 连 127.0.0.1；绑在 ::1 / :: → 连 ::1。
    let reachable: Vec<(SocketAddr, u32)> = listeners
        .into_iter()
        .filter_map(|(ip, pid)| {
            let addr = match ip {
                IpAddr::V4(v4) if v4.is_loopback() || v4.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
                IpAddr::V6(v6) if v6.is_loopback() || v6.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
                _ => return None,
            };
            Some((SocketAddr::new(addr, port), pid))
        })
        .collect();
    if reachable.is_empty() {
        return Err("preview/not_listening".into());
    }
    let roots = match owner {
        Owner::Session { roots } => roots,
        #[cfg(test)]
        Owner::Anyone => {
            let all: Vec<SocketAddr> = reachable.iter().map(|(a, _)| *a).collect();
            return Ok((*all.iter().min_by_key(|a| !a.is_ipv4()).unwrap(), same_port_ok));
        }
    };
    let parent_of = crate::proc::parent_map();
    let mut tree = std::collections::HashSet::new();
    for &root in roots.iter().filter(|r| **r > 0) {
        tree.insert(root);
        tree.extend(crate::proc::descendant_pids(&parent_of, root));
    }
    if let Some(addr) = pick_owned(&reachable, &tree) {
        return Ok((addr, same_port_ok));
    }
    // 进程树断开（`npm run dev &`、nohup、Start-Process 把服务甩出去，中间那层 shell 退出）的不认：
    // 认「工作目录在会话目录里」会把同一仓库里别人起的服务也开出去（独立审查第二轮 A，实测），超出 leader
    // 关卡 B 签的「只开放本会话自己起的端口」。改由 MCP 说明让 AI 用自己的后台运行功能起 dev server，
    // 被拒时提示写清楚原因和改法。
    Err("preview/not_session".into())
}

/// dev server 自己开在所有网卡或某个局域网地址上：同网设备本来就能直接连它。代理再占同号端口，
/// Windows 上会把这个端口的局域网入口抢过来（后绑的具体地址优先，独立审查实测），别的设备就被挡在
/// 外面。这时不占同号端口。代理自己已经占着同号端口时，它那条监听（局域网地址）不算——算进去的话
/// 同号代理每次复查都会被当成「dev server 对局域网开放了」而回收（独立审查第三轮，实测 60 秒内被关）。
fn same_port_free(listeners: &[(IpAddr, u32)], own: Option<IpAddr>) -> bool {
    listeners.iter().filter(|(ip, _)| Some(*ip) != own).all(|(ip, _)| ip.is_loopback())
}

/// 在属于会话进程树的监听里挑一个，IPv4 优先（Node 18+ 的「localhost」可能只开在 ::1，所以两族都收）。
fn pick_owned(reachable: &[(SocketAddr, u32)], tree: &std::collections::HashSet<u32>) -> Option<SocketAddr> {
    let mut owned: Vec<SocketAddr> = reachable.iter().filter(|(_, pid)| tree.contains(pid)).map(|(a, _)| *a).collect();
    owned.sort_by_key(|a| !a.is_ipv4());
    owned.first().copied()
}

/// 本机所有监听 `port` 的套接字：(本地地址, 属主 pid)。None = 本平台查不了监听表（按查不到属主处理，不放行）。
#[cfg(windows)]
fn listener_pids(port: u16) -> Option<Vec<(IpAddr, u32)>> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6TABLE_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
    };
    const AF_INET: u32 = 2;
    const AF_INET6: u32 = 23;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

    fn table(family: u32) -> Option<Vec<u32>> {
        let mut size: u32 = 0;
        // SAFETY: 传空缓冲区只为取所需大小。
        unsafe { GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0) };
        for _ in 0..4 {
            // u32 元素保证表结构需要的 4 字节对齐；两次调用之间监听数可能变多，不够就按新大小重来。
            let mut buf = vec![0u32; (size as usize).div_ceil(4) + 1];
            let mut len = (buf.len() * 4) as u32;
            // SAFETY: buf 长度 len 字节，可写；API 只在其中写表。
            let rc = unsafe { GetExtendedTcpTable(buf.as_mut_ptr().cast(), &mut len, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0) };
            if rc == ERROR_INSUFFICIENT_BUFFER {
                size = len;
                continue;
            }
            return (rc == 0).then_some(buf);
        }
        None
    }

    let mut out = Vec::new();
    let v4 = table(AF_INET)?;
    // SAFETY: 成功时缓冲区开头是 MIB_TCPTABLE_OWNER_PID，后接 dwNumEntries 行（都在缓冲区之内）。
    let rows = unsafe {
        let t = &*(v4.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
        std::slice::from_raw_parts(t.table.as_ptr(), t.dwNumEntries as usize)
    };
    for row in rows {
        // 端口是网络字节序的低 16 位；地址按内存字节就是点分顺序。
        if u16::from_be(row.dwLocalPort as u16) == port {
            out.push((IpAddr::V4(Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes())), row.dwOwningPid));
        }
    }
    if let Some(v6) = table(AF_INET6) {
        // SAFETY: 同上，IPv6 表结构。
        let rows = unsafe {
            let t = &*(v6.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID);
            std::slice::from_raw_parts(t.table.as_ptr(), t.dwNumEntries as usize)
        };
        for row in rows {
            if u16::from_be(row.dwLocalPort as u16) == port {
                out.push((IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)), row.dwOwningPid));
            }
        }
    }
    Some(out)
}

/// macOS/Linux：借 lsof（macOS 自带）。`-F pn` 输出「p<pid>」行后跟它的「n<地址:端口>」行。
#[cfg(not(windows))]
fn listener_pids(port: u16) -> Option<Vec<(IpAddr, u32)>> {
    let output = std::process::Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fpn"])
        .output()
        .ok()?;
    // lsof 找不到匹配项时退出码是 1、输出为空：那是「没人监听」，不是「查不了」。
    Some(parse_lsof_listeners(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg_attr(windows, allow(dead_code))]
fn parse_lsof_listeners(text: &str) -> Vec<(IpAddr, u32)> {
    let mut out = Vec::new();
    let mut pid: Option<u32> = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.trim().parse().ok();
        } else if let (Some(n), Some(pid)) = (line.strip_prefix('n'), pid) {
            // 形如 127.0.0.1:5173、*:5173、[::1]:5173。
            let host = n.rsplit_once(':').map(|(h, _)| h).unwrap_or(n);
            let ip = match host {
                "*" => Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
                h => h.trim_start_matches('[').trim_end_matches(']').parse().ok(),
            };
            if let Some(ip) = ip {
                out.push((ip, pid));
            }
        }
    }
    out
}

// ═══ 代理端口 ═══

/// 代理端口优先与目标同号：dev server 只听 127.0.0.1，同号端口开在局域网地址上不冲突。手机上的地址
/// 于是和电脑上只差主机名，按「服务器端口」去连热更新的客户端也连得上（webpack-dev-server 的
/// websocket 地址带的是服务器端口，换成别的代理端口它就绕过代理直连、在手机上连不上，实测）。
/// 同号不能占（dev server 自己开在局域网上，见 [`resolve_target`]）或绑不上时，再试上次用过的，最后随机。
async fn ensure_proxy(key: Key, target: SocketAddr, owner: Owner, same_port_ok: bool, generation: u64) -> Result<u16, String> {
    let (listen, target_port) = key;
    let preferred = {
        let mut inner = lock();
        if let Some(p) = inner.proxies.get_mut(&key) {
            // 同一端口换了监听族（dev server 重启后从 127.0.0.1 换到 ::1）：改连新地址，端口不变。
            p.target = target;
            p.owner = owner;
            p.verified_at.store(now_secs(), Ordering::SeqCst);
            return Ok(p.port);
        }
        inner.last_ports.get(&key).copied().filter(|p| *p != target_port || same_port_ok)
    };
    let mut listener = None;
    for p in [same_port_ok.then_some(target_port), preferred, Some(0)].into_iter().flatten() {
        if let Ok(l) = tokio::net::TcpListener::bind((listen, p)).await {
            listener = Some(l);
            break;
        }
    }
    let listener = listener.ok_or_else(|| "preview/bind_failed".to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let secret: Arc<str> = crate::remote::generate_token()?.into();
    let (shutdown, rx) = tokio::sync::watch::channel(false);
    let active = Arc::new(AtomicUsize::new(0));
    let last_seen = Arc::new(AtomicU64::new(now_secs()));
    let verified_at = Arc::new(AtomicU64::new(now_secs()));
    let ctx = Arc::new(Ctx {
        port,
        key,
        secret: secret.clone(),
        active: active.clone(),
        last_seen: last_seen.clone(),
        verified_at: verified_at.clone(),
        shutdown: rx.clone(),
    });

    let start_reaper = {
        let mut inner = lock();
        // 桥在这期间停了 / 重启了 / 预览被关了：这次打开作废，刚绑的监听随 listener 一起丢掉。
        if inner.generation != generation || inner.bind.is_none() {
            return Err("preview/remote_off".into());
        }
        // 两个请求同时为同一端口开代理：先到的赢，后到的关掉自己刚绑的监听。
        if let Some(p) = inner.proxies.get(&key) {
            return Ok(p.port);
        }
        inner.proxies.insert(key, Proxy { port, target, owner, shutdown, active, last_seen, verified_at, missing_since: None });
        inner.last_ports.insert(key, port);
        !std::mem::replace(&mut inner.reaper, true)
    };
    tauri::async_runtime::spawn(serve(listener, ctx, rx));
    if start_reaper {
        tauri::async_runtime::spawn(reap_loop());
    }
    eprintln!("[preview] 预览代理已开：{listen}:{port} → 本机 {target_port}");
    Ok(port)
}

async fn reap_loop() {
    loop {
        tokio::time::sleep(REAP_EVERY).await;
        let now = now_secs();
        let idle_limit = IDLE_REAP.as_secs();
        // 除了闲置，还按端口现状回收：
        // - 端口被不属于会话的程序接手：立即回收；
        // - 代理占着同号端口，而 dev server 改成了对局域网开放（重启时加了 --host）：立即回收，下次打开换随机
        //   端口，不再挡着它的局域网入口（独立审查第二轮 D）；
        // - 没人监听 / 查不了：先记下起始时刻，连续 MISSING_GRACE 都这样才回收（第二轮 B）。
        // 每个代理带上身份（它独有的计数器地址）：判断要等进程表，等回来时同一个 key 可能已经被回收又重开，
        // 旧判断不能套到新代理上（独立审查第三轮）。
        let checks: Vec<(Key, usize, u16, Owner)> =
            lock().proxies.iter().map(|(k, p)| (*k, proxy_id(p), p.port, p.owner.clone())).collect();
        let verdicts: Vec<(Key, usize, Verdict)> = tauri::async_runtime::spawn_blocking(move || {
            checks
                .into_iter()
                .map(|(key, id, port, owner)| {
                    let own = (port == key.1).then_some(key.0);
                    let v = verdict(resolve_target_excluding(key.1, &owner, own), port == key.1);
                    (key, id, v)
                })
                .collect()
        })
        .await
        .unwrap_or_default();
        let reaped: Vec<Proxy> = {
            let mut inner = lock();
            for (key, id, v) in &verdicts {
                if let Some(p) = inner.proxies.get_mut(key).filter(|p| proxy_id(p) == *id) {
                    p.missing_since = match v {
                        Verdict::Missing => Some(p.missing_since.unwrap_or(now)),
                        _ => None,
                    };
                }
            }
            let stale: Vec<Key> = inner
                .proxies
                .iter()
                .filter(|(k, p)| {
                    let dropped = verdicts.iter().any(|(key, id, v)| key == *k && *id == proxy_id(p) && matches!(v, Verdict::Drop));
                    let missing_long = p.missing_since.is_some_and(|since| now.saturating_sub(since) >= MISSING_GRACE);
                    let idle = p.active.load(Ordering::SeqCst) == 0 && now.saturating_sub(p.last_seen.load(Ordering::SeqCst)) >= idle_limit;
                    dropped || missing_long || idle
                })
                .map(|(k, _)| *k)
                .collect();
            stale.into_iter().filter_map(|k| inner.proxies.remove(&k)).collect()
        };
        for p in reaped {
            eprintln!("[preview] 预览代理回收：{}", p.port);
            let _ = p.shutdown.send(true);
        }
    }
}

/// 回收循环对一个代理的判断。
#[derive(Debug, PartialEq)]
enum Verdict {
    Fine,
    Missing,
    Drop,
}

/// `same_port`：代理占着与目标同号的端口。
fn verdict(check: Result<(SocketAddr, bool), String>, same_port: bool) -> Verdict {
    match check {
        // 代理占着同号端口，而 dev server（不算代理自己）已经不只听回环：让出来。
        Ok((_, same_port_ok)) if same_port && !same_port_ok => Verdict::Drop,
        Ok(_) => Verdict::Fine,
        Err(code) if code == "preview/not_session" => Verdict::Drop,
        Err(_) => Verdict::Missing,
    }
}

fn proxy_id(p: &Proxy) -> usize {
    Arc::as_ptr(&p.verified_at) as usize
}

struct Ctx {
    port: u16,
    key: Key,
    secret: Arc<str>,
    active: Arc<AtomicUsize>,
    last_seen: Arc<AtomicU64>,
    verified_at: Arc<AtomicU64>,
    /// 回收信号：已升级的 websocket 隧道也要随它断开。
    shutdown: tokio::sync::watch::Receiver<bool>,
}

/// 长连接计数守卫（websocket 隧道、SSE 响应）：结束时减一。
struct ActiveGuard(Arc<AtomicUsize>);
impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn serve(listener: tokio::net::TcpListener, ctx: Arc<Ctx>, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    loop {
        let accepted = tokio::select! {
            r = listener.accept() => r,
            _ = shutdown.changed() => break,
        };
        let Ok((stream, _)) = accepted else { continue };
        let _ = stream.set_nodelay(true);
        // 手机断网、锁屏后连接可能半开着，永远不会收到 FIN：开 TCP 保活，几分钟内探测出来并断掉，
        // 否则热更新隧道一直算「开着」，闲置回收永远不触发（独立审查 #2）。
        let keepalive = socket2::TcpKeepalive::new().with_time(Duration::from_secs(60)).with_interval(Duration::from_secs(15));
        let _ = socket2::SockRef::from(&stream).set_tcp_keepalive(&keepalive);
        let ctx = ctx.clone();
        let mut conn_shutdown = shutdown.clone();
        tauri::async_runtime::spawn(async move {
            let svc_ctx = ctx.clone();
            let service = hyper::service::service_fn(move |req| {
                let ctx = svc_ctx.clone();
                async move { Ok::<_, hyper::Error>(handle(req, &ctx).await) }
            });
            let conn = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades();
            tokio::select! {
                _ = conn => {}
                _ = conn_shutdown.changed() => {}
            }
        });
    }
}

// ═══ 单个请求 ═══

async fn handle(mut req: Request<Incoming>, ctx: &Ctx) -> Response<Body> {
    ctx.last_seen.store(now_secs(), Ordering::SeqCst);
    if req.method() == Method::CONNECT || req.uri().scheme().is_some() || req.uri().authority().is_some() {
        return plain(StatusCode::BAD_REQUEST, "bad request");
    }
    let Some(inbound_host) = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).map(str::to_string) else {
        return plain(StatusCode::BAD_REQUEST, "missing host");
    };
    // 入站 Host 必须就是代理监听的那个地址（防 DNS rebinding：那时 Host 是攻击者的域名）。
    if host_ip(&inbound_host) != Some(ctx.key.0) || !host_is_ip_literal(&inbound_host) {
        return hostname_page();
    }

    if req.uri().path() == AUTH_PATH {
        return redeem(&req, ctx);
    }
    if !has_cookie(req.headers(), &cookie_name(ctx.port), &ctx.secret) {
        return need_link_page();
    }

    let target_port = ctx.key.1;
    let Some((mut target, owner)) = lock().proxies.get(&ctx.key).map(|p| (p.target, p.owner.clone())) else {
        return plain(StatusCode::SERVICE_UNAVAILABLE, "preview closed");
    };
    // 归属过期（15 秒）或上次连失败过：先重查。dev server 停了、端口被别的程序接手，就不再转发。
    let verified = ctx.verified_at.load(Ordering::SeqCst);
    if verified == NEEDS_CHECK || now_secs().saturating_sub(verified) >= VERIFY_EVERY {
        match reverify(ctx, target_port, owner.clone()).await {
            Ok(addr) => target = addr,
            Err(page) => return page,
        }
    }
    rewrite_request_headers(req.headers_mut(), &inbound_host, target_port, ctx.port);
    let upgrade_requested = req.headers().contains_key(header::UPGRADE);
    if upgrade_requested && req.headers().get(header::ORIGIN).is_some_and(|o| o == "null") {
        return plain(StatusCode::FORBIDDEN, "origin not allowed");
    }

    let stream = match tokio::time::timeout(Duration::from_secs(3), tokio::net::TcpStream::connect(target)).await {
        Ok(Ok(s)) => s,
        _ => {
            // 连不上：dev server 停了、重启了，或者换了地址族（127.0.0.1 ↔ ::1）。重查一次归属拿新地址，
            // 不直接去连另一个族——那里可能是别的程序（独立审查 #3）。
            ctx.verified_at.store(NEEDS_CHECK, Ordering::SeqCst);
            let addr = match reverify(ctx, target_port, owner).await {
                Ok(addr) if addr != target => addr,
                Ok(_) => return not_running_page(target_port),
                Err(page) => return page,
            };
            match tokio::time::timeout(Duration::from_secs(3), tokio::net::TcpStream::connect(addr)).await {
                Ok(Ok(s)) => s,
                _ => return not_running_page(target_port),
            }
        }
    };
    let _ = stream.set_nodelay(true);
    let Ok((mut sender, conn)) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await else {
        return not_running_page(target_port);
    };
    tauri::async_runtime::spawn(async move {
        let _ = conn.with_upgrades().await;
    });
    let client_upgrade = upgrade_requested.then(|| hyper::upgrade::on(&mut req));
    let mut resp = match sender.send_request(req).await {
        Ok(r) => r,
        Err(_) => return not_running_page(target_port),
    };
    if resp.status() == StatusCode::SWITCHING_PROTOCOLS {
        if let Some(client_upgrade) = client_upgrade {
            let server_upgrade = hyper::upgrade::on(&mut resp);
            let active = ctx.active.clone();
            active.fetch_add(1, Ordering::SeqCst);
            let guard = ActiveGuard(active);
            let mut shutdown = ctx.shutdown.clone();
            tauri::async_runtime::spawn(async move {
                let _guard = guard;
                if let (Ok(a), Ok(b)) = (client_upgrade.await, server_upgrade.await) {
                    let mut a = TokioIo::new(a);
                    let mut b = TokioIo::new(b);
                    // 回收时隧道一起断：关掉远程访问后，已经建好的热更新连接不能还通着 dev server。
                    tokio::select! {
                        _ = tokio::io::copy_bidirectional(&mut a, &mut b) => {}
                        _ = shutdown.wait_for(|stop| *stop) => {}
                    }
                }
            });
        }
    }
    rewrite_response_headers(resp.headers_mut(), target_port);
    // 用 SSE（text/event-stream）推热更新的页面开着时，这条响应一直在流，同样算「有人在看」，
    // 不能 30 分钟后被闲置回收（独立审查第二轮 C）。守卫跟着响应体走，流结束或手机断开时减一。
    let streaming = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim_start().to_ascii_lowercase().starts_with("text/event-stream"));
    if streaming {
        ctx.active.fetch_add(1, Ordering::SeqCst);
        let guard = ActiveGuard(ctx.active.clone());
        return resp.map(move |b| {
            b.map_frame(move |frame| {
                let _hold = &guard;
                frame
            })
            .boxed()
        });
    }
    resp.map(|b| b.boxed())
}

/// 重查归属：仍属于会话就更新目标地址并记下确认时刻；否则给一页人话（并让回收循环收掉它）。
async fn reverify(ctx: &Ctx, target_port: u16, owner: Owner) -> Result<SocketAddr, Response<Body>> {
    let result = tauri::async_runtime::spawn_blocking(move || resolve_target(target_port, &owner)).await;
    match result {
        Ok(Ok((addr, _))) => {
            if let Some(p) = lock().proxies.get_mut(&ctx.key) {
                p.target = addr;
            }
            ctx.verified_at.store(now_secs(), Ordering::SeqCst);
            Ok(addr)
        }
        Ok(Err(code)) if code == "preview/not_session" => Err(not_session_page(target_port)),
        _ => Err(not_running_page(target_port)),
    }
}

/// 凭一次性码种 cookie，再跳到原链接的路径。码用一次就删，过期的也删。
fn redeem(req: &Request<Incoming>, ctx: &Ctx) -> Response<Body> {
    let code = req
        .uri()
        .query()
        .and_then(|q| url::form_urlencoded::parse(q.as_bytes()).find(|(k, _)| k == "c").map(|(_, v)| v.into_owned()))
        .unwrap_or_default();
    let pending = {
        let mut inner = lock();
        let now = Instant::now();
        inner.codes.retain(|_, c| c.expires > now);
        match inner.codes.get(&code) {
            Some(c) if c.key == ctx.key => inner.codes.remove(&code),
            _ => None,
        }
    };
    let Some(pending) = pending else {
        // 码过期、用过、或者是别的预览的：已经有 cookie 就当刷新，直接进首页；没有就请他回去重点。
        if has_cookie(req.headers(), &cookie_name(ctx.port), &ctx.secret) {
            return redirect("/", None);
        }
        return need_link_page();
    };
    let cookie = format!("{}={}; Path=/; HttpOnly; SameSite=Lax", cookie_name(ctx.port), ctx.secret);
    redirect(&safe_local_path(&pending.path), Some(cookie))
}

fn redirect(location: &str, cookie: Option<String>) -> Response<Body> {
    let mut b = Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, location)
        .header(header::CACHE_CONTROL, "no-store")
        // 换码地址里有码：不让它经 Referer 带给页面引用的第三方资源。
        .header(header::REFERRER_POLICY, "no-referrer");
    if let Some(c) = cookie {
        b = b.header(header::SET_COOKIE, c);
    }
    b.body(empty()).unwrap_or_else(|_| plain(StatusCode::INTERNAL_SERVER_ERROR, "redirect"))
}

/// 跳转目标只能是本站路径：以 `/` 开头、不是 `//`（协议相对地址会跳去别的主机）也不是 `/\`。
fn safe_local_path(path: &str) -> String {
    if path.starts_with('/') && !path.starts_with("//") && !path.starts_with("/\\") {
        path.to_string()
    } else {
        "/".to_string()
    }
}

fn cookie_name(proxy_port: u16) -> String {
    format!("meowo_pv_{proxy_port}")
}

fn has_cookie(headers: &HeaderMap, name: &str, secret: &str) -> bool {
    headers.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).any(|line| {
        line.split(';').filter_map(|kv| kv.trim().split_once('=')).any(|(k, v)| k == name && crate::remote::token_matches_pub(secret, v))
    })
}

/// 入站 Host 只收 IP 字面量（可带端口）。域名一律拒：DNS rebinding 时 Host 是攻击者的域名。
fn host_is_ip_literal(host: &str) -> bool {
    let h = host.trim();
    if let Some(rest) = h.strip_prefix('[') {
        return rest.split_once(']').is_some_and(|(ip, tail)| ip.parse::<Ipv6Addr>().is_ok() && (tail.is_empty() || tail.starts_with(':')));
    }
    let name = h.rsplit_once(':').map(|(n, _)| n).unwrap_or(h);
    name.parse::<Ipv4Addr>().is_ok()
}

/// 发给 dev server 前改头：它以为自己被 localhost 访问（Vite/webpack 查 Host，Next.js 查 Origin/Referer）。
pub(crate) fn rewrite_request_headers(headers: &mut HeaderMap, inbound_host: &str, target_port: u16, proxy_port: u16) {
    let local = format!("localhost:{target_port}");
    let local_origin = format!("http://{local}");
    if let Ok(v) = HeaderValue::from_str(&local) {
        headers.insert(header::HOST, v);
    }
    // Origin 只在它就是预览页自己（同一个代理地址）时改：别的页面（同 IP 的另一个预览、外站）来的请求
    // 原样交给 dev server，让它自带的来源检查照常生效（独立审查 #9）。
    let inbound_origin = format!("http://{inbound_host}");
    if headers.get(header::ORIGIN).is_some_and(|o| o.as_bytes() == inbound_origin.as_bytes()) {
        if let Ok(v) = HeaderValue::from_str(&local_origin) {
            headers.insert(header::ORIGIN, v);
        }
    }
    if let Some(r) = headers.get(header::REFERER).and_then(|v| v.to_str().ok()).map(str::to_string) {
        if let Some(rest) = r.strip_prefix(&inbound_origin) {
            if rest.is_empty() || rest.starts_with('/') || rest.starts_with('?') {
                if let Ok(v) = HeaderValue::from_str(&format!("{local_origin}{rest}")) {
                    headers.insert(header::REFERER, v);
                }
            }
        }
    }
    // 删掉 Meowo 自己的预览 cookie（所有端口的），别让它进 dev server 的日志和应用代码。
    let own = cookie_name(proxy_port);
    let kept: Vec<String> = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|line| line.split(';').map(|kv| kv.trim().to_string()).collect::<Vec<_>>())
        .filter(|kv| !kv.is_empty() && !kv.starts_with("meowo_pv_") && !kv.starts_with(&format!("{own}=")))
        .collect();
    headers.remove(header::COOKIE);
    if !kept.is_empty() {
        if let Ok(v) = HeaderValue::from_str(&kept.join("; ")) {
            headers.insert(header::COOKIE, v);
        }
    }
    for h in ["forwarded", "x-forwarded-for", "x-forwarded-host", "x-forwarded-proto", "x-forwarded-port"] {
        headers.remove(h);
    }
}

/// dev server 回来的头：指向 localhost 的跳转改成本站路径；Set-Cookie 去掉 Secure / Domain
/// （明文 http + IP 主机下带着它们手机存不住，登录态就丢了），SameSite=None 改 Lax（None 要求 Secure）。
pub(crate) fn rewrite_response_headers(headers: &mut HeaderMap, target_port: u16) {
    if let Some(loc) = headers.get(header::LOCATION).and_then(|v| v.to_str().ok()).map(str::to_string) {
        if let Some(path) = local_location_path(&loc, target_port) {
            if let Ok(v) = HeaderValue::from_str(&path) {
                headers.insert(header::LOCATION, v);
            }
        }
    }
    let cookies: Vec<String> = headers.get_all(header::SET_COOKIE).iter().filter_map(|v| v.to_str().ok()).map(rewrite_set_cookie).collect();
    if !cookies.is_empty() {
        headers.remove(header::SET_COOKIE);
        for c in cookies {
            if let Ok(v) = HeaderValue::from_str(&c) {
                headers.append(header::SET_COOKIE, v);
            }
        }
    }
    // dev server 偶尔带 HSTS（自定义中间件）：明文预览端口上收到它，手机会把这个 IP 记成只走 https。
    headers.remove(header::STRICT_TRANSPORT_SECURITY);
}

fn local_location_path(loc: &str, target_port: u16) -> Option<String> {
    let url = url::Url::parse(loc).ok()?;
    let local = match url.host()? {
        url::Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(ip) => ip.is_loopback() || ip.is_unspecified(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
    };
    if !local || url.port_or_known_default() != Some(target_port) {
        return None;
    }
    let mut out = url.path().to_string();
    if let Some(q) = url.query() {
        out.push('?');
        out.push_str(q);
    }
    if let Some(f) = url.fragment() {
        out.push('#');
        out.push_str(f);
    }
    // `http://localhost:5173//evil.example/` 去掉主机后是 `//evil.example/`，浏览器会当成外站（独立审查 #6）。
    Some(safe_local_path(&out))
}

fn rewrite_set_cookie(raw: &str) -> String {
    raw.split(';')
        .map(str::trim)
        .filter(|part| {
            let key = part.split('=').next().unwrap_or("").trim().to_ascii_lowercase();
            key != "secure" && key != "domain"
        })
        .map(|part| {
            if part.eq_ignore_ascii_case("samesite=none") {
                "SameSite=Lax".to_string()
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

// ═══ 自己的几页 ═══

fn empty() -> Body {
    Full::new(Bytes::new()).map_err(|never| match never {}).boxed()
}

fn plain(status: StatusCode, text: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Full::new(Bytes::from_static(text.as_bytes())).map_err(|never| match never {}).boxed())
        .unwrap_or_else(|_| Response::new(empty()))
}

/// 中英两段都给：手机浏览器的语言不一定和 Meowo 设置一致，这一页又拿不到设置。
fn page(status: StatusCode, zh_title: &str, zh_body: &str, en_title: &str, en_body: &str) -> Response<Body> {
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<meta name=\"referrer\" content=\"no-referrer\"><title>Meowo</title><style>\
:root{{color-scheme:light dark}}body{{margin:0;padding:48px 24px;font:16px/1.6 system-ui,-apple-system,sans-serif;background:Canvas;color:CanvasText}}\
main{{max-width:420px;margin:0 auto}}h1{{font-size:20px;margin:0 0 8px}}p{{margin:0 0 28px;opacity:.72}}</style></head>\
<body><main><h1>{zh_title}</h1><p>{zh_body}</p><h1>{en_title}</h1><p>{en_body}</p></main></body></html>"
    );
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Full::new(Bytes::from(html)).map_err(|never| match never {}).boxed())
        .unwrap_or_else(|_| Response::new(empty()))
}

fn need_link_page() -> Response<Body> {
    page(
        StatusCode::UNAUTHORIZED,
        "请从 Meowo 里打开",
        "这个预览地址要从 Meowo 手机页的对话里点链接进入。回到 Meowo，再点一次那个链接。",
        "Open it from Meowo",
        "This preview opens from a link in the Meowo chat on your phone. Go back to Meowo and tap the link again.",
    )
}

fn not_session_page(target_port: u16) -> Response<Body> {
    page(
        StatusCode::FORBIDDEN,
        "这个端口不是本会话的",
        &format!("电脑上 {target_port} 端口现在是别的程序在用，或者 dev server 被放到了会话进程之外（比如用 & 或 nohup 在后台起的），Meowo 不会把它开给手机。可以让 AI 用它自己的后台运行方式重新起 dev server。"),
        "This port isn't from this session",
        &format!("Port {target_port} on your computer is used by another program, or the dev server was started outside this session's processes (for example with & or nohup), so Meowo won't open it to your phone. Ask the AI to restart the dev server with its own background-run feature."),
    )
}

fn hostname_page() -> Response<Body> {
    page(
        StatusCode::MISDIRECTED_REQUEST,
        "请用 IP 地址打开",
        "预览只认电脑的 IP 地址。回到 Meowo 手机页，再点一次对话里的链接。",
        "Open it by IP address",
        "The preview only accepts your computer's IP address. Go back to Meowo on your phone and tap the link in the chat again.",
    )
}

fn not_running_page(target_port: u16) -> Response<Body> {
    page(
        StatusCode::BAD_GATEWAY,
        "预览没在运行",
        &format!("电脑上 {target_port} 端口现在没有响应，dev server 可能停了或者正在重启。等它起来后刷新这一页。"),
        "Preview isn't running",
        &format!("Nothing is answering on port {target_port} on your computer. The dev server may have stopped or is restarting. Refresh once it's back."),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_loopback_http_urls() {
        assert_eq!(parse_local_url("http://localhost:5173/a/b?x=1#top").unwrap(), (5173, "/a/b?x=1#top".to_string()));
        assert_eq!(parse_local_url("http://127.0.0.1:3000").unwrap(), (3000, "/".to_string()));
        assert_eq!(parse_local_url("http://[::1]:4321/").unwrap(), (4321, "/".to_string()));
        assert_eq!(parse_local_url("http://0.0.0.0:8080/x").unwrap(), (8080, "/x".to_string()));
        assert_eq!(parse_local_url("http://LOCALHOST/").unwrap(), (80, "/".to_string()));
        assert_eq!(parse_local_url("https://localhost:5173").unwrap_err(), "preview/https");
        assert_eq!(parse_local_url("http://192.168.1.5:5173").unwrap_err(), "preview/not_local");
        assert_eq!(parse_local_url("http://localhost.evil.com:5173").unwrap_err(), "preview/not_local");
        assert_eq!(parse_local_url("file:///etc/passwd").unwrap_err(), "preview/bad_url");
        assert_eq!(parse_local_url("not a url").unwrap_err(), "preview/bad_url");
    }

    #[test]
    fn host_must_be_an_ip_literal() {
        assert!(host_is_ip_literal("192.168.1.5:40123"));
        assert!(host_is_ip_literal("100.64.0.2"));
        assert!(host_is_ip_literal("[fd7a::1]:40123"));
        assert!(!host_is_ip_literal("evil.example:40123"));
        assert!(!host_is_ip_literal("localhost:40123"));
        assert!(!host_is_ip_literal("[fd7a::1]x"));
    }

    #[test]
    fn request_headers_look_like_localhost_and_lose_meowo_cookies() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_static("192.168.1.5:40123"));
        h.insert(header::ORIGIN, HeaderValue::from_static("http://192.168.1.5:40123"));
        h.insert(header::REFERER, HeaderValue::from_static("http://192.168.1.5:40123/page?q=1"));
        h.insert(header::COOKIE, HeaderValue::from_static("a=1; meowo_pv_40123=s3cret; meowo_pv_40999=x; b=2"));
        h.insert("x-forwarded-for", HeaderValue::from_static("1.2.3.4"));
        rewrite_request_headers(&mut h, "192.168.1.5:40123", 5173, 40123);
        assert_eq!(h[header::HOST], "localhost:5173");
        assert_eq!(h[header::ORIGIN], "http://localhost:5173");
        assert_eq!(h[header::REFERER], "http://localhost:5173/page?q=1");
        assert_eq!(h[header::COOKIE], "a=1; b=2");
        assert!(h.get("x-forwarded-for").is_none());
    }

    #[test]
    fn foreign_referer_and_null_origin_are_left_alone() {
        let mut h = HeaderMap::new();
        h.insert(header::ORIGIN, HeaderValue::from_static("null"));
        h.insert(header::REFERER, HeaderValue::from_static("https://cdn.example/x"));
        h.insert(header::COOKIE, HeaderValue::from_static("meowo_pv_40123=s3cret"));
        rewrite_request_headers(&mut h, "192.168.1.5:40123", 5173, 40123);
        assert_eq!(h[header::ORIGIN], "null");
        assert_eq!(h[header::REFERER], "https://cdn.example/x");
        assert!(h.get(header::COOKIE).is_none());
    }

    #[test]
    fn response_headers_point_back_at_the_proxy() {
        let mut h = HeaderMap::new();
        h.insert(header::LOCATION, HeaderValue::from_static("http://localhost:5173/login?next=%2F"));
        h.append(header::SET_COOKIE, HeaderValue::from_static("sid=abc; Path=/; Secure; HttpOnly; Domain=localhost; SameSite=None"));
        h.append(header::SET_COOKIE, HeaderValue::from_static("theme=dark; Path=/"));
        h.insert(header::STRICT_TRANSPORT_SECURITY, HeaderValue::from_static("max-age=3600"));
        rewrite_response_headers(&mut h, 5173);
        assert_eq!(h[header::LOCATION], "/login?next=%2F");
        let cookies: Vec<_> = h.get_all(header::SET_COOKIE).iter().map(|v| v.to_str().unwrap().to_string()).collect();
        assert_eq!(cookies, vec!["sid=abc; Path=/; HttpOnly; SameSite=Lax", "theme=dark; Path=/"]);
        assert!(h.get(header::STRICT_TRANSPORT_SECURITY).is_none());
    }

    #[test]
    fn redirects_elsewhere_are_untouched() {
        let mut h = HeaderMap::new();
        h.insert(header::LOCATION, HeaderValue::from_static("http://localhost:3000/api"));
        rewrite_response_headers(&mut h, 5173);
        assert_eq!(h[header::LOCATION], "http://localhost:3000/api");
        h.insert(header::LOCATION, HeaderValue::from_static("https://github.com/login"));
        rewrite_response_headers(&mut h, 5173);
        assert_eq!(h[header::LOCATION], "https://github.com/login");
        h.insert(header::LOCATION, HeaderValue::from_static("/relative"));
        rewrite_response_headers(&mut h, 5173);
        assert_eq!(h[header::LOCATION], "/relative");
    }

    #[test]
    fn rewritten_redirect_cannot_leave_the_host() {
        let mut h = HeaderMap::new();
        h.insert(header::LOCATION, HeaderValue::from_static("http://localhost:5173//evil.example/x"));
        rewrite_response_headers(&mut h, 5173);
        assert_eq!(h[header::LOCATION], "/");
    }

    #[test]
    fn origin_from_elsewhere_is_left_for_the_dev_server_to_judge() {
        let mut h = HeaderMap::new();
        h.insert(header::ORIGIN, HeaderValue::from_static("http://192.168.1.5:5199"));
        rewrite_request_headers(&mut h, "192.168.1.5:5173", 5173, 5173);
        assert_eq!(h[header::ORIGIN], "http://192.168.1.5:5199");
    }

    /// 同号代理自己那条局域网监听不算进「dev server 是不是只听回环」（第三轮回归：算进去就 60 秒内被回收）。
    #[test]
    fn own_same_port_listener_does_not_count_against_the_dev_server() {
        let lan: IpAddr = "192.168.5.108".parse().unwrap();
        let listeners = vec![(IpAddr::V4(Ipv4Addr::LOCALHOST), 10), (lan, 99)];
        assert!(!same_port_free(&listeners, None));
        assert!(same_port_free(&listeners, Some(lan)));
        // dev server 自己改成了对所有网卡开放：就算排除代理自己，也该让出同号端口。
        let widened = vec![(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 10), (lan, 99)];
        assert!(!same_port_free(&widened, Some(lan)));
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5173);
        assert_eq!(verdict(Ok((addr, same_port_free(&listeners, Some(lan)))), true), Verdict::Fine);
        assert_eq!(verdict(Ok((addr, same_port_free(&widened, Some(lan)))), true), Verdict::Drop);
        assert_eq!(verdict(Ok((addr, false)), false), Verdict::Fine);
        assert_eq!(verdict(Err("preview/not_session".into()), true), Verdict::Drop);
        assert_eq!(verdict(Err("preview/not_listening".into()), true), Verdict::Missing);
        assert_eq!(verdict(Err("preview/owner_unknown".into()), false), Verdict::Missing);
    }

    /// dev server 自己开在所有网卡上时不占同号端口（Windows 上会把它的局域网入口抢走）。
    #[test]
    fn same_port_only_when_dev_server_is_loopback_only() {
        let me = Owner::Session { roots: vec![std::process::id()] };
        let local = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let p = local.local_addr().unwrap().port();
        assert_eq!(resolve_target(p, &me).unwrap(), (SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), p), true));
        let all = std::net::TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).unwrap();
        let q = all.local_addr().unwrap().port();
        assert_eq!(resolve_target(q, &me).unwrap(), (SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), q), false));
        // 不属于会话（根进程不是本进程）：拒。
        let other = Owner::Session { roots: vec![u32::MAX - 7] };
        assert_eq!(resolve_target(p, &other).unwrap_err(), "preview/not_session");
        drop(local);
        assert_eq!(resolve_target(p, &me).unwrap_err(), "preview/not_listening");
    }

    #[test]
    fn redirect_target_stays_on_this_host() {
        assert_eq!(safe_local_path("/a?b#c"), "/a?b#c");
        assert_eq!(safe_local_path("//evil.example/x"), "/");
        assert_eq!(safe_local_path("/\\evil.example"), "/");
        assert_eq!(safe_local_path("http://evil.example"), "/");
    }

    #[test]
    fn cookie_must_match_name_and_secret() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_static("x=1; meowo_pv_40123=abc"));
        assert!(has_cookie(&h, "meowo_pv_40123", "abc"));
        assert!(!has_cookie(&h, "meowo_pv_40123", "abd"));
        assert!(!has_cookie(&h, "meowo_pv_40124", "abc"));
    }

    #[test]
    fn owned_listener_prefers_ipv4_and_ignores_other_processes() {
        let v4: SocketAddr = "127.0.0.1:5173".parse().unwrap();
        let v6: SocketAddr = "[::1]:5173".parse().unwrap();
        let tree: std::collections::HashSet<u32> = [10, 11].into_iter().collect();
        assert_eq!(pick_owned(&[(v6, 11), (v4, 11)], &tree), Some(v4));
        assert_eq!(pick_owned(&[(v6, 11), (v4, 99)], &tree), Some(v6));
        assert_eq!(pick_owned(&[(v4, 99)], &tree), None);
    }

    #[test]
    fn lsof_listing_parses() {
        let text = "p4242\nn127.0.0.1:5173\nn[::1]:5173\np77\nn*:5173\n";
        let got = parse_lsof_listeners(text);
        assert_eq!(
            got,
            vec![
                ("127.0.0.1".parse().unwrap(), 4242),
                ("::1".parse().unwrap(), 4242),
                (IpAddr::V4(Ipv4Addr::UNSPECIFIED), 77),
            ]
        );
    }

    /// 监听表查得到本进程自己开的监听（Windows 走 GetExtendedTcpTable，其余平台走 lsof）。
    #[test]
    fn listener_table_sees_our_own_socket() {
        let l4 = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = l4.local_addr().unwrap().port();
        let me = std::process::id();
        let found = listener_pids(port).expect("本平台查得到监听表");
        assert!(found.contains(&(IpAddr::V4(Ipv4Addr::LOCALHOST), me)), "{found:?}");
        // 只开在 ::1 上的（Node 18+ 的 localhost 可能这样）也认得出。
        if let Ok(l6) = std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)) {
            let p6 = l6.local_addr().unwrap().port();
            let found = listener_pids(p6).unwrap();
            assert!(found.contains(&(IpAddr::V6(Ipv6Addr::LOCALHOST), me)), "{found:?}");
        }
        // 没人监听的端口：空表，不是「查不了」。
        drop(l4);
        assert!(listener_pids(port).unwrap().iter().all(|(_, pid)| *pid != me));
    }

    /// 手动验真 dev server 用：`PV_TARGETS=5199,5198 cargo test --lib preview_manual -- --ignored --nocapture`。
    /// 每个目标开一个代理、放一个固定码 `pv-<目标端口>`，打印代理端口后挂 PV_SECS 秒（默认 300）。
    #[test]
    #[ignore]
    fn preview_manual() {
        let targets: Vec<u16> = std::env::var("PV_TARGETS").unwrap_or_default().split(',').filter_map(|p| p.trim().parse().ok()).collect();
        let secs: u64 = std::env::var("PV_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
        tauri::async_runtime::block_on(async move {
            for t in targets {
                // 和正式路径一样按实际监听的地址连（Vite 在 Windows 上常只听 ::1），只是不查属主。
                let target = listener_pids(t)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(ip, _)| SocketAddr::new(if ip.is_ipv4() { IpAddr::V4(Ipv4Addr::LOCALHOST) } else { IpAddr::V6(Ipv6Addr::LOCALHOST) }, t))
                    .min_by_key(|a| !a.is_ipv4())
                    .expect("目标端口没人监听");
                let key = (IpAddr::V4(std::env::var("PV_LISTEN").ok().and_then(|s| s.parse().ok()).unwrap_or(Ipv4Addr::LOCALHOST)), t);
                let (_, same_port_ok) = resolve_target(t, &Owner::Anyone).unwrap();
                let generation = { let mut inner = lock(); inner.bind = Some(key.0); inner.generation };
                let port = ensure_proxy(key, target, Owner::Anyone, same_port_ok, generation).await.unwrap();
                for n in 0..10 {
                    let code = if n == 0 { format!("pv-{t}") } else { format!("pv-{t}-{n}") };
                    lock().codes.insert(code, PendingCode { key, path: "/".into(), expires: Instant::now() + Duration::from_secs(secs) });
                }
                println!("PROXY {t} -> {port}");
            }
            tokio::time::sleep(Duration::from_secs(secs)).await;
        });
    }

    /// 走真实的 open()（归属判定、一次性码、代次）验真 dev server：
    /// `PV_LISTEN=<本机IP> PV_CASES="<url>|<根pid>;..." cargo test --lib preview_open_manual -- --ignored --nocapture`。
    /// 打印每个用例的预览地址（或错误码），然后挂 PV_SECS 秒。
    #[test]
    #[ignore]
    fn preview_open_manual() {
        let listen: IpAddr = std::env::var("PV_LISTEN").unwrap().parse().unwrap();
        let cases = std::env::var("PV_CASES").unwrap_or_default();
        let secs: u64 = std::env::var("PV_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(300);
        tauri::async_runtime::block_on(async move {
            bridge_started(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
            for case in cases.split(';').filter(|c| !c.trim().is_empty()) {
                let parts: Vec<&str> = case.split('|').collect();
                let owner = Owner::Session { roots: vec![parts[1].parse().unwrap()] };
                match open(parts[0].to_string(), owner, true, Some(format!("{listen}:18620"))).await {
                    Ok(t) => println!("OPEN {} -> http://{}:{}{}", parts[0], t.host, t.port, t.path),
                    Err(e) => println!("OPEN {} -> ERR {e}", parts[0]),
                }
            }
            tokio::time::sleep(Duration::from_secs(secs)).await;
        });
    }

    // ── 端到端：真起一个上游与代理，走一遍换码、转发、改头、websocket、回收 ──

    async fn upstream() -> u16 {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tauri::async_runtime::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else { break };
                tauri::async_runtime::spawn(async move {
                    let svc = hyper::service::service_fn(|mut req: Request<Incoming>| async move {
                        if req.headers().contains_key(header::UPGRADE) {
                            let up = hyper::upgrade::on(&mut req);
                            tauri::async_runtime::spawn(async move {
                                if let Ok(u) = up.await {
                                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                                    let mut io = TokioIo::new(u);
                                    // 像 Vite 那样：连上就先推一条，不等客户端开口。
                                    // 隔一会儿再推（不和 101 挤在同一个包里），之后再主动推一条。
                                    tokio::time::sleep(Duration::from_millis(300)).await;
                                    let _ = io.write_all(b"hi").await;
                                    let mut buf = [0u8; 5];
                                    if io.read_exact(&mut buf).await.is_ok() {
                                        let _ = io.write_all(&buf).await;
                                    }
                                    tokio::time::sleep(Duration::from_millis(300)).await;
                                    let _ = io.write_all(b"yo").await;
                                }
                            });
                            return Ok::<_, hyper::Error>(
                                Response::builder()
                                    .status(StatusCode::SWITCHING_PROTOCOLS)
                                    .header(header::UPGRADE, "websocket")
                                    .header(header::CONNECTION, "upgrade")
                                    .body(empty())
                                    .unwrap(),
                            );
                        }
                        let get = |name: header::HeaderName| req.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or("-").to_string();
                        let echo = format!("host={} origin={} cookie={}", get(header::HOST), get(header::ORIGIN), get(header::COOKIE));
                        let host = get(header::HOST);
                        Ok(Response::builder()
                            .header(header::SET_COOKIE, "sid=1; Secure; SameSite=None")
                            .header(header::LOCATION, format!("http://{host}/next"))
                            .body(Full::new(Bytes::from(echo)).map_err(|never| match never {}).boxed())
                            .unwrap())
                    });
                    let _ = hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(stream), svc).with_upgrades().await;
                });
            }
        });
        port
    }

    async fn raw(port: u16, request: String) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut s = tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
        s.write_all(request.as_bytes()).await.unwrap();
        let mut out = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(3), s.read_to_end(&mut out)).await;
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn proxies_end_to_end() {
        tauri::async_runtime::block_on(async {
            let target_port = upstream().await;
            let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), target_port);
            // 代理也开在 127.0.0.1：同号端口被上游占着，退到随机端口。归属按本测试进程认（上游就是它开的）。
            let key = (IpAddr::V4(Ipv4Addr::LOCALHOST), target_port);
            let owner = Owner::Session { roots: vec![std::process::id()] };
            let generation = { let mut inner = lock(); inner.bind = Some(key.0); inner.generation };
            let port = ensure_proxy(key, target, owner.clone(), true, generation).await.unwrap();
            assert_ne!(port, target_port);
            // 同一目标再开一次：同一个代理端口。
            assert_eq!(ensure_proxy(key, target, owner.clone(), true, generation).await.unwrap(), port);
            // 代次变了（桥在打开途中停过）：作废，不开出新代理。
            assert_eq!(ensure_proxy((key.0, 1), target, owner, true, generation + 1).await.unwrap_err(), "preview/remote_off");
            let host = format!("127.0.0.1:{port}");

            // 没有 cookie：请他回 Meowo 点链接。
            let resp = raw(port, format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")).await;
            assert!(resp.starts_with("HTTP/1.1 401"), "{resp}");
            // Host 是域名：拒。
            let resp = raw(port, format!("GET / HTTP/1.1\r\nHost: evil.example:{port}\r\nConnection: close\r\n\r\n")).await;
            assert!(resp.starts_with("HTTP/1.1 421"), "{resp}");
            // Host 是 IP 但不是代理监听的那个：同样拒。
            let resp = raw(port, format!("GET / HTTP/1.1\r\nHost: 10.9.8.7:{port}\r\nConnection: close\r\n\r\n")).await;
            assert!(resp.starts_with("HTTP/1.1 421"), "{resp}");
            // 绝对 URI 形式（当跳板用）：拒。
            let resp = raw(port, format!("GET http://10.0.0.1/ HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")).await;
            assert!(resp.starts_with("HTTP/1.1 400"), "{resp}");

            // 换码：种 cookie、跳到原路径；同一个码第二次不认。
            lock().codes.insert("code1".into(), PendingCode { key, path: "/app?x=1".into(), expires: Instant::now() + CODE_TTL });
            let resp = raw(port, format!("GET {AUTH_PATH}?c=code1 HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")).await;
            assert!(resp.starts_with("HTTP/1.1 302"), "{resp}");
            assert!(resp.contains("location: /app?x=1"), "{resp}");
            let cookie_line = resp.lines().find(|l| l.to_ascii_lowercase().starts_with("set-cookie:")).unwrap().to_string();
            assert!(cookie_line.contains("HttpOnly") && cookie_line.contains(&format!("meowo_pv_{port}=")), "{cookie_line}");
            let cookie = cookie_line.split_once(':').unwrap().1.trim().split(';').next().unwrap().to_string();
            let resp = raw(port, format!("GET {AUTH_PATH}?c=code1 HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n")).await;
            assert!(resp.starts_with("HTTP/1.1 401"), "{resp}");

            // 带 cookie：转发，上游看到的是 localhost、没有 Meowo 的 cookie；回来的头改过。
            let resp = raw(
                port,
                format!("GET /app HTTP/1.1\r\nHost: {host}\r\nOrigin: http://{host}\r\nCookie: a=1; {cookie}\r\nConnection: close\r\n\r\n"),
            )
            .await;
            assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
            assert!(resp.contains(&format!("host=localhost:{target_port} origin=http://localhost:{target_port} cookie=a=1")), "{resp}");
            assert!(resp.contains("location: /next"), "{resp}");
            assert!(resp.contains("set-cookie: sid=1; SameSite=Lax"), "{resp}");

            // websocket：升级后字节双向透传。
            {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut s = tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
                let req = format!("GET /hmr HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nOrigin: http://{host}\r\n\r\n");
                s.write_all(req.as_bytes()).await.unwrap();
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    s.read_exact(&mut byte).await.unwrap();
                    head.push(byte[0]);
                }
                assert!(String::from_utf8_lossy(&head).starts_with("HTTP/1.1 101"));
                // 服务器先推的那条要马上到，不能等客户端发了东西才冲出来。
                let mut first = [0u8; 2];
                tokio::time::timeout(Duration::from_secs(2), s.read_exact(&mut first)).await.expect("服务器先推的消息没到").unwrap();
                assert_eq!(&first, b"hi");
                s.write_all(b"hello").await.unwrap();
                let mut echo = [0u8; 5];
                tokio::time::timeout(Duration::from_secs(3), s.read_exact(&mut echo)).await.unwrap().unwrap();
                assert_eq!(&echo, b"hello");
                let mut push = [0u8; 2];
                tokio::time::timeout(Duration::from_secs(2), s.read_exact(&mut push)).await.expect("服务器后推的消息没到").unwrap();
                assert_eq!(&push, b"yo");
            }

            // 留一条热更新隧道开着，看回收时它是否一起断。
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut tunnel = tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
            let req = format!("GET /hmr HTTP/1.1\r\nHost: {host}\r\nCookie: {cookie}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n");
            tunnel.write_all(req.as_bytes()).await.unwrap();
            let mut first = vec![0u8; 512];
            let n = tokio::time::timeout(Duration::from_secs(3), tunnel.read(&mut first)).await.unwrap().unwrap();
            assert!(String::from_utf8_lossy(&first[..n]).starts_with("HTTP/1.1 101"));
            tokio::time::sleep(Duration::from_millis(400)).await;

            // 远程桥停了：代理一起关，连不上了；已建好的隧道也断。
            bridge_stopped();
            let mut rest = Vec::new();
            let closed = tokio::time::timeout(Duration::from_secs(3), tunnel.read_to_end(&mut rest)).await;
            assert!(matches!(closed, Ok(Ok(_)) | Ok(Err(_))), "回收后隧道还通着");
            tokio::time::sleep(Duration::from_millis(200)).await;
            let gone = tokio::time::timeout(Duration::from_secs(1), tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))).await;
            assert!(!matches!(gone, Ok(Ok(_))), "代理端口还开着");
        });
    }
}

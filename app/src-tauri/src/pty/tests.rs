use super::*;

// 注：曾有一个直接驱动 ConPTY 的实证测试，结论已固化到 finalize_exit / waiter 的注释里
// （子进程退出后 reader 不 EOF；drop master 也唤不醒阻塞中的 read）。测试本身依赖
// 测试环境里 ConPTY 的交互行为（cmd /c 甚至不退出），过于 flaky，不保留。

#[test]
fn readable_tail_strips_ansi_and_keeps_last_lines() {
    // 秒退诊断的原料是 TUI 首帧：清屏序列 + 标题 OSC + 真正的报错行。
    let raw = b"\x1b[2J\x1b[H\x1b]0;claude\x07noise line\r\nError: Session e9a is already in use\r\n\x1b[31mPlease close the other client.\x1b[0m\r\n";
    let tail = readable_tail(raw, 240);
    assert!(tail.contains("already in use"), "tail={tail}");
    assert!(tail.contains("Please close"), "tail={tail}");
    assert!(!tail.contains('\x1b'));
    // 中文按 UTF-8 完整解码，不得拆成乱码字节。
    let zh = readable_tail("错误：会话被占用\n".as_bytes(), 240);
    assert_eq!(zh, "错误：会话被占用");
    // GBK 输出（中文 Windows 常见）也要解得出，不是整屏 U+FFFD（"错误"的 GBK 字节）。
    let gbk = readable_tail(&[0xB4, 0xED, 0xCE, 0xF3], 240);
    assert_eq!(gbk, "错误");
    // 限长截断加省略号。
    let long = readable_tail(&[b'a'; 500], 10);
    assert_eq!(long, "aaaaaaaaaa…");
}

#[test]
fn pty_size_is_clamped_to_safe_bounds() {
    let tiny = size(0, 1);
    assert_eq!((tiny.cols, tiny.rows), (2, 2));
    let huge = size(u16::MAX, u16::MAX);
    assert_eq!((huge.cols, huge.rows), (500, 500));
}

/// panic 展开时 guard 必须摘掉 starting 占位——否则该会话到进程重启都无法再起，
/// 且静默（start 的 is_err 分支在展开路径上执行不到）。
#[test]
fn start_guard_clears_placeholder_on_unwind() {
    let broker = PtyBroker::default();
    broker.begin_start(4242).unwrap();
    let cloned = broker.clone();
    let handle = std::thread::spawn(move || {
        let _guard = StartPlaceholderGuard {
            broker: &cloned,
            session_id: 4242,
            armed: true,
        };
        panic!("模拟 start_spawned 里的原生调用崩溃");
    });
    assert!(handle.join().is_err());
    assert!(broker.starting.lock().unwrap().is_empty());
}

/// disarm（成功路径）后 Drop 不得误清占位——占位由登记入表后的 end_start 负责。
#[test]
fn disarmed_guard_leaves_placeholder_alone() {
    let broker = PtyBroker::default();
    broker.begin_start(4243).unwrap();
    {
        let mut guard = StartPlaceholderGuard {
            broker: &broker,
            session_id: 4243,
            armed: true,
        };
        guard.armed = false;
    }
    assert!(broker.starting.lock().unwrap().contains(&4243));
}

#[test]
fn inactive_snapshot_is_empty_and_large_input_is_rejected() {
    let broker = PtyBroker::default();
    let snapshot = broker.snapshot(42, 0);
    assert!(!snapshot.active);
    assert!(!snapshot.exited);
    assert!(snapshot.data.is_empty());
    assert_eq!(snapshot.exit_code, None);
    assert!(broker.write(42, &vec![0; 64 * 1024 + 1]).is_err());
}

/// 最小可用的 ManagedPty：不碰真 PTY，供占位/登记/shutdown 的状态机测试。
#[derive(Debug)]
struct DummyChild;
impl portable_pty::ChildKiller for DummyChild {
    fn kill(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
        Box::new(DummyChild)
    }
}
impl portable_pty::Child for DummyChild {
    fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        Ok(None)
    }
    fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
        Err(std::io::Error::other("dummy child never exits"))
    }
    fn process_id(&self) -> Option<u32> {
        None
    }
    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

/// 「活着但已收不到任何输出」的子进程：EOF 先于进程退出的场景（conhost 先死、
/// agent 卡死）。wait 在真实世界会无限阻塞，测试里以 Err 代表「未先 kill 就挂死」。
#[derive(Debug)]
struct HungChild {
    killed: Arc<AtomicBool>,
}
impl portable_pty::ChildKiller for HungChild {
    fn kill(&mut self) -> std::io::Result<()> {
        self.killed.store(true, Ordering::Release);
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
        Box::new(HungChild {
            killed: self.killed.clone(),
        })
    }
}
impl portable_pty::Child for HungChild {
    fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        // kill 生效后 try_wait 能拿到退出码——贴近真实 TerminateProcess 成功的行为。
        if self.killed.load(Ordering::Acquire) {
            Ok(Some(portable_pty::ExitStatus::with_exit_code(1)))
        } else {
            Ok(None)
        }
    }
    fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
        // 真实世界的 wait 对杀不死的进程会永久阻塞并拿着 child 锁不还——
        // 收尾路径绝不许再调它，测试里直接以 panic 当哨兵。
        panic!("reap_child 不得再调无限阻塞的 wait()");
    }
    fn process_id(&self) -> Option<u32> {
        None
    }
    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

/// 「TerminateProcess 静默无效」的子进程：kill 恒返回 Ok（portable-pty 0.9 的谎报），
/// 但 try_wait 永远拿不到退出——卡死在 ConPTY 内核 I/O 的真实场景。
#[derive(Debug)]
struct NeverDiesChild {
    killed: Arc<AtomicBool>,
}
impl portable_pty::ChildKiller for NeverDiesChild {
    fn kill(&mut self) -> std::io::Result<()> {
        self.killed.store(true, Ordering::Release);
        Ok(())
    }
    fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> {
        Box::new(NeverDiesChild {
            killed: self.killed.clone(),
        })
    }
}
impl portable_pty::Child for NeverDiesChild {
    fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
        Ok(None)
    }
    fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
        panic!("reap_child 不得再调无限阻塞的 wait()");
    }
    fn process_id(&self) -> Option<u32> {
        None
    }
    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}

/// 回归：EOF 收尾遇到还活着的子进程时必须先 kill 再收尸，不得阻塞 wait——
/// 真实翻车过：conhost 先死、claude 卡死，finalize_exit 拿着 child 锁在 wait 上
/// 永久不还，stop()（结束终端/结束会话）在同一把锁上排队，按钮点了毫无反应。
#[test]
fn reap_child_kills_live_child_instead_of_blocking_wait() {
    let killed = Arc::new(AtomicBool::new(false));
    let mut child: Box<dyn portable_pty::Child + Send + Sync> = Box::new(HungChild {
        killed: killed.clone(),
    });
    let code = reap_child(child.as_mut());
    assert!(killed.load(Ordering::Acquire), "活着的子进程应先被 kill");
    assert_eq!(code, Some(1), "kill 后应立即收到退出码");
}

/// 回归：TerminateProcess 静默无效（卡死在 ConPTY 内核 I/O）时，收尸必须在超时后
/// 放弃返回 None，而不是无限阻塞在 wait() 上拿着 child 锁不还。
#[test]
fn reap_child_gives_up_on_unkillable_child() {
    let killed = Arc::new(AtomicBool::new(false));
    let mut child: Box<dyn portable_pty::Child + Send + Sync> = Box::new(NeverDiesChild {
        killed: killed.clone(),
    });
    let start = std::time::Instant::now();
    let code = reap_child_within(
        child.as_mut(),
        std::time::Duration::from_millis(30),
        std::time::Duration::from_millis(50),
    );
    assert!(killed.load(Ordering::Acquire), "宽限到点应尝试 kill");
    assert_eq!(code, None, "杀不死就放弃退出码");
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "必须在超时上限附近返回，不得无限等待"
    );
}

/// 有界投递的三种结局。队满超时必须在上限附近返回——这是输出链不反压的根基：
/// emitter/UI 卡死时 reader 靠它丢帧保读，agent 才不会被 meowo 自己拖成假死。
#[test]
fn offer_with_deadline_times_out_when_full() {
    let (tx, rx) = mpsc::sync_channel::<u8>(1);
    assert_eq!(
        offer_with_deadline(&tx, 1, std::time::Duration::from_millis(50)),
        Offer::Sent
    );
    let start = std::time::Instant::now();
    assert_eq!(
        offer_with_deadline(&tx, 2, std::time::Duration::from_millis(50)),
        Offer::TimedOut,
        "队满且无人消费:到点放弃"
    );
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "必须在超时上限附近返回"
    );
    drop(rx);
    assert_eq!(
        offer_with_deadline(&tx, 3, std::time::Duration::from_millis(50)),
        Offer::Disconnected
    );
}

/// 注销「正在看」必须是 CAS：React 重挂时旧实例的 cleanup 可能晚于新实例的注册，
/// 无条件清零会抹掉新注册、实时流断掉（快照兜底下画面 80ms 一跳）。
#[test]
fn viewer_clear_is_cas_scoped_to_own_registration() {
    let broker = PtyBroker::default();
    broker.set_viewer(7);
    broker.set_viewer(9); // 新实例先注册
    broker.clear_viewer(7); // 旧实例的迟到注销
    assert_eq!(broker.viewed_session.load(Ordering::Acquire), 9);
    broker.clear_viewer(9);
    assert_eq!(broker.viewed_session.load(Ordering::Acquire), 0);
}

/// 有界拿锁的三种结局。占用超时必须在上限附近返回——这是保证路径（waiter 升级链 /
/// finalize / shutdown）不被卡死在 ConPTY syscall 里的持锁者拖死的根基
/// （0.5.13「结束会话仍无效」的回归）。中毒按全仓策略恢复，破坏性收尾不因一次
/// 别处 panic 永久失效。
#[test]
fn lock_within_gives_up_on_held_lock_and_recovers_poison() {
    let mutex = Arc::new(Mutex::new(0u8));
    assert!(lock_within(&mutex, std::time::Duration::from_millis(20)).is_some());
    let guard = mutex.lock().unwrap();
    let start = std::time::Instant::now();
    assert!(
        lock_within(&mutex, std::time::Duration::from_millis(50)).is_none(),
        "锁被占:到点放弃"
    );
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "必须在超时上限附近返回"
    );
    drop(guard);
    let poisoner = mutex.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poisoner.lock().unwrap();
        panic!("毒化锁");
    })
    .join();
    assert!(
        lock_within(&mutex, std::time::Duration::from_millis(20)).is_some(),
        "中毒恢复,不永久失效"
    );
}

/// stop 升级链的档位判定：纯时间点注入，避开线程/时序 flakiness。
#[test]
fn stop_stage_escalates_by_elapsed_time() {
    let now = std::time::Instant::now();
    assert_eq!(stop_stage(None, now), StopStage::Wait, "未请求 stop 不升级");
    assert_eq!(stop_stage(Some(now), now), StopStage::Wait);
    assert_eq!(
        stop_stage(Some(now), now + std::time::Duration::from_millis(500)),
        StopStage::Wait
    );
    assert_eq!(
        stop_stage(Some(now), now + STOP_KILL_TREE_AFTER),
        StopStage::KillTree
    );
    assert_eq!(
        stop_stage(Some(now), now + STOP_FORCE_FINALIZE_AFTER),
        StopStage::ForceFinalize
    );
}

/// stop() 必须先武装升级链（落时间戳）再 kill，且 kill 的谎报 Ok 不影响返回值语义。
#[test]
fn stop_records_request_timestamp_and_kills() {
    let broker = PtyBroker::default();
    let killed = Arc::new(AtomicBool::new(false));
    let managed = managed_with_child(
        7,
        Box::new(NeverDiesChild {
            killed: killed.clone(),
        }),
        None,
    );
    broker.sessions.lock().unwrap().insert(7, managed.clone());
    assert!(broker.stop(7).is_ok());
    assert!(killed.load(Ordering::Acquire), "stop 应发出 kill");
    let first = managed
        .stop_requested_at
        .lock()
        .unwrap()
        .expect("应落时间戳");
    // 重复点击不重置计时，否则连点会把升级一直往后推。
    assert!(broker.stop(7).is_ok());
    assert_eq!(managed.stop_requested_at.lock().unwrap().unwrap(), first);
}

/// 回归：临时负 id 被 SessionStart 认领换绑成真实 id 后（同一个 PTY 只换映射键），
/// 仍拿着旧 id 的调用方——对话窗要等 250ms 的 binding 轮询才跟上——不得被当成
/// 「会话不存在」。实拍过：新建会话后立刻打字，认领落地那一瞬的按键全数报
/// 「会话终端未在运行」，而终端画面好端端留在屏上。
#[test]
fn stale_temp_id_still_reaches_the_rebound_pty() {
    let broker = PtyBroker::default();
    let managed = dummy_managed(64);
    broker.sessions.lock().unwrap().insert(64, managed.clone());
    broker.attach.bindings.lock().unwrap().insert(-3, 64);
    // 快照：认领前后画面不能断档（active 掉成 false，前端就以为终端没了）。
    assert!(broker.snapshot(-3, 0).active);
    // 按键：找得到会话就不该再是「未运行」；假会话没有 writer 线程，止步于通道已关闭。
    assert_eq!(broker.write(-3, b"hi").unwrap_err(), "PTY 输入通道已关闭");
    // 网格自愈按旧 id 也要查得到真实尺寸，否则 TUI 停在占位宽度。
    managed
        .last_size
        .store(pack_size(100, 30), Ordering::Release);
    assert_eq!(broker.grid(-3), (100, 30));
    // 结束会话同理，旧 id 一样要落到真 PTY 上。
    assert!(broker.stop(-3).is_ok());
    // 没有绑定的陌生 id 仍老实报「未运行」——翻译是补漏，不是兜底遮蔽。
    assert_eq!(broker.write(-9, b"hi").unwrap_err(), "PTY 会话未运行");
    assert!(!broker.snapshot(-9, 0).active);
}

/// 回归：换绑必须连同绑定一起对外可见。曾经是「先摘键、出锁后再写 bindings」，
/// 那一瞬旧 id 两头落空，按键照样报「会话终端未在运行」。
#[test]
fn claim_rebind_publishes_the_binding_inside_the_same_lock() {
    let broker = PtyBroker::default();
    broker
        .sessions
        .lock()
        .unwrap()
        .insert(-5, dummy_managed(-5));
    let managed = broker
        .try_claim_rebind(-5, 88)
        .expect("锁未中毒")
        .expect("临时 id 已登记，应换绑成功");
    assert_eq!(managed.session_id.load(Ordering::Acquire), 88);
    assert_eq!(broker.binding(-5), Some(88), "绑定应与换绑同时可见");
    assert!(
        broker.snapshot(-5, 0).active,
        "旧 id 拉快照仍要看到活着的终端"
    );
}

/// 回归：同一个 PTY 换代多次（temp → A → B）后,最初的临时 id 和中间那代真实 id
/// 都要能一跳翻译到当前 id。留成链的话 lookup 只翻一跳,落到早已不存在的中间 id 上。
#[test]
fn repeated_rebinds_compress_instead_of_chaining() {
    let broker = PtyBroker::default();
    broker
        .sessions
        .lock()
        .unwrap()
        .insert(-7, dummy_managed(-7));
    broker.try_claim_rebind(-7, 10).unwrap().expect("首次认领");
    broker
        .try_claim_rebind(10, 20)
        .unwrap()
        .expect("/clear 换代");
    assert_eq!(broker.binding(-7), Some(20), "临时 id 直指当前会话");
    assert_eq!(broker.binding(10), Some(20), "上一代真实 id 同样直指当前");
    assert!(broker.snapshot(-7, 0).active);
    assert!(broker.snapshot(10, 0).active);
}

/// 升级第二档：补刀 kill；pid 未知时跳过杀树，不 panic 不阻塞。
/// （master 的 take/drop 无法在此覆盖：测试里造不出 Box<dyn MasterPty>——
/// 其 Error 是 anyhow，非本 crate 直接依赖；该行为与 finalize_exit 同一句式。）
#[test]
fn escalate_stop_kills_without_tree_or_master() {
    let killed = Arc::new(AtomicBool::new(false));
    let managed = managed_with_child(
        7,
        Box::new(NeverDiesChild {
            killed: killed.clone(),
        }),
        None,
    );
    escalate_stop(&managed);
    assert!(killed.load(Ordering::Acquire), "升级档应补刀 kill");
}

fn dummy_managed(session_id: i64) -> Arc<ManagedPty> {
    managed_with_child(session_id, Box::new(DummyChild), None)
}

fn managed_with_child(
    session_id: i64,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    child_pid: Option<u32>,
) -> Arc<ManagedPty> {
    // rx 直接丢弃：假会话没有 writer 线程，写入以 Disconnected 快速失败即可。
    let (input_tx, _) = mpsc::sync_channel(1);
    Arc::new(ManagedPty {
        session_id: AtomicI64::new(session_id),
        master: Mutex::new(None),
        finalized: AtomicBool::new(false),
        input_tx,
        child: Mutex::new(child),
        child_pid,
        conhost_pid: None,
        stop_requested_at: Mutex::new(None),
        backlog: Mutex::new(VecDeque::new()),
        output_end: AtomicU64::new(0),
        subscribers: Mutex::new(Vec::new()),
        probe: ScreenProbe::new(24, 80, "claude".into()),
        last_size: AtomicU32::new(0),
        modes: AtomicU32::new(0),
    })
}

/// 测试用：往 probe 的屏幕状态机喂字节（parser 必存在，即 provider 有规则集）。
fn feed_screen(probe: &ScreenProbe, bytes: &[u8]) {
    probe
        .parser
        .as_ref()
        .expect("该 provider 应有规则集")
        .lock()
        .expect("锁未被毒化")
        .process(bytes);
}

#[test]
fn screen_frame_keeps_text_cursor_and_temporary_binding_together() {
    let broker = PtyBroker::default();
    let managed = dummy_managed(9);
    feed_screen(
        &managed.probe,
        b"\x1b[2J\x1b[4;6HAsk anything\x1b[4;6H\x1b[?25h",
    );
    broker.sessions.lock().unwrap().insert(9, managed.clone());
    broker.attach.bindings.lock().unwrap().insert(-7, 9);
    let first = broker.screen_frame(-7).unwrap();
    assert_eq!((first.cursor_row, first.cursor_col), (3, 5));
    assert!(first.cursor_visible);
    assert!(first.lines[3].contains("Ask anything"));
    assert_eq!(Some(first.clone()), broker.screen_frame(9));
    feed_screen(&managed.probe, b"\x1b[C\x1b[?25l");
    let moved = broker.screen_frame(-7).unwrap();
    assert_eq!(moved.lines, first.lines);
    assert_eq!((moved.cursor_row, moved.cursor_col), (3, 6));
    assert!(!moved.cursor_visible);
    assert!(broker.screen_frame(404).is_none());
    assert!(ScreenProbe::new(24, 80, "unknown-provider".into())
        .frame()
        .is_none());
}

/// 并发实测：PTY reader 高频喂字节的同时检测线程持续扫描。二者共享 parser 锁
/// （reader 每 chunk 写、ticker 每轮持锁导出整屏），锁序错了会死锁、导出期间
/// panic 会毒化锁。这里用真实的两线程竞争跑一遍，并断言结束后状态仍正确——
/// 静态阅读看不出锁竞争下的活性问题。
#[test]
fn screen_probe_survives_concurrent_feed_and_tick() {
    let probe = Arc::new(ScreenProbe::new(24, 80, "claude".into()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(300);
    let feeder = {
        let probe = probe.clone();
        std::thread::spawn(move || {
            let mut written = 0u64;
            while std::time::Instant::now() < deadline {
                // 模拟构建日志刷屏：带 SGR 的整行输出 + 偶尔全屏重绘。
                feed_screen(&probe, b"\x1b[1;32m  compiling\x1b[0m crate v0.1.0\r\n");
                if written.is_multiple_of(50) {
                    feed_screen(&probe, b"\x1b[2J\x1b[H");
                }
                written += 1;
            }
            written
        })
    };
    let ticker = {
        let probe = probe.clone();
        std::thread::spawn(move || {
            let after_grace =
                probe.started_at + DETECT_STARTUP_GRACE + std::time::Duration::from_secs(1);
            let mut ticks = 0u64;
            while std::time::Instant::now() < deadline {
                // 每轮换一个 end 值，强制真正扫描（不走跳扫短路）。
                probe.tick(ticks + 1, after_grace);
                ticks += 1;
            }
            ticks
        })
    };
    let written = feeder.join().expect("feeder 不应 panic（锁未被毒化）");
    let ticks = ticker.join().expect("ticker 不应 panic");
    assert!(written > 0 && ticks > 0, "两侧都应真正跑起来");

    // 竞争结束后 parser 状态仍健康：喂进一屏审批 UI 应被正确判定。
    feed_screen(&probe, b"\x1b[2J\x1b[H");
    feed_screen(
        &probe,
        " Bash command\r\n Do you want to proceed?\r\n \u{276F} 1. Yes\r\n   2. No\r\n".as_bytes(),
    );
    let after_grace = probe.started_at + DETECT_STARTUP_GRACE + std::time::Duration::from_secs(1);
    assert_eq!(
        probe.tick(u64::MAX, after_grace),
        Some(crate::detect::ScreenState::Blocked)
    );
}

/// OSC 9 进度序列要能从**真实字节流**走到判定：vt100 不内建处理 OSC 9，靠
/// `unhandled_osc` 回调取出。漏采它等于让 claude 少一条兜底的 idle 判据
/// （标题未必总带 ✳——用户自设终端标题、或 CLI 版本不写标题时就没了）。
#[test]
fn osc_progress_reaches_the_rules() {
    let probe = ScreenProbe::new(24, 80, "claude".into());
    let after_grace = probe.started_at + DETECT_STARTUP_GRACE + std::time::Duration::from_secs(1);
    // 屏幕上只有历史输出、标题也不带 ✳：唯一的空闲证据就是进度清零。
    feed_screen(&probe, b"\x1b]0;my-terminal\x07");
    feed_screen(&probe, "  cargo build finished\r\n".as_bytes());
    feed_screen(&probe, b"\x1b]9;4;0\x07");
    assert_eq!(
        probe.tick(1, after_grace),
        Some(crate::detect::ScreenState::Idle)
    );
    let snapshot = probe.snapshot().expect("有规则集");
    assert_eq!(snapshot.progress.as_deref(), Some("4;0"));
    assert_eq!(
        crate::detect::evaluate("claude", &snapshot).map(|e| e.rule_id()),
        Some("osc_progress_idle")
    );
}

/// 检测规则按调用方传入的 provider 选，与 argv[0] 无关——包装启动（npx/bunx/fnm
/// shim/中转脚本）下 argv[0] 不是 agent 名，靠它猜会让整条会话静默失去状态检测。
#[test]
fn screen_probe_takes_provider_from_caller_not_argv() {
    let probe = ScreenProbe::new(24, 80, "claude".into());
    let after_grace = probe.started_at + DETECT_STARTUP_GRACE + std::time::Duration::from_secs(1);
    feed_screen(&probe, "\x1b]0;\u{2733} claude\x07".as_bytes());
    assert_eq!(
        probe.tick(1, after_grace),
        Some(crate::detect::ScreenState::Idle),
        "provider 正确时规则命中"
    );
    // provider 是包装器名（argv[0] 猜出来的典型错值）→ 无规则集：连 parser 都不建，
    // reader 热路径一个字节都不解析（门在生产端，不是消费端）。
    let mistaken = ScreenProbe::new(24, 80, "npx".into());
    assert!(
        mistaken.parser.is_none(),
        "无规则集的 provider 不该建 parser——否则每 chunk 白跑一遍 VT 状态机"
    );
    assert_eq!(
        mistaken.tick(
            1,
            mistaken.started_at + DETECT_STARTUP_GRACE + std::time::Duration::from_secs(1)
        ),
        None
    );
}

/// 冷启动路径：题面必须能被**轮询**取回。`interactive-question` 事件在对话窗
/// 冷启动（WebView2 1~2s）时打进虚空且不重放，入表是唯一的兜底。会话须为本
/// broker 托管——外部终端会话的题面留在终端作答，根本不入表。收卡时撤表，
/// 避免轮询把答过的题重新弹成幽灵卡。
#[test]
fn interactive_question_survives_a_missed_event_and_clears_on_dismiss() {
    let broker = PtyBroker::default();
    broker
        .sessions
        .lock()
        .unwrap()
        .insert(11, dummy_managed(11));
    let request = ApprovalRequest {
        session_id: 11,
        request_id: "request-11-ask".into(),
        provider: "claude".into(),
        tool_name: "AskUserQuestion".into(),
        input: r#"{"questions":[{"question":"晚饭吃什么？","options":[{"label":"火锅"}]}]}"#.into(),
        description: None,
        permission_suggestions: vec![],
        pre_tool_use: false,
    };
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server_side, _) = listener.accept().unwrap();
    let token = broker.attach.token.clone();
    broker
        .handle_approval(&token, request.clone(), server_side)
        .unwrap();
    drop(client);

    // 事件即使无人接收，题面仍在表里等轮询来取。
    assert_eq!(broker.interactive_question(11).as_ref(), Some(&request));
    // 别的会话取不到别人的题面。
    assert_eq!(broker.interactive_question(12), None);
    // 收卡撤表后不再返回——否则下一轮轮询会把答过的题重新弹出来。
    broker.clear_interactive_question(11);
    assert_eq!(broker.interactive_question(11), None);
}

/// 表里的题面按 TTL 自净：后端无从得知用户何时在终端答完，卡的去留由前端信号决定，
/// 这里只保证不会无限堆积、也不会在前端 180s 兜底之后还残留着把旧题弹回来。
#[test]
fn stale_interactive_questions_expire() {
    let broker = PtyBroker::default();
    let request = ApprovalRequest {
        session_id: 13,
        request_id: "request-13-ask".into(),
        provider: "claude".into(),
        tool_name: "AskUserQuestion".into(),
        input: "{}".into(),
        description: None,
        permission_suggestions: vec![],
        pre_tool_use: false,
    };
    // 直接以「刚好超过 TTL」的时刻入表，模拟一条没人处理的陈旧题面。
    broker.attach.interactive_questions.lock().unwrap().insert(
        13,
        (request, crate::now_ms() - INTERACTIVE_QUESTION_TTL_MS - 1),
    );
    assert_eq!(broker.interactive_question(13), None, "过期题面不得返回");
    assert!(
        broker
            .attach
            .interactive_questions
            .lock()
            .unwrap()
            .is_empty(),
        "过期条目应被顺手清掉，不留堆积"
    );
}

/// PreToolUse 代答桥（带 pre_tool_use 标记）：GUI 不可用（测试环境无 app handle →
/// ensure_approval_window 必 false）→ 回 pass 交还权限流程，approvals 不滞留；
/// 题面条目留在表里作降级展示卡数据源，且 answerable=false（无 approvals 背书）。
#[test]
fn marked_question_without_gui_passes_and_keeps_a_display_only_entry() {
    let broker = PtyBroker::default();
    broker
        .sessions
        .lock()
        .unwrap()
        .insert(21, dummy_managed(21));
    let mut request = approval_request(21, "AskUserQuestion");
    request.pre_tool_use = true;
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server_side, _) = listener.accept().unwrap();
    let token = broker.attach.token.clone();
    broker
        .handle_approval(&token, request, server_side)
        .unwrap();
    let mut reply = String::new();
    std::io::BufRead::read_line(&mut std::io::BufReader::new(client), &mut reply).unwrap();
    assert_eq!(reply.trim(), "pass");
    assert!(
        broker.attach.approvals.lock().unwrap().is_empty(),
        "降级后请求不得滞留在 approvals 表"
    );
    let dto = broker
        .interactive_question_dto(21)
        .expect("题面应留作展示卡");
    assert!(!dto.answerable, "无 approvals 背书的题面不可作答");
}

/// 挂起中的提问由 resolve 通道代答/交还：`answers:<JSON 映射>` → Answer；
/// `pass` → Pass（去终端作答）。answers 臂只对提问开放，普通审批条目与坏答案拒收。
#[test]
fn resolve_channel_answers_or_passes_a_held_question() {
    let broker = PtyBroker::default();
    let (tx, rx) = mpsc::channel();
    let mut request = approval_request(31, "AskUserQuestion");
    request.pre_tool_use = true;
    request.input = r#"{"questions":[{"question":"晚饭吃什么？"}]}"#.into();
    broker.attach.approvals.lock().unwrap().insert(
        request.request_id.clone(),
        PendingApproval {
            request: request.clone(),
            response: tx,
        },
    );
    // 挂起的提问不得被当成普通审批吐给前端（否则同题双卡）。
    assert!(broker.pending_approval(31).is_none());

    broker
        .resolve_approval_choice(
            31,
            &request.request_id,
            r#"answers:{"晚饭吃什么？":"火锅"}"#,
        )
        .unwrap();
    let decision = rx.recv().unwrap();
    let ApprovalDecision::Answer(answers) = decision else {
        panic!("代答应是 Answer，实际：{decision:?}");
    };
    assert_eq!(answers["晚饭吃什么？"], "火锅");

    // 「去终端作答」：pass 让 reporter 零输出、表单回落终端。
    let (tx, rx) = mpsc::channel();
    broker.attach.approvals.lock().unwrap().insert(
        request.request_id.clone(),
        PendingApproval {
            request: request.clone(),
            response: tx,
        },
    );
    broker
        .resolve_approval_choice(31, &request.request_id, "pass")
        .unwrap();
    assert!(matches!(rx.recv().unwrap(), ApprovalDecision::Pass));

    // 坏答案（非 JSON / 非对象 / 空映射 / 空答案 / 非字符串 / 键对不上题面）一律拒收。
    for bad in [
        "answers:晚饭 → 火锅",
        "answers:[]",
        "answers:{}",
        r#"answers:{"晚饭吃什么？":"  "}"#,
        r#"answers:{"晚饭吃什么？":1}"#,
        r#"answers:{"午饭吃什么？":"火锅"}"#,
        r#"answers:{"晚饭吃什么？":"火锅","":"x"}"#,
    ] {
        let (tx, _rx) = mpsc::channel();
        broker.attach.approvals.lock().unwrap().insert(
            request.request_id.clone(),
            PendingApproval {
                request: request.clone(),
                response: tx,
            },
        );
        assert!(
            broker
                .resolve_approval_choice(31, &request.request_id, bad)
                .is_err(),
            "{bad} 应被拒收"
        );
    }

    // 非提问条目一律拒收。
    let (tx, _rx) = mpsc::channel();
    broker.attach.approvals.lock().unwrap().insert(
        "request-bash-31".into(),
        PendingApproval {
            request: {
                let mut bash = approval_request(31, "Bash");
                bash.request_id = "request-bash-31".into();
                bash
            },
            response: tx,
        },
    );
    assert!(broker
        .resolve_approval_choice(31, "request-bash-31", r#"answers:{"q":"a"}"#)
        .is_err());
    assert!(broker
        .resolve_approval_choice(31, "request-bash-31", "pass")
        .is_err());
}

#[test]
fn runtime_request_reads_and_answers_a_claude_or_kimi_question() {
    for provider in ["claude", "kimi"] {
        let broker = PtyBroker::default();
        let mut request = approval_request(37, "AskUserQuestion");
        request.provider = provider.into();
        request.pre_tool_use = true;
        request.input = r#"{"questions":[{"question":"选哪个？"}]}"#.into();
        broker
            .attach
            .interactive_questions
            .lock()
            .unwrap()
            .insert(37, (request.clone(), crate::now_ms()));
        let initial = broker.runtime_question(37).unwrap();
        assert!(matches!(
            initial.response_capability,
            meowo_protocol::runtime_request::RuntimeResponseCapability::NotResumable { .. }
        ));

        let (tx, rx) = mpsc::channel();
        broker.attach.approvals.lock().unwrap().insert(
            request.request_id.clone(),
            PendingApproval {
                request: request.clone(),
                response: tx,
            },
        );
        let live = broker.runtime_question(37).unwrap();
        assert_eq!(live.provider, provider);
        assert!(matches!(
            live.response_capability,
            meowo_protocol::runtime_request::RuntimeResponseCapability::BrokerLive
        ));
        broker
            .respond_runtime_request(37, &request.request_id, r#"answers:{"选哪个？":"甲"}"#)
            .unwrap();
        assert!(matches!(rx.recv().unwrap(), ApprovalDecision::Answer(_)));
        let resolved = broker.runtime_question(37).unwrap();
        assert_eq!(
            resolved.status,
            meowo_protocol::runtime_request::RuntimeRequestStatus::Resolved
        );
        assert!(matches!(
            resolved.response_capability,
            meowo_protocol::runtime_request::RuntimeResponseCapability::NotResumable { .. }
        ));
        broker
            .respond_runtime_request(37, &request.request_id, r#"answers:{"选哪个？":"甲"}"#)
            .unwrap();
        assert!(rx.try_recv().is_err(), "重复回答不得再次送入 broker");
        assert_eq!(
            broker
                .respond_runtime_request(37, &request.request_id, "pass")
                .unwrap_err(),
            "runtime_request/already_resolved"
        );
    }
}

#[test]
fn runtime_request_reads_and_answers_a_claude_or_codex_approval() {
    for provider in ["claude", "codex"] {
        let broker = PtyBroker::default();
        let (tx, rx) = mpsc::channel();
        let mut request = approval_request(37, "Bash");
        request.provider = provider.into();
        request.request_id = format!("runtime-approval-{provider}");
        request.permission_suggestions = vec![serde_json::json!({"destination":"session"})];
        broker.attach.approvals.lock().unwrap().insert(
            request.request_id.clone(),
            PendingApproval {
                request: request.clone(),
                response: tx,
            },
        );
        let live = broker.runtime_approval(37).unwrap();
        assert_eq!(live.kind, meowo_protocol::runtime_request::RuntimeRequestKind::Approval);
        assert_eq!(live.provider, provider);
        assert!(matches!(
            live.response_capability,
            meowo_protocol::runtime_request::RuntimeResponseCapability::BrokerLive
        ));
        broker
            .respond_runtime_request(37, &request.request_id, "allow_once")
            .unwrap();
        assert!(matches!(rx.recv().unwrap(), ApprovalDecision::Allow));
        let resolved = broker.runtime_approval(37).unwrap();
        assert_eq!(resolved.status, meowo_protocol::runtime_request::RuntimeRequestStatus::Resolved);
        broker
            .respond_runtime_request(37, &request.request_id, "allow_once")
            .unwrap();
        assert!(rx.try_recv().is_err(), "重复审批不得再次送入 broker");
        assert_eq!(
            broker.respond_runtime_request(37, &request.request_id, "deny").unwrap_err(),
            "runtime_request/already_resolved"
        );
        assert_eq!(
            broker.respond_runtime_request(38, &request.request_id, "allow_once").unwrap_err(),
            "runtime_request/session_mismatch"
        );
    }
}

#[test]
fn concurrent_runtime_approval_responses_deliver_only_one_decision() {
    for choices in [("allow_once", "allow_once"), ("allow_once", "deny")] {
        let broker = PtyBroker::default();
        let rx = seed_pending_approval(&broker, 42, "runtime-concurrent-approval");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let handles: Vec<_> = [choices.0, choices.1]
            .into_iter()
            .map(|choice| {
                let broker = broker.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    broker.respond_runtime_request(42, "runtime-concurrent-approval", choice)
                })
            })
            .collect();
        barrier.wait();
        let results: Vec<_> = handles.into_iter().map(|handle| handle.join().unwrap()).collect();
        if choices.0 == choices.1 {
            assert!(results.iter().all(Result::is_ok), "相同回答应幂等成功：{results:?}");
        } else {
            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
            assert!(results.iter().any(|result| matches!(result, Err(error) if error == "runtime_request/already_resolved")));
        }
        assert!(rx.recv().is_ok(), "原生审批只收到一个决策");
        assert!(rx.try_recv().is_err(), "另一个客户端不得再次投递");
    }
}

#[test]
fn answering_one_runtime_request_does_not_clear_a_concurrent_request() {
    let (dir, store, sid) = runtime_pending_store("concurrent");
    let broker = PtyBroker::default();
    let mut approval = approval_request(sid, "Bash");
    approval.request_id = "runtime-clear-approval".into();
    let (tx, rx) = mpsc::channel();
    broker.insert_pending_approval(&approval, tx, &dir).unwrap();
    let mut question = approval_request(sid, "AskUserQuestion");
    question.request_id = "runtime-clear-question".into();
    question.input = r#"{"questions":[{"question":"保留这个问题"}]}"#.into();
    broker.attach.interactive_questions.lock().unwrap().insert(sid, (question.clone(), crate::now_ms()));
    let (tx, _question_rx) = mpsc::channel();
    broker.insert_pending_approval(&question, tx, &dir).unwrap();
    assert_eq!(store.session_pending_review(sid).unwrap().as_deref(), Some("question"));

    assert_eq!(
        broker.respond_runtime_request(sid, &approval.request_id, "allow_once").unwrap(),
        RuntimeResponseOutcome::Delivered
    );
    assert!(matches!(rx.recv().unwrap(), ApprovalDecision::Allow));
    broker.clear_pending_review_if_no_requests(&dir, sid);
    assert_eq!(store.session_pending_review(sid).unwrap().as_deref(), Some("question"), "B 仍在等，A 不得清掉 B");
    assert!(broker.runtime_question(sid).is_some_and(|request| {
        request.request_id == question.request_id
            && matches!(request.response_capability, meowo_protocol::runtime_request::RuntimeResponseCapability::BrokerLive)
    }));
    drop(store);
    let _ = std::fs::remove_file(&dir);
    let _ = std::fs::remove_dir(dir.parent().unwrap());
}

#[test]
fn receipt_replay_does_not_clear_a_later_request() {
    let (dir, store, sid) = runtime_pending_store("replay");
    let broker = PtyBroker::default();
    let mut first = approval_request(sid, "Bash");
    first.request_id = "runtime-replay-first".into();
    let (tx, rx) = mpsc::channel();
    broker.insert_pending_approval(&first, tx, &dir).unwrap();
    assert_eq!(
        broker.respond_runtime_request(sid, &first.request_id, "deny").unwrap(),
        RuntimeResponseOutcome::Delivered
    );
    assert!(matches!(rx.recv().unwrap(), ApprovalDecision::Deny));
    broker.clear_pending_review_if_no_requests(&dir, sid);
    assert_eq!(store.session_pending_review(sid).unwrap(), None);

    let mut later = approval_request(sid, "Bash");
    later.request_id = "runtime-replay-later".into();
    let (tx, _later_rx) = mpsc::channel();
    broker.insert_pending_approval(&later, tx, &dir).unwrap();
    assert_eq!(store.session_pending_review(sid).unwrap().as_deref(), Some("approval"));
    let replay = broker.respond_runtime_request(sid, &first.request_id, "deny").unwrap();
    assert_eq!(replay, RuntimeResponseOutcome::ReceiptReplay);
    if replay == RuntimeResponseOutcome::Delivered {
        broker.clear_pending_review_if_no_requests(&dir, sid);
    }
    assert_eq!(store.session_pending_review(sid).unwrap().as_deref(), Some("approval"), "A 的收据重试不得写 B 的状态");
    assert!(broker.runtime_approval(sid).is_some_and(|request| request.request_id == later.request_id));
    assert!(rx.try_recv().is_err(), "收据重试不得再次投递 A");
    drop(store);
    let _ = std::fs::remove_file(&dir);
    let _ = std::fs::remove_dir(dir.parent().unwrap());
}

#[test]
fn runtime_requests_keep_simultaneous_approval_and_question_independent() {
    let broker = PtyBroker::default();
    let mut question = approval_request(40, "AskUserQuestion");
    question.request_id = "runtime-simultaneous-question".into();
    question.input = r#"{"questions":[{"question":"继续吗？"}]}"#.into();
    broker.attach.interactive_questions.lock().unwrap().insert(
        40,
        (question.clone(), crate::now_ms()),
    );
    let (question_tx, _question_rx) = mpsc::channel();
    broker.attach.approvals.lock().unwrap().insert(
        question.request_id.clone(),
        PendingApproval { request: question, response: question_tx },
    );
    let mut approval = approval_request(40, "Bash");
    approval.request_id = "runtime-simultaneous-approval".into();
    let (approval_tx, approval_rx) = mpsc::channel();
    broker.attach.approvals.lock().unwrap().insert(
        approval.request_id.clone(),
        PendingApproval { request: approval.clone(), response: approval_tx },
    );

    let both = broker.runtime_requests(40);
    assert_eq!(both.len(), 2);
    assert!(both.iter().any(|request| request.kind == meowo_protocol::runtime_request::RuntimeRequestKind::Approval));
    assert!(both.iter().any(|request| request.kind == meowo_protocol::runtime_request::RuntimeRequestKind::Question));
    broker.respond_runtime_request(40, &approval.request_id, "deny").unwrap();
    assert!(matches!(approval_rx.recv().unwrap(), ApprovalDecision::Deny));
    let remaining = broker.runtime_requests(40);
    assert!(remaining.iter().any(|request| {
        request.kind == meowo_protocol::runtime_request::RuntimeRequestKind::Question
            && request.status == meowo_protocol::runtime_request::RuntimeRequestStatus::Pending
    }));
}

#[test]
fn runtime_approval_exposes_terminal_outcomes_without_live_capability() {
    use meowo_protocol::runtime_request::RuntimeRequestStatus;
    for (status, reason) in [
        (RuntimeRequestStatus::Expired, "timed_out"),
        (RuntimeRequestStatus::Resolved, "resolved_elsewhere"),
        (RuntimeRequestStatus::Cancelled, "peer_gone"),
        (RuntimeRequestStatus::Cancelled, "consumer_abandoned"),
    ] {
        let broker = PtyBroker::default();
        let request = approval_request(41, "Bash");
        broker.record_runtime_request_outcome(&request, status, reason, None);
        let terminal = broker.runtime_approval(41).unwrap();
        assert_eq!(terminal.status, status);
        assert!(matches!(
            terminal.response_capability,
            meowo_protocol::runtime_request::RuntimeResponseCapability::NotResumable { reason: actual } if actual == reason
        ));
    }
    assert!(PtyBroker::default().runtime_approval(41).is_none(), "新进程没有旧收据，应返回空快照");
}

#[test]
fn runtime_request_refuses_observation_only_approvals_and_other_questions() {
    let broker = PtyBroker::default();
    let (tx, _rx) = mpsc::channel();
    let mut request = approval_request(38, "Bash");
    request.provider = "kimi".into();
    request.request_id = "runtime-plain-approval".into();
    broker.attach.approvals.lock().unwrap().insert(
        request.request_id.clone(),
        PendingApproval {
            request: request.clone(),
            response: tx,
        },
    );
    assert!(broker
        .respond_runtime_request(38, &request.request_id, "allow_once")
        .is_err());

    let mut other = approval_request(39, "AskUserQuestion");
    other.provider = "codex".into();
    broker
        .attach
        .interactive_questions
        .lock()
        .unwrap()
        .insert(39, (other, crate::now_ms()));
    assert!(broker.runtime_question(39).is_none());
    assert_eq!(
        broker
            .respond_runtime_request(39, "already-gone", "pass")
            .unwrap_err(),
        "runtime_request/not_resumable"
    );
}

/// 挂起中的题面豁免 TTL：代答等待期 300s 长于 TTL 150s，不豁免的话挂起中途
/// 题面就被 retain 清掉、轮询丢卡；approvals 出表后恢复正常过期。
#[test]
fn held_questions_survive_the_ttl_until_settled() {
    let broker = PtyBroker::default();
    let mut request = approval_request(41, "AskUserQuestion");
    request.pre_tool_use = true;
    let stale_at = crate::now_ms() - INTERACTIVE_QUESTION_TTL_MS - 1;
    broker
        .attach
        .interactive_questions
        .lock()
        .unwrap()
        .insert(41, (request.clone(), stale_at));
    let (tx, _rx) = mpsc::channel();
    broker.attach.approvals.lock().unwrap().insert(
        request.request_id.clone(),
        PendingApproval {
            request: request.clone(),
            response: tx,
        },
    );
    let dto = broker
        .interactive_question_dto(41)
        .expect("挂起中的题面不得过期");
    assert!(dto.answerable, "approvals 持有中应可作答");
    // 结算（出表）后，同一条陈旧题面恢复 TTL 语义被清掉。
    broker.attach.approvals.lock().unwrap().clear();
    assert_eq!(
        broker.interactive_question(41),
        None,
        "结算后陈旧题面应过期"
    );
}

/// 召唤策略:目标会话已有消费者→Ready;用户盯着别的会话(有租约+窗口在眼前)→Hold
/// 不夺屏;窗口不在眼前(含「租约在但窗口隐藏」的托盘态)→Summon 切会话召唤。
#[test]
fn summon_policy_never_yanks_a_watched_window() {
    use super::{approval_summon_action, SummonAction};
    assert_eq!(
        approval_summon_action(true, true, true),
        SummonAction::Ready
    );
    // 在眼前与否不影响 Ready:请求靠事件+轮询送达已注册的消费者。
    assert_eq!(
        approval_summon_action(true, true, false),
        SummonAction::Ready
    );
    assert_eq!(
        approval_summon_action(false, true, true),
        SummonAction::Hold
    );
    // 租约还在但窗口收进托盘:徽标没人看得见,必须召唤。
    assert_eq!(
        approval_summon_action(false, true, false),
        SummonAction::Summon
    );
    assert_eq!(
        approval_summon_action(false, false, true),
        SummonAction::Summon
    );
    assert_eq!(
        approval_summon_action(false, false, false),
        SummonAction::Summon
    );
}

/// 代答答案必须与挂起提问的题面一一对应：CC 按问题原文取答案，空键、多余键、漏题
/// 都会让工具拿到对不上的答案。题面本身不可键控（空题面 / 同文重题）时一律拒收——
/// 前端此时不给提交，只留「去终端作答」。
#[test]
fn answers_must_cover_exactly_the_held_questions() {
    let input = r#"{"questions":[{"question":"晚饭吃什么？"},{"question":"配菜选哪些？"}]}"#;
    let map = |json: &str| {
        serde_json::from_str::<serde_json::Value>(json)
            .unwrap()
            .as_object()
            .cloned()
            .unwrap()
    };
    assert!(answers_match_questions(
        input,
        &map(r#"{"晚饭吃什么？":"火锅","配菜选哪些？":"毛肚, 虾滑"}"#)
    ));
    for bad in [
        r#"{"晚饭吃什么？":"火锅"}"#,
        r#"{"晚饭吃什么？":"火锅","配菜选哪些？":"毛肚","多余":"x"}"#,
        r#"{"晚饭吃什么？":"火锅","":"毛肚"}"#,
        r#"{"晚饭吃什么？":"火锅","配菜选哪些？":"  "}"#,
        r#"{"晚饭吃什么？":"火锅","配菜选哪些？":1}"#,
        r#"{}"#,
    ] {
        assert!(!answers_match_questions(input, &map(bad)), "{bad} 应被拒收");
    }
    let one = map(r#"{"晚饭吃什么？":"火锅"}"#);
    for unkeyable in [
        r#"{"questions":[]}"#,
        r#"{"questions":[{"question":"晚饭吃什么？"},{"question":"晚饭吃什么？"}]}"#,
        r#"{"questions":[{"question":"  "}]}"#,
        r#"not json"#,
    ] {
        assert!(
            !answers_match_questions(unkeyable, &one),
            "{unkeyable} 不可键控"
        );
    }
}

fn approval_request(session_id: i64, tool: &str) -> ApprovalRequest {
    ApprovalRequest {
        session_id,
        request_id: format!("request-{session_id}-ask"),
        provider: "claude".into(),
        tool_name: tool.into(),
        description: None,
        input: r#"{"questions":[]}"#.into(),
        permission_suggestions: vec![],
        pre_tool_use: false,
    }
}

fn approval_roundtrip(broker: &PtyBroker, session_id: i64, tool: &str) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server_side, _) = listener.accept().unwrap();
    let token = broker.attach.token.clone();
    broker
        .handle_approval(&token, approval_request(session_id, tool), server_side)
        .unwrap();
    let mut reply = String::new();
    std::io::BufRead::read_line(&mut std::io::BufReader::new(client), &mut reply).unwrap();
    reply.trim().to_string()
}

/// AskUserQuestion 是提问不是权限：必须立即回 allow（TUI 表单零延迟），且**绝不入
/// 审批表**——入了表 GUI 会看到一张没人能消解的幽灵审批卡。对照：普通工具在无 GUI
/// 消费者时按既有语义回 pass 交还终端。会话为本 broker 托管（外部会话另有整段
/// 短路，见下一个测试）。
#[test]
fn ask_user_question_is_auto_allowed_without_queueing() {
    let broker = PtyBroker::default();
    broker.sessions.lock().unwrap().insert(7, dummy_managed(7));
    assert_eq!(approval_roundtrip(&broker, 7, "AskUserQuestion"), "allow");
    assert!(
        broker.attach.approvals.lock().unwrap().is_empty(),
        "自动放行的提问不得滞留在审批表里"
    );
    // 普通工具不受影响：无 GUI 消费者 → 撤回并交还终端（pass）。
    assert_eq!(approval_roundtrip(&broker, 7, "Bash"), "pass");
}

/// 外部终端跑的会话（非本 broker 托管）：提问与审批一律留在终端处理。提问回
/// allow（终端表单零延迟）、审批回 pass（决定权交还 agent 界面），且都不入表——
/// 入了表对话窗就会长出题面卡/审批卡，而用户明确要求外部会话的这些交互只在
/// 终端出现（GUI 抢走审批时，终端前的人反而看不到权限框）。
#[test]
fn externally_run_sessions_keep_interactions_in_the_terminal() {
    let broker = PtyBroker::default();
    assert_eq!(approval_roundtrip(&broker, 99, "AskUserQuestion"), "allow");
    assert_eq!(
        broker.interactive_question(99),
        None,
        "外部会话的题面不得入表——否则对话窗轮询会把卡弹出来"
    );
    assert_eq!(approval_roundtrip(&broker, 99, "Bash"), "pass");
    assert!(
        broker.attach.approvals.lock().unwrap().is_empty(),
        "外部会话的审批不得挂进 broker"
    );
}

/// 审批等待循环的四路信号。场景即实拍 bug：TUI 权限框与 hook 并行竞速，用户在终端里
/// 批准后 GUI 卡片却要盲等满 300s 才消失——断连与 pending_review 清位两路信号都必须
/// 能提前结束等待，且「从未见置位的清除」不得误当结算。
#[test]
fn approval_wait_settles_on_terminal_side_signals() {
    use std::time::{Duration, Instant};
    fn pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    }
    let tick = Duration::from_millis(5);
    let far = || Instant::now() + Duration::from_secs(5);

    // 对端断开（CLI 取消 hook → reporter 进程死）→ PeerGone。
    let (client, server) = pair();
    let (_tx, rx) = mpsc::channel::<ApprovalDecision>();
    drop(client);
    assert_eq!(
        await_approval_outcome(&rx, &server, far(), tick, || Some(true), || false),
        ApprovalWait::PeerGone
    );

    // pending_review 先置位再清除（作答后 PostToolUse 落库）→ 连续两拍确认后结算。
    let (_client2, server2) = pair();
    let (_tx2, rx2) = mpsc::channel::<ApprovalDecision>();
    let mut ticks = 0;
    let outcome = await_approval_outcome(
        &rx2,
        &server2,
        far(),
        tick,
        move || {
            ticks += 1;
            Some(ticks <= 2) // 两拍置位，此后清除
        },
        || false,
    );
    assert_eq!(outcome, ApprovalWait::ResolvedElsewhere);

    // 从未观察到置位的「清除」不是本请求的结算（写库失败 / 迟到的上一工具事件）：
    // 只能走到超时。
    let (_client3, server3) = pair();
    let (_tx3, rx3) = mpsc::channel::<ApprovalDecision>();
    assert_eq!(
        await_approval_outcome(
            &rx3,
            &server3,
            Instant::now() + Duration::from_millis(40),
            tick,
            || Some(false),
            || false
        ),
        ApprovalWait::TimedOut
    );

    // 领卡方(远端手机)中途消失 → Abandoned(回落 TUI,不空等满超时)。
    let (_client5, server5) = pair();
    let (_tx5, rx5) = mpsc::channel::<ApprovalDecision>();
    let mut probes = 0;
    assert_eq!(
        await_approval_outcome(
            &rx5,
            &server5,
            far(),
            tick,
            || Some(false),
            move || {
                probes += 1;
                probes > 2 // 前两拍还在,之后手机租约过期
            }
        ),
        ApprovalWait::Abandoned
    );

    // GUI 决策最高优先：先于任何监视信号被消费。
    let (_client4, server4) = pair();
    let (tx4, rx4) = mpsc::channel::<ApprovalDecision>();
    tx4.send(ApprovalDecision::Allow).unwrap();
    assert_eq!(
        await_approval_outcome(&rx4, &server4, far(), tick, || Some(true), || false),
        ApprovalWait::Decision(ApprovalDecision::Allow)
    );
}

/// 字节流 → 终端仿真 → 规则判定的端到端：真实风格的 ANSI（全屏重绘、光标定位、SGR、
/// OSC 标题、跨 chunk 撕裂的多字节字符）必须被仿真层消化，规则拿到的是干净末屏——
/// 这正是「不能对原始 backlog 做字符串匹配」的验证。
#[test]
fn screen_probe_detects_states_from_raw_ansi() {
    let feed = feed_screen;
    let probe = ScreenProbe::new(24, 80, "claude".into());
    // 启动宽限之外的时间基准。
    let after_grace = probe.started_at + DETECT_STARTUP_GRACE + std::time::Duration::from_secs(1);

    // working：OSC 0 标题带盲文 spinner 帧 + 带 SGR 的滚动输出，
    // 其中「构建中」两个多字节字符故意撕裂在两个 chunk 之间。
    feed(&probe, b"\x1b]0;\xe2\xa0\x8b Reticulating splines\x07");
    let text = "\u{1b}[1;32m构建中\u{1b}[0m cargo build...\r\n".as_bytes();
    let (head, tail) = text.split_at(9); // 切在多字节字符中间
    feed(&probe, head);
    feed(&probe, tail);
    assert_eq!(
        probe.tick(1, after_grace),
        Some(crate::detect::ScreenState::Working)
    );
    // 宽限期内不发布任何状态。
    let fresh = ScreenProbe::new(24, 80, "claude".into());
    assert_eq!(fresh.tick(1, fresh.started_at), None);

    // 审批弹窗：spinner 停转、标题换掉（真实 claude 行为——标题规则 1100 优先级
    // 压过屏幕规则，spinner 挂着时永远判 working），清屏重绘出对话框 → blocked
    // 立即发布（不等确认期）。
    feed(&probe, b"\x1b]0;Claude Code\x07");
    feed(&probe, b"\x1b[2J\x1b[H");
    feed(&probe, " Bash command\r\n".as_bytes());
    feed(&probe, "   cargo test --all\r\n".as_bytes());
    feed(
        &probe,
        "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\r\n".as_bytes(),
    );
    feed(&probe, " Do you want to proceed?\r\n".as_bytes());
    feed(&probe, " \u{276F} 1. Yes\r\n".as_bytes());
    feed(&probe, "   2. No\r\n".as_bytes());
    feed(&probe, " Esc to cancel\r\n".as_bytes());
    assert_eq!(
        probe.tick(2, after_grace),
        Some(crate::detect::ScreenState::Blocked)
    );

    // 放行后回到提示框：标题换 ✳、屏上是 ❯ 提示框 → 可见 idle 直接发布（防抖直通）。
    feed(&probe, "\x1b]0;\u{2733} claude\x07".as_bytes());
    feed(&probe, b"\x1b[2J\x1b[H");
    feed(&probe, "  tool output done\r\n".as_bytes());
    feed(
        &probe,
        "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\r\n".as_bytes(),
    );
    feed(&probe, " \u{276F} \r\n".as_bytes());
    feed(
        &probe,
        "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\r\n".as_bytes(),
    );
    feed(&probe, "  ? for shortcuts\r\n".as_bytes());
    assert_eq!(
        probe.tick(3, after_grace),
        Some(crate::detect::ScreenState::Idle)
    );

    // 无新输出（序号不变）且无待确认：整轮跳过。
    assert_eq!(probe.tick(3, after_grace), None);
}

/// 高优通道只接「运行中 → 空闲/阻塞」的降级：升级转变（多是用户自己触发的）与
/// 无先前发布值的首发都不该绕过合流/节流。
#[test]
fn only_working_to_stopped_transitions_are_downgrades() {
    use crate::detect::ScreenState::{Blocked, Idle, Working};
    assert!(super::is_downgrade(Some(Working), Idle));
    assert!(super::is_downgrade(Some(Working), Blocked));
    assert!(!super::is_downgrade(Some(Idle), Working), "升级不高优");
    assert!(!super::is_downgrade(Some(Blocked), Working), "升级不高优");
    assert!(!super::is_downgrade(None, Idle), "首次发布不算降级");
    assert!(
        !super::is_downgrade(Some(Idle), Blocked),
        "非运行中出发不算"
    );
}

/// 三态判定与订阅表严格同步：None 误判会新开重复窗口，Legacy/Pid 误判会不开新视图。
#[test]
fn external_viewer_state_follows_the_subscriber_table() {
    let broker = PtyBroker::default();
    assert_eq!(
        broker.external_viewer(7),
        ExternalViewer::None,
        "无此会话必须判 None"
    );
    let managed = dummy_managed(7);
    broker.sessions.lock().unwrap().insert(7, managed.clone());
    assert_eq!(
        broker.external_viewer(7),
        ExternalViewer::None,
        "无订阅必须判 None"
    );
    let (tx1, _rx1) = mpsc::channel::<Vec<u8>>();
    managed.subscribers.lock().unwrap().push(AttachSubscriber {
        id: 1,
        pid: None,
        tx: tx1,
    });
    assert_eq!(
        broker.external_viewer(7),
        ExternalViewer::Legacy,
        "在线但未上报 pid（旧 reporter）→ Legacy"
    );
    let (tx2, _rx2) = mpsc::channel::<Vec<u8>>();
    managed.subscribers.lock().unwrap().push(AttachSubscriber {
        id: 2,
        pid: Some(4242),
        tx: tx2,
    });
    let (tx3, _rx3) = mpsc::channel::<Vec<u8>>();
    managed.subscribers.lock().unwrap().push(AttachSubscriber {
        id: 3,
        pid: Some(5353),
        tx: tx3,
    });
    assert_eq!(
        broker.external_viewer(7),
        ExternalViewer::Pid(5353),
        "取最近注册的上报者"
    );
    managed.subscribers.lock().unwrap().clear();
    assert_eq!(
        broker.external_viewer(7),
        ExternalViewer::None,
        "订阅摘除后必须回落 None"
    );
}

#[test]
fn starting_placeholder_suppresses_duplicate_starts_until_cleared() {
    let broker = PtyBroker::default();
    assert!(broker.begin_start(7).unwrap());
    // 占位期间第二个 start 必须被判为重复（contains 检查与占位插入在同一锁程内原子完成）。
    assert!(!broker.begin_start(7).unwrap());
    broker.end_start(7);
    // 启动失败清掉占位后，重试必须能重新登记。
    assert!(broker.begin_start(7).unwrap());
    broker.end_start(7);
    // 已在运行的会话同样压住新占位（走 sessions.contains 分支）。
    broker.sessions.lock().unwrap().insert(8, dummy_managed(8));
    assert!(!broker.begin_start(8).unwrap());
}

/// 判重提前返回同样要摘掉 completed 残留：start 按 Ok 收敛后，调用方的秒退探测只凭
/// completed 判断「起没起来」，上一代退出的定格快照会被误报成本次启动秒退。
#[test]
fn duplicate_start_clears_the_stale_completed_snapshot() {
    let broker = PtyBroker::default();
    let stale = || CompletedPty {
        data: b"old".to_vec(),
        start_offset: 0,
        end_offset: 3,
        code: Some(1),
        user_stopped: false,
        seq: 0,
    };
    // sessions.contains 分支（会话仍在运行，completed 里躺着上一代的退出快照）。
    broker.sessions.lock().unwrap().insert(7, dummy_managed(7));
    broker.completed.lock().unwrap().insert(7, stale());
    assert!(!broker.begin_start(7).unwrap());
    assert!(broker.exit_info(7).is_none(), "残留快照必须被摘掉");
    // starting 占位分支（另一个 start 正在锁外 spawn；finalize 先插 completed 再摘
    // sessions 的间隙里，completed 也可能出现当前占位之前的写入）。
    broker.begin_start(8).unwrap();
    broker.completed.lock().unwrap().insert(8, stale());
    assert!(!broker.begin_start(8).unwrap());
    assert!(broker.exit_info(8).is_none());
    broker.end_start(8);
}

#[test]
fn a_starting_session_reads_as_not_yet_running() {
    let broker = PtyBroker::default();
    broker.begin_start(7).unwrap();
    // snapshot：inactive 且非 exited 的空帧——与启动前一致，前端按既有「未连接」路径
    // 处理，绝不会把启动中的会话误判成已退出（completed 已在登记占位时摘掉）。
    let snapshot = broker.snapshot(7, 0);
    assert!(!snapshot.active);
    assert!(!snapshot.exited);
    assert!(snapshot.data.is_empty());
    // write/resize/stop 快速失败，不在启动中的会话上排队等待。
    assert!(broker.write(7, b"x").is_err());
    assert!(broker.resize(7, 80, 24).is_err());
    assert!(broker.stop(7).is_err());
    assert!(!broker.is_managed(7));
    broker.end_start(7);
}

#[test]
fn claim_waits_for_the_inflight_start_to_register() {
    let broker = PtyBroker::default();
    broker
        .attach
        .pending
        .lock()
        .unwrap()
        .insert("launch".into(), -5);
    broker.begin_start(-5).unwrap();
    let token = broker.attach.token.clone();

    // 模拟 start 的锁外段：spawn 完成后登记入表、再摘占位。
    let registrar = broker.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        registrar
            .sessions
            .lock()
            .unwrap()
            .insert(-5, dummy_managed(-5));
        registrar.end_start(-5);
    });

    // claim 不重试（reporter 一次性）：登记未落的窗口里必须等，不能按「已结束」错杀。
    broker.handle_claim(&token, "launch", 9, None).unwrap();
    handle.join().unwrap();
    let sessions = broker.sessions.lock().unwrap();
    assert!(sessions.contains_key(&9));
    assert!(!sessions.contains_key(&-5));
    drop(sessions);
    assert_eq!(broker.binding(-5), Some(9));
    // token 不随认领消费：/clear 换代要带着它重认领，存续到 PTY 退出路径清理。
    assert_eq!(
        broker.attach.pending.lock().unwrap().get("launch"),
        Some(&-5)
    );
}

/// 「正在启动」占位卡元数据的生命周期：start_pending 登记（该路径由集成行为覆盖，
/// 这里直接摆好登记后的状态）→ 首次 claim 认领时摘除——真实会话行已由 hook 落库，
/// 看板下一次刷新用真卡顶替，占位与真卡不同屏并存。
#[test]
fn claim_retires_the_starting_placeholder() {
    let broker = PtyBroker::default();
    broker
        .attach
        .pending
        .lock()
        .unwrap()
        .insert("launch".into(), -5);
    broker.attach.pending_launches.lock().unwrap().insert(
        -5,
        PendingLaunch {
            provider: "codex".into(),
            cwd: Some("/tmp/x".into()),
            started_at: 1,
        },
    );
    broker
        .sessions
        .lock()
        .unwrap()
        .insert(-5, dummy_managed(-5));
    assert_eq!(broker.pending_launches().len(), 1);
    let token = broker.attach.token.clone();
    broker.handle_claim(&token, "launch", 9, None).unwrap();
    assert!(
        broker.pending_launches().is_empty(),
        "认领后占位卡必须撤下（真实会话行已落库）"
    );
}

#[test]
fn claim_after_a_failed_start_keeps_its_token() {
    let broker = PtyBroker::default();
    broker
        .attach
        .pending
        .lock()
        .unwrap()
        .insert("launch".into(), -5);
    broker.begin_start(-5).unwrap();
    broker.end_start(-5); // 启动失败：清占位、不入表
    let token = broker.attach.token.clone();
    assert!(broker.handle_claim(&token, "launch", 9, None).is_err());
    // token 不消费：早到/迟到的 claim 都不得断送下一次认领（原有语义保持）。
    assert_eq!(
        broker.attach.pending.lock().unwrap().get("launch"),
        Some(&-5)
    );
}

#[test]
fn registration_after_shutdown_is_rejected() {
    let broker = PtyBroker::default();
    broker.shutdown();
    // drain 结束后完成的 spawn 必须登记失败（调用方当场收尾），不能混进表里孤儿化。
    assert!(broker.register_spawned(7, &dummy_managed(7)).is_err());
    assert!(!broker.sessions.lock().unwrap().contains_key(&7));
}

#[test]
fn completed_snapshot_preserves_output_offsets() {
    let broker = PtyBroker::default();
    broker.completed.lock().unwrap().insert(
        42,
        CompletedPty {
            data: b"tail".to_vec(),
            start_offset: 96,
            end_offset: 100,
            code: Some(0),
            user_stopped: false,
            seq: 0,
        },
    );

    let snapshot = broker.snapshot(42, 0);
    assert!(!snapshot.active);
    assert!(snapshot.exited);
    assert_eq!(
        snapshot.data,
        base64::engine::general_purpose::STANDARD.encode(b"tail")
    );
    assert_eq!(snapshot.start_offset, 96);
    assert_eq!(snapshot.end_offset, 100);
    assert_eq!(snapshot.exit_code, Some(0));
}

/// 启动探测代答的核心契约:首个可见字节之前的 `ESC[6n` 答、且每个只答一次,同时
/// **从流中摘除**——下游(attach DsrFilter、GUI xterm)看不到已答的查询,就不可能
/// 再答第二遍;其余字节一个不动。画过首帧后永久停机、原样透传(实时查询交还给
/// 活着的视图,xterm 以真实位置应答)。这是「新建会话卡初始化」与「恢复会话
/// composer 多个 C」两个 bug 的共同守卫。
#[test]
fn startup_probe_scanner_answers_and_strips_prepaint_queries_once() {
    let mut scanner = StartupProbeScanner::new();
    // claude 冷启动的真实形态:查光标、藏光标、清屏——探测被摘除,其余原样保留。
    assert_eq!(
        scanner.feed(b"\x1b[6n\x1b[?25l\x1b[2J"),
        (b"\x1b[?25l\x1b[2J".to_vec(), 1)
    );
    // 跨 chunk 撕裂的查询:前缀暂存不外流,补齐后计数,字节不重复不丢失。
    assert_eq!(scanner.feed(b"\x1b["), (vec![], 0));
    assert_eq!(scanner.feed(b"6n"), (vec![], 1));
    // 撕裂后排除探测的序列:暂存的前缀原样冲出。
    assert_eq!(scanner.feed(b"\x1b[6"), (vec![], 0));
    assert_eq!(scanner.feed(b"m"), (b"\x1b[6m".to_vec(), 0));
    // OSC 标题文本与空白控制字节都不算画面,探测期未结束。
    assert_eq!(
        scanner.feed(b"\x1b]0;title\x07 \r\n\x1b[6n"),
        (b"\x1b]0;title\x07 \r\n".to_vec(), 1)
    );
    // 首个可见字节后停机:此后的查询原样透传,由活着的视图应答。
    assert_eq!(scanner.feed(b"W"), (b"W".to_vec(), 0));
    assert_eq!(scanner.feed(b"\x1b[6n"), (b"\x1b[6n".to_vec(), 0));
}

#[test]
fn startup_probe_scanner_ignores_lookalikes_and_stops_at_paint() {
    let mut scanner = StartupProbeScanner::new();
    // 参数不是恰好 "6" 的 CSI-n 都不是光标探测(DSR 状态查询/DECXCPR 变体等),
    // 一律不答、原样保留。
    let lookalikes: &[u8] = b"\x1b[16n\x1b[?6n\x1b[6;1n\x1b[0n";
    assert_eq!(scanner.feed(lookalikes), (lookalikes.to_vec(), 0));
    // 同一 chunk 里查询在可见字节之前:计入并摘除,可见字节起原样透传(含后续查询)。
    let mut scanner = StartupProbeScanner::new();
    assert_eq!(
        scanner.feed(b"\x1b[6nhello\x1b[6n"),
        (b"hello\x1b[6n".to_vec(), 1)
    );
}

/// charset 指定(`ESC ( B`)与 DCS/APC 等字符串序列的负载不是画面:此前 ESC 的
/// 中间字节/串负载被误判为可见字节,扫描器提前停机——之后的探测没人答,TUI 卡在
/// 首帧前(25s 兜底后黑屏)。
#[test]
fn startup_probe_scanner_survives_charset_and_string_sequences() {
    let mut scanner = StartupProbeScanner::new();
    assert_eq!(scanner.feed(b"\x1b(B\x1b)0"), (b"\x1b(B\x1b)0".to_vec(), 0));
    assert_eq!(
        scanner.feed(b"\x1bP+q544e\x1b\\"),
        (b"\x1bP+q544e\x1b\\".to_vec(), 0)
    );
    assert_eq!(
        scanner.feed(b"\x1b_Ga=q\x1b\\"),
        (b"\x1b_Ga=q\x1b\\".to_vec(), 0)
    );
    // 这些序列之后的探测仍有人答。
    assert_eq!(scanner.feed(b"\x1b[6n"), (vec![], 1));
    // 真正的可见字节才停机。
    assert_eq!(scanner.feed(b"x"), (b"x".to_vec(), 0));
    assert!(scanner.painted());
}

/// attach 回放的展示流要滤掉历史查询(客户端 DsrFilter 会代答流里的每个查询,
/// 历史查询的迟到应答会打进 agent 输入框);形似而非与残缺前缀原样保留。
#[test]
fn strip_dsr_queries_removes_only_complete_queries() {
    assert_eq!(strip_dsr_queries(b"ab\x1b[6ncd"), b"abcd");
    assert_eq!(strip_dsr_queries(b"\x1b[6n\x1b[6n"), b"");
    assert_eq!(strip_dsr_queries(b"\x1b[16n\x1b[?6n"), b"\x1b[16n\x1b[?6n");
    // backlog 裁剪边界留下的残缺前缀:不匹配,不误删。
    assert_eq!(strip_dsr_queries(b"\x1b[6"), b"\x1b[6");
}

/// since 的三种边界：命中中段只回增量、追平后回空、落在已裁剪区之前退化为全量。
/// 这是「轮询不再每次传整个 backlog」的核心契约。
#[test]
fn snapshot_returns_only_bytes_after_since() {
    let decode = |s: &str| base64::engine::general_purpose::STANDARD.decode(s).unwrap();
    let broker = PtyBroker::default();
    broker.completed.lock().unwrap().insert(
        7,
        CompletedPty {
            data: b"abcdef".to_vec(),
            start_offset: 100,
            end_offset: 106,
            code: Some(0),
            user_stopped: false,
            seq: 0,
        },
    );

    // since 在区间中段：只回它之后的字节，start_offset 前移到 since。
    let mid = broker.snapshot(7, 103);
    assert_eq!(decode(&mid.data), b"def");
    assert_eq!((mid.start_offset, mid.end_offset), (103, 106));

    // since 已追平末尾：空增量，稳态轮询的常态——此前每次都要传整份。
    let caught_up = broker.snapshot(7, 106);
    assert!(decode(&caught_up.data).is_empty());
    assert_eq!((caught_up.start_offset, caught_up.end_offset), (106, 106));

    // since 早于 backlog 起点（那段已被裁剪）：退化为全量，不能假装数据还在。
    let stale = broker.snapshot(7, 40);
    assert_eq!(decode(&stale.data), b"abcdef");
    assert_eq!(stale.start_offset, 100);

    // since 超前于末尾（会话被重置）：不得 panic 或越界，回空让前端按 offset 重新对齐。
    let ahead = broker.snapshot(7, 999);
    assert!(decode(&ahead.data).is_empty());
}

#[test]
fn attach_server_rejects_bad_token_and_tokens_are_unique() {
    let first = PtyBroker::default();
    let second = PtyBroker::default();
    assert_eq!(first.attach.token.len(), 64);
    assert_ne!(first.attach.token, second.attach.token);
    first.start_attach_server().unwrap();
    let endpoint = first.attach.endpoint.lock().unwrap().unwrap();
    let mut stream = TcpStream::connect(endpoint).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .unwrap();
    writeln!(stream, "MEOWO1 wrong 1 80 24 nonce1234").unwrap();
    let mut byte = [0u8; 1];
    assert_eq!(stream.read(&mut byte).unwrap(), 0);

    first
        .attach
        .pending
        .lock()
        .unwrap()
        .insert("launch-token".into(), -7);
    assert!(first
        .handle_claim("wrong", "launch-token", 9, None)
        .is_err());
    assert_eq!(
        first.attach.pending.lock().unwrap().get("launch-token"),
        Some(&-7)
    );

    first.attach.bindings.lock().unwrap().insert(-7, 9);
    // binding 是只读非消费：对话窗口会重复轮询，重挂后还会再读——一次性消费会让
    // 后到的读方永远等不到真实 id。绑定表由 PTY 退出路径清理（reader 线程 retain）。
    assert_eq!(first.binding(-7), Some(9));
    assert_eq!(first.binding(-7), Some(9));
}

/// /clear 换代的 claim 状态机（不触 DB 的分支）：同 id 重放幂等成功、launch token
/// 不被消费；旧会话已不在 sessions（PTY 已亡）时换代报错且不动绑定表。
#[test]
fn reclaim_is_idempotent_on_replay_and_rejects_dead_session() {
    let broker = PtyBroker::default();
    let token = broker.attach.token.clone();
    broker
        .attach
        .pending
        .lock()
        .unwrap()
        .insert("launch-token".into(), -7);
    broker.attach.bindings.lock().unwrap().insert(-7, 9);

    // compact 等场景会对同一真实 id 重发 SessionStart：重放必须幂等成功。
    broker
        .handle_claim(&token, "launch-token", 9, None)
        .unwrap();
    // token 存续（/clear 换代还要靠它重认领），不随认领消费。
    assert_eq!(
        broker.attach.pending.lock().unwrap().get("launch-token"),
        Some(&-7)
    );

    // 换代到新 id，但旧会话已不在 sessions（PTY 已退出）：报错，且绑定保持原值。
    assert!(broker
        .handle_claim(&token, "launch-token", 10, None)
        .is_err());
    assert_eq!(broker.binding(-7), Some(9));
}

/// 认领的会话不在本库里（会话内跑的测试 / 探针用临时库建的会话，id 继承外层 PTY 凭据
/// 来认领）：首次认领和换代都拒绝，原绑定不动（实拍 1537 被换代到不存在的会话 1）。
#[test]
fn claim_for_a_session_missing_from_the_db_is_rejected() {
    const GHOST: i64 = 424_242;
    MISSING_FOR_TEST.lock().unwrap().push(GHOST);
    let broker = PtyBroker::default();
    let token = broker.attach.token.clone();
    broker
        .attach
        .pending
        .lock()
        .unwrap()
        .insert("launch-ghost".into(), -17);
    // 首次认领：拒绝，不建绑定。
    assert!(broker
        .handle_claim(&token, "launch-ghost", GHOST, None)
        .is_err());
    assert_eq!(broker.binding(-17), None);
    // 换代：拒绝，原绑定保持。
    broker.attach.bindings.lock().unwrap().insert(-17, 9);
    let err = broker
        .handle_claim(&token, "launch-ghost", GHOST, None)
        .unwrap_err();
    assert!(err.contains("不在本库中"), "{err}");
    assert_eq!(broker.binding(-17), Some(9));
    MISSING_FOR_TEST.lock().unwrap().retain(|id| *id != GHOST);
}

/// 换代守卫：会话内 Bash 起的嵌套 agent（`claude -p` 探针等）继承 PTY 环境变量后，
/// SessionStart 会带同一 launch token、不同 pid 来认领——旧会话进程还活着时必须拒绝，
/// 否则真会话被 supersede 到探针行（卡片顶探针标题、子任务全失联，2026-08-18 实拍）。
#[test]
fn nested_agent_claim_is_rejected_only_while_the_old_session_lives() {
    // 同 pid = /clear 原地换代，放行（不该走到判活）。
    assert!(!nested_claim_hijacks_reclaim(60072, Some(60072), |_| {
        panic!("同 pid 不该判活")
    }));
    // 异 pid + 旧进程活着 = 嵌套误认领，拒绝。
    assert!(nested_claim_hijacks_reclaim(62732, Some(60072), |_| true));
    // 异 pid + 旧进程已死 = 同终端手动重启 agent，PTY 真换主，放行。
    assert!(!nested_claim_hijacks_reclaim(62732, Some(60072), |_| false));
    // 旧行没记 pid：无从比对，放行维持旧行为。
    assert!(!nested_claim_hijacks_reclaim(62732, None, |_| {
        panic!("没有旧 pid 不该判活")
    }));
}

/// 换代守卫：claude daemon 派生的后台会话不在 PTY 进程树内，不许抢托管终端。
/// 进程链取自 2026-10-27 实拍：meowo(27888) → claude(68524) 是托管会话；
/// WmiPrvSE(8100) → daemon(66548) → bg-pty-host(21396) → claude(64980) 是抢走 PTY 的后台会话。
#[test]
fn claim_from_outside_the_pty_tree_is_rejected() {
    let parent_of: HashMap<u32, u32> = [
        (68524, 27888),
        (66548, 8100),
        (21396, 66548),
        (64980, 21396),
        // 合法后代：托管会话 Bash 里起的子进程。
        (70001, 68524),
        (70002, 70001),
    ]
    .into_iter()
    .collect();
    // PTY 子进程本身认领 = /clear 原地换代。
    assert!(!claim_outside_pty_tree(68524, 68524, &parent_of));
    // 树内后代：交给后面的嵌套守卫裁决，这一层放行。
    assert!(!claim_outside_pty_tree(70002, 68524, &parent_of));
    // daemon 派生的后台会话：树外，拒绝。
    assert!(claim_outside_pty_tree(64980, 68524, &parent_of));
    // 进程表里查不到的 pid（父链断开）也按树外处理。
    assert!(claim_outside_pty_tree(99999, 68524, &parent_of));
    // pid 复用造成的伪环不能死循环。
    let cyclic: HashMap<u32, u32> = [(1, 2), (2, 1)].into_iter().collect();
    assert!(claim_outside_pty_tree(1, 68524, &cyclic));
}

/// 凭据分级：注入 agent 环境的 scoped reporter token 只许认领自己那次 launch、
/// 只许为自己（签发时绑定的）会话递审批；attach（PTY 帧读写）、跨会话 claim/审批
/// 一律拒绝。full token 行为不变（由既有测试覆盖）。未登记 token default-deny。
#[test]
fn token_level_resolution_is_default_deny() {
    let broker = PtyBroker::default();
    let full = broker.attach.token.clone();
    assert_eq!(broker.token_level(&full), Some(TokenLevel::Full));
    broker
        .attach
        .scoped_tokens
        .lock()
        .unwrap()
        .insert("scoped-a".into(), -7);
    assert_eq!(broker.token_level("scoped-a"), Some(TokenLevel::Scoped(-7)));
    assert_eq!(broker.token_level("unknown"), None);

    // 归属判定：resume/重启路径 owner 即真实 id 直接相等；start_pending 的临时负 id
    // 按当前 bindings 比对（claim 落绑、/clear 换代换绑）。
    broker.attach.bindings.lock().unwrap().insert(-7, 9);
    assert!(broker.scoped_owns_session(-7, 9));
    assert!(broker.scoped_owns_session(9, 9));
    assert!(!broker.scoped_owns_session(-7, 10));
    assert!(!broker.scoped_owns_session(-8, 9));
}

/// scoped token 的 claim 会话绑定：只能认领签发时绑定的那次 launch。
/// 拿 A 会话的环境变量认领 B 会话的 launch token（嵌套 agent/恶意后代）按认证失败拒绝。
#[test]
fn scoped_claim_is_bound_to_its_own_launch() {
    let broker = PtyBroker::default();
    {
        let mut pending = broker.attach.pending.lock().unwrap();
        pending.insert("launch-a".into(), -7);
        pending.insert("launch-b".into(), -8);
    }
    {
        let mut tokens = broker.attach.scoped_tokens.lock().unwrap();
        tokens.insert("scoped-a".into(), -7);
        tokens.insert("scoped-b".into(), -8);
    }
    // 同 id 重放分支（bindings 已有 -7→9）不触 DB，给出干净的 Ok 路径。
    broker.attach.bindings.lock().unwrap().insert(-7, 9);
    assert!(broker.handle_claim("scoped-a", "launch-a", 9, None).is_ok());
    // 跨 launch 认领与未登记 token：一律认证失败。
    assert_eq!(
        broker.handle_claim("scoped-a", "launch-b", 9, None),
        Err("PTY claim 认证失败".to_string())
    );
    assert_eq!(
        broker.handle_claim("scoped-b", "launch-a", 9, None),
        Err("PTY claim 认证失败".to_string())
    );
    assert_eq!(
        broker.handle_claim("wrong", "launch-a", 9, None),
        Err("PTY claim 认证失败".to_string())
    );
    // full token 不受 launch 绑定限制（既有语义不变）。
    let full = broker.attach.token.clone();
    broker.attach.bindings.lock().unwrap().insert(-8, 11);
    assert!(broker.handle_claim(&full, "launch-b", 11, None).is_ok());
}

/// scoped token 的审批会话绑定：只许为自己会话提交；跨会话提交与未登记 token
/// 按认证失败拒绝（reporter 侧拿不到决策 → 回落 agent 自己的 TUI，不静默放行）。
#[test]
fn scoped_approval_is_bound_to_its_own_session() {
    let broker = PtyBroker::default();
    broker
        .attach
        .scoped_tokens
        .lock()
        .unwrap()
        .insert("scoped-a".into(), -7);
    broker.attach.bindings.lock().unwrap().insert(-7, 9);

    // 会话不在 sessions（外部会话语义）→ 自动 pass，一条 round-trip 即返回，
    // 足够区分「认证已过」与「认证被拒」。
    let try_approval = |token: &str, session_id: i64| {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server_side, _) = listener.accept().unwrap();
        let result =
            broker.handle_approval(token, approval_request(session_id, "Bash"), server_side);
        drop(client);
        result
    };
    // 自己的会话（经 bindings 归属）：放行。
    assert!(try_approval("scoped-a", 9).is_ok());
    // 别的会话：跨会话提交，拒绝。
    assert_eq!(
        try_approval("scoped-a", 10),
        Err("审批通道认证失败".to_string())
    );
    // 未登记 token：default-deny。
    assert_eq!(
        try_approval("wrong", 9),
        Err("审批通道认证失败".to_string())
    );
    // 吊销后（PTY 退出清理的语义）同一张 token 立即失效。
    broker.revoke_scoped_token("scoped-a");
    assert_eq!(
        try_approval("scoped-a", 9),
        Err("审批通道认证失败".to_string())
    );
}

/// scoped token 不得通过 attach 认证：attach = PTY 帧读写（完全接管权），只认 full token。
#[test]
fn scoped_token_cannot_attach() {
    let broker = PtyBroker::default();
    broker
        .attach
        .scoped_tokens
        .lock()
        .unwrap()
        .insert("scoped-a".into(), -7);
    broker.start_attach_server().unwrap();
    let endpoint = broker.attach.endpoint.lock().unwrap().unwrap();
    let mut stream = TcpStream::connect(endpoint).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .unwrap();
    writeln!(stream, "MEOWO1 scoped-a 1 80 24 nonce1234").unwrap();
    let mut byte = [0u8; 1];
    assert_eq!(
        stream.read(&mut byte).unwrap(),
        0,
        "scoped token 不得通过 attach 认证"
    );
}

#[test]
fn resolving_wrong_session_does_not_consume_approval() {
    let broker = PtyBroker::default();
    let (tx, rx) = mpsc::channel();
    let request = ApprovalRequest {
        session_id: 7,
        request_id: "request-123".into(),
        provider: "codex".into(),
        tool_name: "Bash".into(),
        description: Some("run tests".into()),
        input: "{}".into(),
        permission_suggestions: vec![],
        pre_tool_use: false,
    };
    broker.attach.approvals.lock().unwrap().insert(
        request.request_id.clone(),
        PendingApproval {
            request: request.clone(),
            response: tx,
        },
    );
    assert_eq!(broker.pending_approval(7).unwrap().tool_name, "Bash");
    assert!(broker
        .resolve_approval(8, "request-123", ApprovalDecision::Allow)
        .is_err());
    broker
        .resolve_approval(7, "request-123", ApprovalDecision::Deny)
        .unwrap();
    assert!(matches!(rx.recv().unwrap(), ApprovalDecision::Deny));
    assert!(broker.pending_approval(7).is_none());
}

#[test]
fn resolving_agent_suggestion_returns_its_original_permission_update() {
    let broker = PtyBroker::default();
    let (tx, rx) = mpsc::channel();
    let suggestion = serde_json::json!({
        "type": "addRules",
        "behavior": "allow",
        "destination": "localSettings",
        "rules": [{ "toolName": "Bash", "ruleContent": "cargo test" }],
    });
    broker.attach.approvals.lock().unwrap().insert(
        "request-options".into(),
        PendingApproval {
            request: ApprovalRequest {
                session_id: 7,
                request_id: "request-options".into(),
                provider: "claude".into(),
                tool_name: "Bash".into(),
                description: None,
                input: r#"{"command":"cargo test"}"#.into(),
                permission_suggestions: vec![suggestion.clone()],
                pre_tool_use: false,
            },
            response: tx,
        },
    );

    broker
        .respond_runtime_request(7, "request-options", "suggestion:0")
        .unwrap();
    assert_eq!(
        rx.recv().unwrap(),
        ApprovalDecision::AllowWithPermissions(vec![suggestion])
    );
}

/// 附理由拒绝：`deny:<理由>` → DenyWith（理由去首尾空白）；空理由退回普通 Deny。
#[test]
fn deny_choice_carries_reason_to_model() {
    let broker = PtyBroker::default();
    let rx = seed_pending_approval(&broker, 9, "request-deny-reason");
    broker
        .respond_runtime_request(9, "request-deny-reason", "deny:  别动生产配置 ")
        .unwrap();
    assert_eq!(
        rx.recv().unwrap(),
        ApprovalDecision::DenyWith("别动生产配置".into())
    );
    let rx = seed_pending_approval(&broker, 9, "request-deny-empty");
    broker
        .respond_runtime_request(9, "request-deny-empty", "deny:   ")
        .unwrap();
    assert_eq!(rx.recv().unwrap(), ApprovalDecision::Deny);
}

/// RuntimeRequest 跨请求清理用独立临时库，绝不接触开发实例共享的 ~/.meowo。
fn runtime_pending_store(name: &str) -> (std::path::PathBuf, meowo_store::Store, i64) {
    let dir = std::env::temp_dir().join(format!(
        "meowo-runtime-request-{name}-{}-{}",
        std::process::id(),
        crate::now_ms()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("board.db");
    let store = meowo_store::Store::open(&db).unwrap();
    let project = store.upsert_project_by_root("C:/repo", "repo", 100).unwrap();
    let (session_id, _) = store.start_session(project, "runtime-request", 100).unwrap();
    (db, store, session_id)
}

/// 测试固件：向审批表塞一条挂起请求，返回决策接收端。
fn seed_pending_approval(
    broker: &PtyBroker,
    session_id: i64,
    request_id: &str,
) -> mpsc::Receiver<ApprovalDecision> {
    let (tx, rx) = mpsc::channel();
    broker.attach.approvals.lock().unwrap().insert(
        request_id.into(),
        PendingApproval {
            request: ApprovalRequest {
                session_id,
                request_id: request_id.into(),
                provider: "codex".into(),
                tool_name: "Bash".into(),
                description: None,
                input: "{}".into(),
                permission_suggestions: vec![],
                pre_tool_use: false,
            },
            response: tx,
        },
    );
    rx
}

/// 测试固件：把远端租约人为推回过去，模拟 TTL 过期/临期。
fn age_consumer_lease(broker: &PtyBroker, consumer_id: &str, by_ms: i64) {
    broker
        .attach
        .approval_consumers
        .lock()
        .unwrap()
        .get_mut(consumer_id)
        .unwrap()
        .seen_ms -= by_ms;
}

#[test]
fn closing_approval_consumer_passes_every_pending_request() {
    // 没有任何消费者时关窗：全部挂起审批交还 TUI（release 的退化形态 = 旧全清语义）。
    let broker = PtyBroker::default();
    let receivers = vec![
        seed_pending_approval(&broker, 7, "request-1"),
        seed_pending_approval(&broker, 7, "request-2"),
    ];
    broker.release_desktop_consumers();
    assert!(broker.attach.approvals.lock().unwrap().is_empty());
    assert!(receivers
        .into_iter()
        .all(|rx| matches!(rx.recv().unwrap(), ApprovalDecision::Pass)));
}

#[test]
fn approval_consumer_lease_is_session_scoped_and_releases_on_last_unregister() {
    let broker = PtyBroker::default();
    broker
        .register_approval_consumer(7, "consumer-a".into())
        .unwrap();
    broker
        .register_approval_consumer(7, "consumer-b".into())
        .unwrap();
    let (tx, rx) = mpsc::channel();
    broker.attach.approvals.lock().unwrap().insert(
        "request-7".into(),
        PendingApproval {
            request: ApprovalRequest {
                session_id: 7,
                request_id: "request-7".into(),
                provider: "codex".into(),
                tool_name: "Bash".into(),
                description: None,
                input: "{}".into(),
                permission_suggestions: vec![],
                pre_tool_use: false,
            },
            response: tx,
        },
    );

    broker.unregister_approval_consumer("consumer-a");
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    assert!(broker.pending_approval(7).is_some());

    broker.unregister_approval_consumer("consumer-b");
    assert!(matches!(rx.recv().unwrap(), ApprovalDecision::Pass));
    assert!(broker.pending_approval(7).is_none());
}

#[test]
fn destroying_chat_window_resets_viewed_session() {
    // 关窗兜底（reset_viewer）：窗毁后注册整体失效，无条件清零；漏清会让 emitter
    // 对已不存在的消费者白做 base64 + emit（app 级 emit 后 emit 侧查不到窗口存在性）。
    let broker = PtyBroker::default();
    broker.set_viewer(42);
    assert_eq!(broker.viewed_session.load(Ordering::Acquire), 42);
    broker.reset_viewer();
    assert_eq!(broker.viewed_session.load(Ordering::Acquire), 0);
    // 与 clear_viewer 的分工：CAS 注销只清自己的注册，误清他人会让实时流断掉。
    broker.set_viewer(7);
    broker.clear_viewer(8);
    assert_eq!(broker.viewed_session.load(Ordering::Acquire), 7);
    broker.clear_viewer(7);
    assert_eq!(broker.viewed_session.load(Ordering::Acquire), 0);
}

#[test]
fn destroying_chat_window_clears_every_desktop_lease() {
    // 窗口销毁时前端的 unregister 未必执行得到；关窗兜底必须把桌面租约清干净，
    // 否则残留租约会让下一个审批入队空等 300s 而不是立即交还 TUI。
    let broker = PtyBroker::default();
    broker
        .register_approval_consumer(7, "consumer-a".into())
        .unwrap();
    broker
        .register_approval_consumer(8, "consumer-b".into())
        .unwrap();
    broker.release_desktop_consumers();
    assert!(!broker.has_approval_consumer(7));
    assert!(!broker.has_approval_consumer(8));
}

#[test]
fn closing_chat_window_spares_fresh_remote_leases_and_their_approvals() {
    // 关桌面对话窗不许连坐手机端：远端新鲜租约保留，其会话的挂起审批继续可领取；
    // 只有无人看的会话的审批被交还 TUI。
    let broker = PtyBroker::default();
    broker
        .register_approval_consumer(7, "chat-a".into())
        .unwrap();
    broker
        .register_approval_consumer(8, "remote:phone".into())
        .unwrap();
    let desktop_rx = seed_pending_approval(&broker, 7, "request-7");
    let remote_rx = seed_pending_approval(&broker, 8, "request-8");

    broker.release_desktop_consumers();

    assert!(!broker.has_approval_consumer(7), "桌面租约应被清掉");
    assert!(broker.has_approval_consumer(8), "远端新鲜租约应保留");
    assert!(
        matches!(desktop_rx.recv().unwrap(), ApprovalDecision::Pass),
        "无人看的会话的审批交还 TUI"
    );
    assert!(
        matches!(remote_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "手机端正等着批的卡不得被 pass"
    );
    assert!(broker.pending_approval(8).is_some());
}

#[test]
fn stale_remote_lease_counts_as_absent_everywhere() {
    // 过期远端租约（手机锁屏/被杀，没走 unregister）不得压制任何判定：
    // 目标会话判「无消费者」、关窗时其审批照常交还、顺手从表里清掉。
    let broker = PtyBroker::default();
    broker
        .register_approval_consumer(7, "remote:phone".into())
        .unwrap();
    age_consumer_lease(&broker, "remote:phone", REMOTE_CONSUMER_TTL_MS + 1);

    assert!(!broker.has_approval_consumer(7));
    assert!(!broker.has_fresh_remote_consumer(7));
    assert!(!broker.viewed_session_ids().contains(&7));

    let rx = seed_pending_approval(&broker, 7, "request-7");
    broker.release_desktop_consumers();
    assert!(matches!(rx.recv().unwrap(), ApprovalDecision::Pass));
    assert!(
        broker.attach.approval_consumers.lock().unwrap().is_empty(),
        "过期远端租约应随关窗清理"
    );
}

#[test]
fn unregister_ignores_stale_remote_lease_when_deciding_last_watcher() {
    // 桌面租约注销时若同会话只剩过期远端租约，等同没人看：审批立即交还 TUI，
    // 不许被一个不会来领卡的幽灵租约压着空等 300s。
    let broker = PtyBroker::default();
    broker
        .register_approval_consumer(7, "chat-a".into())
        .unwrap();
    broker
        .register_approval_consumer(7, "remote:phone".into())
        .unwrap();
    age_consumer_lease(&broker, "remote:phone", REMOTE_CONSUMER_TTL_MS + 1);
    let rx = seed_pending_approval(&broker, 7, "request-7");

    broker.unregister_approval_consumer("chat-a");
    assert!(matches!(rx.recv().unwrap(), ApprovalDecision::Pass));
}

#[test]
fn remote_lease_reregistration_refreshes_ttl() {
    // 手机端 20s 心跳 = 同 id 重注册；临期租约续约后必须重新算新鲜。
    let broker = PtyBroker::default();
    broker
        .register_approval_consumer(7, "remote:phone".into())
        .unwrap();
    age_consumer_lease(&broker, "remote:phone", REMOTE_CONSUMER_TTL_MS - 1_000);
    assert!(broker.has_fresh_remote_consumer(7), "临期未过期仍新鲜");
    broker
        .register_approval_consumer(7, "remote:phone".into())
        .unwrap();
    age_consumer_lease(&broker, "remote:phone", 1_000);
    assert!(broker.has_fresh_remote_consumer(7), "续约后重新起算");
}

/// 闸门矩阵：远端在场恒可领取（含轻量模式——chat_enabled 关的是桌面窗，不关远程
/// 通道）；远端缺席时轻量模式回落 TUI、常规模式走桌面召唤。远程未开时注册不出
/// remote: 租约，第一参恒 false，桌面语义与改动前逐字一致。
#[test]
fn approval_gate_matrix() {
    assert_eq!(approval_gate(true, true), ApprovalGate::Claimable);
    assert_eq!(approval_gate(true, false), ApprovalGate::Claimable);
    assert_eq!(approval_gate(false, false), ApprovalGate::FallbackTui);
    assert_eq!(approval_gate(false, true), ApprovalGate::Desktop);
}

#[test]
fn external_approval_passes_immediately_without_gui_consumer() {
    let broker = PtyBroker::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = listener.local_addr().unwrap();
    let server = broker.clone();
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        server.handle_attach(stream, None).unwrap();
    });
    let request = ApprovalRequest {
        session_id: 77,
        request_id: "external-request-77".into(),
        provider: "codex".into(),
        tool_name: "Bash".into(),
        description: Some("build release".into()),
        input: "{}".into(),
        permission_suggestions: vec![],
        pre_tool_use: false,
    };
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&request).unwrap());
    let mut stream = TcpStream::connect(endpoint).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .unwrap();
    writeln!(stream, "MEOWOAPPROVAL1 {} {}", broker.attach.token, encoded).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert_eq!(response, "pass\n");
    assert!(broker.pending_approval(77).is_none());
    handle.join().unwrap();
}

#[test]
fn external_v2_approval_uses_the_shared_framing_and_passes_without_consumer() {
    let broker = PtyBroker::default();
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = listener.local_addr().unwrap();
    let server = broker.clone();
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        server.handle_attach(stream, None).unwrap();
    });
    let request = ApprovalRequest {
        session_id: 78,
        request_id: "external-request-78".into(),
        provider: "codex".into(),
        tool_name: "Bash".into(),
        description: Some("build release".into()),
        input: "{}".into(),
        permission_suggestions: vec![],
        pre_tool_use: false,
    };
    let mut stream = TcpStream::connect(endpoint).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .unwrap();
    meowo_protocol::broker::write_v2_handshake(
        &mut stream,
        &BrokerRequest::Approval {
            token: broker.attach.token.clone(),
            request,
        },
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert_eq!(response, "pass\n");
    assert!(broker.pending_approval(78).is_none());
    handle.join().unwrap();
}

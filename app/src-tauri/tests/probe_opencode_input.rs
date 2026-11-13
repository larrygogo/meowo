//! 复测 OpenCode 在 Windows ConPTY 下的菜单输入；仅发 /models 和 Esc，不提交模型请求。
//! `cargo test -p meowo-app --test probe_opencode_input -- --ignored --nocapture`
//!
//! 必须用**打包 OpenConsole**复现：生产经 portable-pty 加载应用目录的 conpty.dll，而测试
//! 二进制在 deps/ 下找不到它、会悄悄退回系统 conhost——系统 conhost 下原样字节本来就通，
//! 此前探针判「不通」是三处误差叠加：输入框画出前就发键、没答 OpenConsole 的 DA1、在原始
//! 流里找字面串（opentui 按单元格差量重绘，字面串不连续）。改为按 vt100 重建的真实屏幕判定
//! 后原样字节即通（2026-10-16 取证）。`OC_DELAY_MS` 调输入框出现后的等待（默认 0）。

use portable_pty::{native_pty_system, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod common;

/// 把打包的 conpty.dll + OpenConsole.exe 放到测试 exe 旁边，portable-pty 才会加载它们。
fn use_bundled_conpty() {
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/conpty");
    let target = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
    for name in ["conpty.dll", "OpenConsole.exe"] {
        let from = source.join(name);
        assert!(from.exists(), "缺 {}，先跑 scripts/fetch-conpty.mjs", from.display());
        if !target.join(name).exists() {
            std::fs::copy(&from, target.join(name)).unwrap();
        }
    }
}

#[test]
#[ignore = "启动真实 OpenCode；需要本机已安装 CLI"]
fn opencode_models_menu_accepts_pty_input() {
    use_bundled_conpty();
    let argv = meowo_agent::by_id("opencode").unwrap().launch_argv();
    let root = std::env::temp_dir().join(format!("meowo-opencode-input-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = common::agent_command(&argv[0], &root);
    command.args(&argv[1..]);
    // OpenCode 会在启动时创建配置/数据/缓存；探针不得加载真实凭据或改写用户目录。
    for (key, dir) in [
        ("OPENCODE_CONFIG_DIR", "config/opencode"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_STATE_HOME", "state"),
    ] {
        command.env(key, root.join(dir));
    }
    command.env("OPENCODE_DISABLE_AUTOUPDATE", "1");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
    let screen = Arc::new(Mutex::new(vt100::Parser::new(40, 120, 0)));
    let (answer, parser) = (writer.clone(), screen.clone());
    std::thread::spawn(move || {
        let mut bytes = [0; 16_384];
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 {
                break;
            }
            let chunk = &bytes[..count];
            // OpenConsole 启动握手：CPR 与 DA1 都得原样应答（生产里分别由后端与 xterm 答）。
            let mut writer = answer.lock().unwrap();
            if chunk.windows(4).any(|part| part == b"\x1b[6n") {
                let _ = writer.write_all(b"\x1b[1;1R").and_then(|_| writer.flush());
            }
            if chunk.windows(3).any(|part| part == b"\x1b[c") {
                let _ = writer.write_all(b"\x1b[?62;22c").and_then(|_| writer.flush());
            }
            drop(writer);
            parser.lock().unwrap().process(chunk);
        }
    });
    let contents = || screen.lock().unwrap().screen().contents().to_lowercase();
    let send = |text: &str| {
        let mut writer = writer.lock().unwrap();
        writer.write_all(text.as_bytes()).unwrap();
        writer.flush().unwrap();
    };
    let wait_for = |want: &dyn Fn(&str) -> bool, timeout: Duration| {
        let started = Instant::now();
        while started.elapsed() < timeout {
            if want(&contents()) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    };
    let result = (|| -> Result<(), String> {
        if !wait_for(&|s| s.contains("ask anything"), Duration::from_secs(20)) {
            return Err("首屏未出现输入框".into());
        }
        let delay = std::env::var("OC_DELAY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        std::thread::sleep(Duration::from_millis(delay));
        send("/models");
        if !wait_for(&|s| s.contains("/models"), Duration::from_secs(3)) {
            return Err("输入框未回显 /models".into());
        }
        send("\r");
        if !wait_for(&|s| s.contains("select model"), Duration::from_secs(5)) {
            return Err("Enter 后未出现模型选择器".into());
        }
        send("\x1b");
        if !wait_for(&|s| !s.contains("select model"), Duration::from_secs(3)) {
            return Err("Esc 未关闭模型选择器".into());
        }
        Ok(())
    })();
    let last_screen = contents();
    // 失败路径也必须收掉我们创建的 CLI，避免探针留下进程锁与后台任务。
    let _ = child.kill();
    let _ = child.wait();
    drop(pair.master);
    let _ = std::fs::remove_dir_all(root);
    assert!(result.is_ok(), "{}；末屏=\n{last_screen}", result.unwrap_err());
}

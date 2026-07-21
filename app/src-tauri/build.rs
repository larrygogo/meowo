fn main() {
    // Windows：manifest 不走 tauri_build 的默认嵌入（它编进 resource.lib，只挂 bin），
    // 改为用全局 linker 参数嵌进**所有**链接产物。动机是 cargo test：lib 单测可执行
    // 拿不到 bin 的 manifest（cargo 的 rustc-link-arg-tests 也不覆盖 lib 单测目标），
    // loader 便把 comctl32 解析到 System32 的 v5.82——而 tauri 链接闭包引用的
    // TaskDialogIndirect 只存在于 SxS 的 v6，于是整个测试套件以
    // STATUS_ENTRYPOINT_NOT_FOUND 秒死。windows-manifest.xml 与 tauri 默认 manifest 等价，
    // 生产 bin 的行为不变，只是嵌入方式换了。
    let windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows_msvc {
        tauri_build::try_build(
            tauri_build::Attributes::new()
                .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
        )
        .expect("tauri_build::try_build failed");
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        // 注：/MANIFESTINPUT 的路径不能含空格（link.exe 参数不带引号透传）。
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    } else {
        tauri_build::build();
    }
}

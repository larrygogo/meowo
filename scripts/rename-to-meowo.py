#!/usr/bin/env python3
"""把项目从 cc-kanban 重命名为 Meowo（中文喵呜）。"""

import os
import re
import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

def text_files():
    """生成所有需要扫描的文本文件路径（排除 .git、node_modules、target 等）。"""
    skip_dirs = {
        ".git", "node_modules", "target", "dist", ".cargo", ".claude",
        ".github", ".superpowers", "tmp"
    }
    skip_exts = {".png",".ico",".icns",".bmp",".bin",".rgba",".lock",".gif",".jpg",".jpeg",".webp"}
    for dirpath, dirnames, filenames in os.walk(ROOT):
        dirnames[:] = [d for d in dirnames if d not in skip_dirs]
        for f in filenames:
            p = Path(dirpath) / f
            if p.suffix.lower() in skip_exts:
                continue
            # 跳过隐藏会话/日志等明显二进制的无扩展名文件
            if p.suffix == "" and p.name in {"dev.log", "CACHEDIR.TAG"}:
                continue
            yield p

def replace_in_file(path: Path, mapping):
    try:
            data = path.read_bytes()
    except Exception:
            return False
    # 只处理看起来像文本的文件（拒绝含 NUL）
    if b"\x00" in data:
            return False
    text = data.decode("utf-8", errors="surrogateescape")
    new_text = text
    for old, new in mapping:
            new_text = new_text.replace(old, new)
    if new_text == text:
            return False
    path.write_bytes(new_text.encode("utf-8", errors="surrogateescape"))
    print(f"  updated {path.relative_to(ROOT)}")
    return True

def main():
    # 阶段 1：Rust crate / 模块名、可执行文件名、环境变量、目录名
    stage1 = [
        ("meowo_store::", "meowo_store::"),
        ("meowo_reporter::", "meowo_reporter::"),
        ("meowo-store", "meowo-store"),
        ("meowo-reporter", "meowo-reporter"),
        ("meowo_reporter", "meowo_reporter"),
        ("meowo_store", "meowo_store"),
        ("CARGO_BIN_EXE_meowo-reporter", "CARGO_BIN_EXE_meowo-reporter"),
    ]
    print("Stage 1: crates / modules / executables / env prefixes")
    for p in text_files():
        replace_in_file(p, stage1)

    # 阶段 2：显示名称（先做，避免被阶段 3 的批量路径替换误伤）
    # 用正则限定上下文：前后不能是 word/路径字符
    print("\nStage 2: display names")
    for p in text_files():
        if not any(suffix in str(p) for suffix in [".tsx", ".ts", ".rs", ".md", ".json", ".html", ".plist", ".mjs", ".yml", ".yaml", ".css"]):
            continue
        try:
            data = p.read_bytes()
            if b"\x00" in data:
                continue
            text = data.decode("utf-8", errors="surrogateescape")
            new_text = text
            new_text = re.sub(r'(?<![\w/._-])cc-kanban(?![\w/._-])', 'Meowo', new_text)
            new_text = new_text.replace("CC Kanban", "Meowo")
            if new_text != text:
                p.write_bytes(new_text.encode("utf-8", errors="surrogateescape"))
                print(f"  updated {p.relative_to(ROOT)}")
        except Exception as e:
            print(f"  skip {p}: {e}")

    # 阶段 3：应用名、identifier、GitHub、本地目录、环境变量前缀、localStorage key
    stage3 = [
        ("com.larrygogo.meowo", "com.larrygogo.meowo"),
        ("meowo", "meowo"),
        ("MEOWO", "MEOWO"),
        ("meowo-frontend", "meowo-frontend"),
        ("meowo-tray", "meowo-tray"),
        ("meowo-install-", "meowo-install-"),
        ("meowo-test-", "meowo-test-"),
        ("meowo-menubar-", "meowo-menubar-"),
        ("meowo-snap-edge", "meowo-snap-edge"),
        ("meowo-normal-size", "meowo-normal-size"),
        ("meowo-pinned", "meowo-pinned"),
        ("meowo-tab", "meowo-tab"),
        ("meowo-starred", "meowo-starred"),
        ("meowo-usage-provider", "meowo-usage-provider"),
        ("meowo-appearance", "meowo-appearance"),
        ("meowo-lang", "meowo-lang"),
        ("meowo-mock-update", "meowo-mock-update"),
        ("larrygogo/meowo", "larrygogo/meowo"),
        ("~/.meowo", "~/.meowo"),
        (".meowo", ".meowo"),
    ]
    print("\nStage 3: identifiers / paths / env / localStorage keys")
    for p in text_files():
        replace_in_file(p, stage3)

    print("\nStage 4: rename crates directories")
    display_files = [
        "README.md",
        "app/index.html",
        "app/demo.html",
        "app/package.json",
        "app/src-tauri/tauri.conf.json",
        "app/src-tauri/tauri.macos.conf.json",
        "app/src-tauri/Info.plist",
        "app/src-tauri/Cargo.toml",
        "crates/meowo-store/Cargo.toml",
        "crates/meowo-reporter/Cargo.toml",
        "Cargo.toml",
    ]
    # 另外遍历源码中可能包含产品显示名的文件
    for p in text_files():
        if any(suffix in str(p) for suffix in [".tsx", ".ts", ".rs", ".md", ".json", ".html", ".plist", ".mjs", ".yml", ".yaml"]):
            try:
                    data = p.read_bytes()
                    if b"\x00" in data:
                            continue
                    text = data.decode("utf-8", errors="surrogateescape")
                    new_text = text
                    # 把独立的 cc-kanban 显示名替换为 Meowo；保留已经是路径/文件名的情况（阶段2已改）
                    new_text = re.sub(r'(?<![\w/._-])cc-kanban(?![\w/._-])', 'Meowo', new_text)
                    # CC Kanban -> Meowo
                    new_text = new_text.replace("CC Kanban", "Meowo")
                    if new_text != text:
                            p.write_bytes(new_text.encode("utf-8", errors="surrogateescape"))
                            print(f"  updated {p.relative_to(ROOT)}")
            except Exception as e:
                    print(f"  skip {p}: {e}")

    # 阶段 5：重命名 crates 目录
    print("\nStage 5: rename crates directories")
    old_store = ROOT / "crates" / "meowo-store"
    new_store = ROOT / "crates" / "meowo-store"
    old_reporter = ROOT / "crates" / "meowo-reporter"
    new_reporter = ROOT / "crates" / "meowo-reporter"
    if old_store.exists() and not new_store.exists():
        shutil.move(old_store, new_store)
        print(f"  renamed crates/meowo-store -> crates/meowo-store")
    if old_reporter.exists() and not new_reporter.exists():
        shutil.move(old_reporter, new_reporter)
        print(f"  renamed crates/meowo-reporter -> crates/meowo-reporter")

    print("\nDone. Next: review changes and run `cargo check` / `bunx tsc --noEmit`.")

if __name__ == "__main__":
    main()

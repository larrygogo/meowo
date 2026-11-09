#!/usr/bin/env bash
# 用法：bash scripts/probe-lead-nopython.sh app/src-tauri/target/debug/meowo-reporter.exe（先 cargo build -p meowo-reporter）
# 验收：没有 Python 时 SessionStart 注入（agent 答出 SPEC 暗号）、PostToolUse 认领、Stop 拦截都生效。
# 验收 #3：PATH 里去掉 python 后，用 Meowo 内置的 reporter lead 子命令跑真实 claude -p 会话，
# 看 SessionStart 注入与 Stop 拦截是否生效。所有临时文件都在 $P（系统临时目录）里。
set -u
REP="$1"   # meowo-reporter.exe 的路径
P=$(mktemp -d)/nopy-proj
T=$P/.lead/20261109-nopy
mkdir -p "$P/.git" "$T"
echo 20261109-nopy > "$P/.lead/active"
printf '# SPEC\n目的：演示。暗号是 GREEN-17\n' > "$T/SPEC.md"
printf 'phase: implement\n' > "$T/progress.md"
echo '[{"id":1,"check":"由 leader 验收的界面效果（agent 无法自行完成）","passes":false}]' > "$T/acceptance.json"
R=$(cygpath -m "$REP")
cat > "$P/../settings.json" <<EOF
{"hooks":{
 "SessionStart":[{"hooks":[{"type":"command","command":"\"$R\" lead session","timeout":10}]}],
 "PostToolUse":[{"matcher":"Write|Edit|MultiEdit|Bash|PowerShell","hooks":[{"type":"command","command":"\"$R\" lead claim","timeout":10}]}],
 "Stop":[{"hooks":[{"type":"command","command":"\"$R\" lead stop","timeout":10}]}]
}}
EOF
# 去掉所有 python 相关的 PATH 项（含 WindowsApps 里的 python3 别名）
NEWPATH=$(echo "$PATH" | tr ':' '\n' | grep -viE 'python|windowsapps' | paste -sd ':')
export PATH="$NEWPATH"
export MEOWO_DB="$(cygpath -m "$P")/board.db"
echo "python 还在吗: $(command -v python python3 || echo 不在)"
cd "$P"
OUT=$(timeout 400 claude -p "先用一句话说出系统注入的任务 SPEC 里的暗号（没有就说没有）。然后在 .lead/20261109-nopy/progress.md 末尾追加一行：开始。之后结束回复。" \
  --settings "$(cygpath -m "$P/../settings.json")" --permission-mode bypassPermissions --max-turns 15 --output-format json 2>/dev/null)
echo "$OUT" | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const d=JSON.parse(s);console.log("turns",d.num_turns,"| 结果:",String(d.result).slice(0,300))})'
echo "owner 已登记: $( [ -s "$T/.owner" ] && echo 是 || echo 否 )"
echo "拦截状态: $(cat "$T/.stop-state.json" 2>/dev/null)"
echo "登记表: $(cat "$(dirname "$(cygpath -u "$MEOWO_DB")")/lead-sessions.json" 2>/dev/null | head -c 200)"
tail -3 "$T/progress.md"
rm -rf "$(dirname "$P")"

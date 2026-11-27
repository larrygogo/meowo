# Meowo 终端页签：手打 claude 跟随 Meowo 当前选中的 Claude 账号（bash / zsh 共用）。
# 照 Orca 的 getPosixClaudeShellFunction（stablyai/orca c1bd89d）。
# 每次运行都重新读指针文件，所以切换账号后不用重开终端。
# 指针为空 = 默认账号：还原用户自己的 CLAUDE_CONFIG_DIR（没有就去掉）。
# 用户在这个终端里自己设了别的 CLAUDE_CONFIG_DIR、或自己定义了 claude 函数时，以用户的为准。
__meowo_claude_binary="$(unalias claude 2>/dev/null || :; command -v claude 2>/dev/null || :)"
if ! typeset -f claude >/dev/null 2>&1 && [ -n "${__meowo_claude_binary:-}" ] && [ -x "${__meowo_claude_binary}" ] &&
   [ -n "${MEOWO_CLAUDE_PROFILE_POINTER:-}" ]; then
  # 用 function 关键字：claude 若有别名，`claude()` 的写法会先被别名展开成语法错误。
  function claude {
    local __meowo_home __meowo_own="${MEOWO_CLAUDE_USER_CONFIG_DIR%/}" __meowo_cur="${CLAUDE_CONFIG_DIR%/}"
    __meowo_home="$(cat "$MEOWO_CLAUDE_PROFILE_POINTER" 2>/dev/null || :)"
    if [ -n "${CLAUDE_CONFIG_DIR:-}" ] && [ "$CLAUDE_CONFIG_DIR" != "${MEOWO_CLAUDE_INJECTED_CONFIG_DIR:-}" ] &&
       { [ -z "$__meowo_own" ] || [ "$__meowo_cur" != "$__meowo_own" ]; }; then
      command claude "$@"; return
    fi
    # 子 shell + exec：环境只改给这一次 claude，不污染这个终端。
    if [ -z "$__meowo_home" ]; then
      if [ -n "${MEOWO_CLAUDE_USER_CONFIG_DIR:-}" ]; then
        ( unset MEOWO_CLAUDE_INJECTED_CONFIG_DIR; export CLAUDE_CONFIG_DIR="$MEOWO_CLAUDE_USER_CONFIG_DIR"; unset -f claude; exec claude "$@" ); return
      fi
      ( unset CLAUDE_CONFIG_DIR MEOWO_CLAUDE_INJECTED_CONFIG_DIR; unset -f claude; exec claude "$@" ); return
    fi
    # 指针坏了：宁可不跑，也不要在错的账号下跑。
    case "$__meowo_home" in /*|[A-Za-z]:*) ;; *) return 1 ;; esac
    ( export CLAUDE_CONFIG_DIR="$__meowo_home" MEOWO_CLAUDE_INJECTED_CONFIG_DIR="$__meowo_home"; unset -f claude; exec claude "$@" )
  }
fi
unset __meowo_claude_binary

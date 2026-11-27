# Meowo 终端页签：手打 claude 跟随 Meowo 当前选中的 Claude 账号。
# 照 Orca 的 getPowerShellClaudeShellFunction（stablyai/orca c1bd89d）。
# 每次运行都重新读指针文件，所以切换账号后不用重开终端。
# 指针为空 = 默认账号：还原用户自己的 CLAUDE_CONFIG_DIR（没有就去掉）。
# 用户在这个终端里自己设了别的 CLAUDE_CONFIG_DIR 时，以用户的为准。
$meowoClaudeCommand = Get-Command claude -CommandType Application, ExternalScript -ErrorAction SilentlyContinue | Select-Object -First 1
$meowoClaudeFunction = Get-Command claude -CommandType Function -ErrorAction SilentlyContinue
if ($env:MEOWO_CLAUDE_PROFILE_POINTER -and $meowoClaudeCommand -and -not $meowoClaudeFunction) {
    function Global:claude {
        $names = @('CLAUDE_CONFIG_DIR', 'MEOWO_CLAUDE_INJECTED_CONFIG_DIR')
        $saved = @{}
        foreach ($name in $names) { $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
        try {
            $meowoHome = ''
            if (Test-Path -LiteralPath $env:MEOWO_CLAUDE_PROFILE_POINTER -PathType Leaf) {
                $meowoHome = [IO.File]::ReadAllText($env:MEOWO_CLAUDE_PROFILE_POINTER).Trim()
            }
            # Windows 上同一个目录两种分隔符都可能出现。
            $own = ("$env:MEOWO_CLAUDE_USER_CONFIG_DIR" -replace '/', '\').TrimEnd('\')
            $current = ("$env:CLAUDE_CONFIG_DIR" -replace '/', '\').TrimEnd('\')
            if ($env:CLAUDE_CONFIG_DIR -and $env:CLAUDE_CONFIG_DIR -ne $env:MEOWO_CLAUDE_INJECTED_CONFIG_DIR -and (-not $own -or $current -ne $own)) {
                # 这个终端里用户自己设的值优先。
            } elseif (-not $meowoHome) {
                Remove-Item Env:MEOWO_CLAUDE_INJECTED_CONFIG_DIR -ErrorAction SilentlyContinue
                if ($env:MEOWO_CLAUDE_USER_CONFIG_DIR) { $env:CLAUDE_CONFIG_DIR = $env:MEOWO_CLAUDE_USER_CONFIG_DIR }
                else { Remove-Item Env:CLAUDE_CONFIG_DIR -ErrorAction SilentlyContinue }
            } elseif (-not [IO.Path]::IsPathRooted($meowoHome)) {
                # 指针坏了：宁可不跑，也不要在错的账号下跑。
                $global:LASTEXITCODE = 1
                return
            } else {
                $env:CLAUDE_CONFIG_DIR = $meowoHome
                $env:MEOWO_CLAUDE_INJECTED_CONFIG_DIR = $meowoHome
            }
            $binary = Get-Command claude -CommandType Application, ExternalScript -ErrorAction Stop | Select-Object -First 1
            if ($MyInvocation.ExpectingInput) { $input | & $binary.Source @args } else { & $binary.Source @args }
            $global:LASTEXITCODE = $LASTEXITCODE
        } catch {
            $global:LASTEXITCODE = 1
            Write-Error $_ -ErrorAction Continue
        } finally {
            # 用 Remove-Item：.NET 9 起把 $null 写回去会留下一个空变量，而不是删掉它。
            foreach ($name in $names) {
                if ($null -eq $saved[$name]) { Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue }
                else { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
            }
        }
    }
}
Remove-Variable meowoClaudeCommand, meowoClaudeFunction -ErrorAction SilentlyContinue

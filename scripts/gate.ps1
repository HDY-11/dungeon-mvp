#!/usr/bin/env powershell
# 本地/CI 门禁：新 core 方向的「一条命令」验收（REFACTOR.md §10.6 第 5 项 / §11.3 Phase F4）。
#
# 用法：
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/gate.ps1            # 默认 --offline
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/gate.ps1 -Online    # 需要联网拉依赖
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/gate.ps1 -SkipClippy # 只跑构建与测试
#
# 注意：本文件必须保存为 **UTF-8 with BOM**。Windows PowerShell 5.1 在无 BOM 时
# 会按系统 ANSI 代码页（中文 Windows 是 GBK）解析 .ps1，中文注释会直接变成语法错误。
#
# 覆盖范围与**不覆盖**范围：
#   覆盖  —— workspace 构建检查、新方向的测试（render-api / ecs_core / presentation /
#            utils / tui / sys）、render-api / presentation / ecs_core 的 clippy（-D warnings）。
#   不覆盖 —— 旧 `dungeon-*` crate 与根 `dungeon-app` 的测试：它们针对被取代的旧架构，
#            不纳入新代码门禁（REFACTOR.md §10.4）。`cargo test --workspace` 也不在此脚本里，
#            因为它会带上那些旧测试目标；需要全量时请单独运行。

[CmdletBinding()]
param(
    [switch]$Online,
    [switch]$SkipClippy
)

$ErrorActionPreference = 'Stop'
$offlineArgs = if ($Online) { @() } else { @('--offline') }

# 从脚本位置反推仓库根，保证在任意 cwd 下都能跑。
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot

# 每个步骤：名字 + cargo 参数。
$steps = @(
    @{ Name = 'cargo check --workspace';   Args = @('check', '--workspace') + $offlineArgs }
    @{ Name = 'cargo test (新 core 方向)'; Args = @('test', '-p', 'render-api', '-p', 'ecs_core', '-p', 'presentation', '-p', 'utils', '-p', 'tui', '-p', 'sys') + $offlineArgs }
    # 端到端：装配层 + 整条链路（喂输入 → 世界推进 → 提取帧 → 真实绘制）。
    # 它覆盖了 bin 之外的全部代码，所以能回答"装起来还能不能跑"。
    @{ Name = 'cargo test -p dungeon-app (端到端)'; Args = @('test', '-p', 'dungeon-app', '--test', 'mvp_loop_test') + $offlineArgs }
    @{ Name = 'cargo test -p dungeon-app (单元)';   Args = @('test', '-p', 'dungeon-app', '--lib') + $offlineArgs }
    @{ Name = 'cargo build -p dungeon-app (bin)';   Args = @('build', '-p', 'dungeon-app') + $offlineArgs }
)
if (-not $SkipClippy) {
    # 注意 --offline 必须放在 -- **之前**：`--` 之后的参数是给 rustc 的，
    # `--offline` 会被 rustc 当成未知选项（本脚本第一版就踩了这个坑）。
    $steps += @{ Name = 'cargo clippy -p render-api'; Args = @('clippy', '-p', 'render-api', '--all-targets') + $offlineArgs + @('--', '-D', 'warnings') }
    $steps += @{ Name = 'cargo clippy -p presentation'; Args = @('clippy', '-p', 'presentation', '--all-targets') + $offlineArgs + @('--', '-D', 'warnings') }
    $steps += @{ Name = 'cargo clippy -p ecs_core';       Args = @('clippy', '-p', 'ecs_core', '--all-targets') + $offlineArgs + @('--', '-D', 'warnings') }
}

# 直接调用 cargo，但把「原生命令的非零退出」交给 $LASTEXITCODE 判定：
# 默认的 $ErrorActionPreference='Stop' 会把原生命令写到 stderr 的正常输出
# （"Finished ..."）当成 NativeCommandError 直接中断脚本，所以这里临时放宽，
# 再用 $LASTEXITCODE 拿真实退出码。
$results = @()
foreach ($step in $steps) {
    Write-Host ''
    Write-Host "=== $($step.Name) ===" -ForegroundColor Cyan
    $ErrorActionPreference = 'Continue'
    & cargo @($step.Args)
    $exitCode = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    $results += [pscustomobject]@{ Step = $step.Name; ExitCode = $exitCode }
}

# ── 依赖边界（DESIGN Dsn28）────────────────────────────
#
# 这三条边界是「渲染后端可替换」的全部保证，而它们**只能靠依赖图验证**：
# 代码里看起来没用到 `ecs_core` 不代表依赖表里没有（那样早晚会有人"顺手"用一下）。
# 用 `cargo tree` 当门禁，比代码评审可靠。
Write-Host ''
Write-Host '=== 依赖边界检查 ===' -ForegroundColor Cyan
$forbidden = @(
    @{ Pkg = 'tui';          Banned = 'ecs_core';   Why = 'tui 不得依赖 ecs_core（只消费 render-api）' }
    @{ Pkg = 'tui';          Banned = 'presentation'; Why = 'tui 不得依赖 presentation（只消费 render-api）' }
    @{ Pkg = 'presentation'; Banned = 'ratatui';    Why = 'presentation 不得依赖 ratatui' }
    @{ Pkg = 'presentation'; Banned = 'crossterm';  Why = 'presentation 不得依赖 crossterm' }
    @{ Pkg = 'render-api';   Banned = 'ecs_core';   Why = 'render-api 不得依赖 ecs_core' }
)
$boundaryOk = $true
foreach ($rule in $forbidden) {
    $ErrorActionPreference = 'Continue'
    $tree = & cargo tree -p $rule.Pkg --offline 2>&1 | Out-String
    $ErrorActionPreference = 'Stop'
    # 只匹配"依赖边"，避免把包名出现在注释/说明里的情况算进去。
    if ($tree -match "(?m)^[│├└─\s]*[│├└─\s]$([regex]::Escape($rule.Banned))\s") {
        Write-Host ("  FAIL  {0} 依赖了 {1}：{2}" -f $rule.Pkg, $rule.Banned, $rule.Why) -ForegroundColor Red
        $boundaryOk = $false
    } else {
        Write-Host ("  PASS  {0} 不依赖 {1}" -f $rule.Pkg, $rule.Banned) -ForegroundColor Green
    }
}
$results += [pscustomobject]@{ Step = '依赖边界（Dsn28）'; ExitCode = $(if ($boundaryOk) { 0 } else { 1 }) }

Write-Host ''
Write-Host '=== 门禁结果 ===' -ForegroundColor Cyan
$failed = 0
foreach ($result in $results) {
    if ($result.ExitCode -eq 0) {
        Write-Host ("  PASS  {0}" -f $result.Step) -ForegroundColor Green
    } else {
        Write-Host ("  FAIL  {0}  (exit {1})" -f $result.Step, $result.ExitCode) -ForegroundColor Red
        $failed++
    }
}

Pop-Location
if ($failed -gt 0) {
    Write-Host ''
    Write-Host "$failed 个步骤失败" -ForegroundColor Red
    exit 1
}
Write-Host ''
Write-Host '全部门禁通过' -ForegroundColor Green
exit 0

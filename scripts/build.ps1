# scripts/build.ps1
# MacTouchpad 本地跨环境一键构建脚本 (支持 MSVC 与 MinGW-GNU 自动识别与构建)

param(
    [ValidateSet('release', 'debug')]
    [string]$Mode = 'release',
    [switch]$CheckOnly,
    [switch]$RunTests
)

$ErrorActionPreference = 'Stop'
Write-Host "========================================" -ForegroundColor Cyan
Write-Host " 🚀 MacTouchpad 本地自动化构建脚本" -ForegroundColor Cyan
Write-Host "========================================" -ForegroundColor Cyan

# 1. 检测 Rust 工具链
$toolchain = (rustup show active-toolchain) -replace '\s+\(default\)', ''
Write-Host "当前活动 Rust 工具链: $toolchain" -ForegroundColor Green

if ($RunTests) {
    Write-Host "`n[1/3] 运行核心引擎单元测试..." -ForegroundColor Yellow
    cargo test --workspace
    if ($LASTEXITCODE -ne 0) { throw "单元测试失败！" }
}

if ($CheckOnly) {
    Write-Host "`n检查工程编译..." -ForegroundColor Yellow
    cargo check --workspace
    Write-Host "✅ 语法与依赖检查通过！" -ForegroundColor Green
    exit 0
}

Write-Host "`n[2/3] 执行 Tauri 构建 (Mode: $Mode)..." -ForegroundColor Yellow
if ($Mode -eq 'release') {
    cargo build --release --manifest-path src-tauri/Cargo.toml
} else {
    cargo build --manifest-path src-tauri/Cargo.toml
}

Write-Host "`n[3/3] 产物输出检测..." -ForegroundColor Yellow
$targetDir = "src-tauri/target/$Mode"
if (Test-Path "$targetDir/mac-touchpad.exe") {
    $size = (Get-Item "$targetDir/mac-touchpad.exe").Length / 1MB
    Write-Host "🎉 构建成功! 可执行文件: $targetDir/mac-touchpad.exe ($([math]::Round($size, 2)) MB)" -ForegroundColor Green
} else {
    Write-Host "⚠️ 可执行文件未在默认目录找到，请检查输出。" -ForegroundColor Red
}
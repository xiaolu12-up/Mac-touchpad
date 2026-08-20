<#
.SYNOPSIS
    MacTouchpad 一键版本递增与发版准备脚本
.DESCRIPTION
    1. 自动更新 src-tauri/Cargo.toml 中的版本号
    2. 自动运行 cargo check 刷新 Cargo.lock
    3. 自动生成 docs/release-notes/vX.Y.Z.md 初稿
    4. 输出一键提交与发版 Git 指令
.EXAMPLE
    .\bump.ps1 -Version "0.1.6"
#>

param (
    [Parameter(Mandatory=$true)]
    [string]$Version,

    [switch]$NoDoc
)

# Strip leading 'v' or 'V' if present
$ver = $Version.TrimStart('v').TrimStart('V')

# Validate version format (e.g. 0.1.2)
if ($ver -notmatch '^\d+\.\d+\.\d+$') {
    Write-Error "版本号格式错误，应为 X.Y.Z 格式（例如 0.1.6）"
    exit 1
}

$tag = "v$ver"

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "🚀 开始递增版本到: $ver (Tag: $tag)" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan

# Helper function to write UTF-8 without BOM
function Write-Utf8NoBom ($FilePath, $Content) {
    $Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    $AbsolutePath = [System.IO.Path]::GetFullPath($FilePath)
    [System.IO.File]::WriteAllText($AbsolutePath, $Content, $Utf8NoBom)
}

# 1. Update src-tauri/Cargo.toml
$cargoPath = "src-tauri/Cargo.toml"
if (Test-Path $cargoPath) {
    $content = (Get-Content $cargoPath -Raw -Encoding utf8) -replace '(?m)^version = ".*"', "version = `"$ver`""
    Write-Utf8NoBom $cargoPath $content
    Write-Host "[OK] 已更新 $cargoPath" -ForegroundColor Green
}

# 2. Run cargo check to update Cargo.lock
Write-Host "正在运行 cargo check 更新 Cargo.lock ..." -ForegroundColor Yellow
cargo check --manifest-path src-tauri/Cargo.toml

# 3. 自动生成 Release Notes 初稿
if (-not $NoDoc) {
    $genScript = "scripts/generate-release-notes.ps1"
    if (Test-Path $genScript) {
        Write-Host "正在自动生成 Release Notes ..." -ForegroundColor Yellow
        & $genScript -Version $ver
    }
}

Write-Host "`n==========================================" -ForegroundColor Green
Write-Host "🎉 版本递增与发版准备完成！" -ForegroundColor Green
Write-Host "==========================================" -ForegroundColor Green
Write-Host "请按以下步骤完成发布：" -ForegroundColor White
Write-Host " 1. 查看并微调发布文档: docs/release-notes/$tag.md" -ForegroundColor Gray
Write-Host " 2. 执行以下命令提交并推送 Tag (将自动触发 GitHub Actions 打包发布):`n" -ForegroundColor Gray

Write-Host "git add -A" -ForegroundColor Cyan
Write-Host "git commit -m `"chore: release $tag`"" -ForegroundColor Cyan
Write-Host "git tag $tag" -ForegroundColor Cyan
Write-Host "git push origin main --tags" -ForegroundColor Cyan
Write-Host ""

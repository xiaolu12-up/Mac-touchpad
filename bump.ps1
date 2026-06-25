param (
    [Parameter(Mandatory=$true)]
    [string]$Version
)

# Validate version format (e.g. 0.1.2)
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    Write-Error "版本号格式错误，应为 X.Y.Z 格式（例如 0.1.1）"
    exit 1
}

Write-Host "开始将版本号修改为: $Version ..."

# Update src-tauri/Cargo.toml
$cargoPath = "src-tauri/Cargo.toml"
if (Test-Path $cargoPath) {
    (Get-Content $cargoPath -Encoding utf8) -replace '^version = ".*"', "version = `"$Version`"" | Set-Content $cargoPath -Encoding utf8
    Write-Host "✓ 已更新 $cargoPath"
}

# Update src-tauri/tauri.conf.json
$tauriConfPath = "src-tauri/tauri.conf.json"
if (Test-Path $tauriConfPath) {
    (Get-Content $tauriConfPath -Encoding utf8) -replace '"version": ".*"', "`"version`": `"$Version`"" | Set-Content $tauriConfPath -Encoding utf8
    Write-Host "✓ 已更新 $tauriConfPath"
}

# Run cargo check to update Cargo.lock
Write-Host "正在运行 cargo check 以同步更新 Cargo.lock ..."
cargo check --manifest-path src-tauri/Cargo.toml

Write-Host "✓ 版本更新全部完成！"

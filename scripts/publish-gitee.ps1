<#
.SYNOPSIS
    MacTouchpad 自动同步发布到 Gitee Releases
.DESCRIPTION
    使用 Gitee OpenAPI v5 创建发行版并上传安装包资产。
    可在 GitHub Actions 中自动运行，也可在本地执行。
.EXAMPLE
    .\scripts\publish-gitee.ps1 -Token "your_gitee_token" -Tag "v0.1.7"
#>

param (
    [string]$Token = $env:GITEE_TOKEN,
    [string]$Owner = "lu52",
    [string]$Repo = "Mac-touchpad",
    [string]$Tag = "",
    [string]$Name = "",
    [string]$Body = "",
    [string[]]$Files = @()
)

$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($Token)) {
    Write-Host "⚠️ [Gitee Sync] GITEE_TOKEN 未配置，跳过 Gitee 发行版同步。" -ForegroundColor Yellow
    Write-Host "   如需启用，请在 GitHub 仓库 Settings -> Secrets -> Actions 中添加 GITEE_TOKEN。" -ForegroundColor Gray
    exit 0
}

# 1. 规范化 Tag
if ([string]::IsNullOrWhiteSpace($Tag)) {
    $Tag = (git describe --tags --abbrev=0 2>$null)
    if (-not $Tag) { $Tag = "v0.1.7" }
}
if (-not $Tag.StartsWith("v") -and $Tag -match '^[0-9]') {
    $Tag = "v$Tag"
}
$ver = $Tag.TrimStart('v')

if ([string]::IsNullOrWhiteSpace($Name)) {
    $Name = "MacTouchpad $Tag"
}

# 2. 读取发布说明
if ([string]::IsNullOrWhiteSpace($Body)) {
    $docPath = "docs/release-notes/$Tag.md"
    if (Test-Path $docPath) {
        $Body = Get-Content $docPath -Raw -Encoding utf8
    } elseif (Test-Path "docs/release-notes/v$ver.md") {
        $Body = Get-Content "docs/release-notes/v$ver.md" -Raw -Encoding utf8
    } elseif (Test-Path "CHANGELOG.md") {
        $changelog = Get-Content "CHANGELOG.md" -Raw -Encoding utf8
        if ($changelog -match "(?ms)## \[$ver\].*?(?=(## \[|$))") {
            $Body = $matches[0]
        }
    }
}
if ([string]::IsNullOrWhiteSpace($Body)) {
    $Body = "MacTouchpad $Tag 自动化构建版本。可在下方下载对应安装程序。"
}

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "🚀 开始同步发布到 Gitee Release: $Tag" -ForegroundColor Cyan
Write-Host "目标仓库: $Owner/$Repo" -ForegroundColor Yellow
Write-Host "==========================================" -ForegroundColor Cyan

# 3. 检查 Gitee 上是否已存在该 Release
$releaseId = $null
$releaseUrl = "https://gitee.com/api/v5/repos/$Owner/$Repo/releases/tags/$Tag"
try {
    $existing = Invoke-RestMethod -Uri "$releaseUrl?access_token=$Token" -Method Get -ErrorAction SilentlyContinue
    if ($existing -and $existing.id) {
        $releaseId = $existing.id
        Write-Host "ℹ️ Gitee 上已存在发行版: ID = $releaseId" -ForegroundColor Yellow
    }
} catch {
    # 404
}

# 4. 如果不存在，调用 API 创建 Release
if (-not $releaseId) {
    Write-Host "正在创建 Gitee 发行版: $Name ($Tag)..." -ForegroundColor Yellow
    $createUrl = "https://gitee.com/api/v5/repos/$Owner/$Repo/releases"
    $payloadObj = @{
        access_token = $Token
        tag_name     = $Tag
        name         = $Name
        body         = $Body
        prerelease   = $false
    }
    $payloadJson = $payloadObj | ConvertTo-Json -Compress

    try {
        $res = Invoke-RestMethod -Uri $createUrl -Method Post -Body $payloadJson -ContentType "application/json;charset=utf-8"
        $releaseId = $res.id
        Write-Host "✅ Gitee 发行版创建成功: ID = $releaseId" -ForegroundColor Green
    } catch {
        Write-Error "创建 Gitee 发行版失败: $_"
        exit 1
    }
}

# 5. 查找待上传的安装包文件
if ($Files.Count -eq 0) {
    $searchPaths = @(
        "src-tauri/target/release/bundle/nsis/*.exe",
        "src-tauri/target/release/bundle/msi/*.msi",
        "src-tauri/target/release/bundle/*.exe",
        "src-tauri/target/release/bundle/*.msi"
    )
    foreach ($pattern in $searchPaths) {
        $found = @(Get-Item $pattern -ErrorAction SilentlyContinue)
        foreach ($f in $found) {
            if ($f.FullName -notmatch '-debug' -and $Files -notcontains $f.FullName) {
                $Files += $f.FullName
            }
        }
    }
}

if ($Files.Count -eq 0) {
    Write-Host "⚠️ 未找到安装包文件 (nsis/*.exe 或 msi/*.msi)，跳过附件上传。" -ForegroundColor Yellow
    exit 0
}

# 6. 上传附件到 Gitee Release
$uploadUrl = "https://gitee.com/api/v5/repos/$Owner/$Repo/releases/$releaseId/attach_files"

try {
    $relInfo = Invoke-RestMethod -Uri "https://gitee.com/api/v5/repos/$Owner/$Repo/releases/$releaseId?access_token=$Token" -Method Get
    $existingAssets = @($relInfo.assets | ForEach-Object { $_.name })
} catch {
    $existingAssets = @()
}

foreach ($file in $Files) {
    if (-not (Test-Path $file)) { continue }
    $fileName = [System.IO.Path]::GetFileName($file)
    $sizeMb = [math]::Round((Get-Item $file).Length / 1MB, 2)

    if ($existingAssets -contains $fileName) {
        Write-Host "ℹ️ 附件已存在，跳过: $fileName ($sizeMb MB)" -ForegroundColor Gray
        continue
    }

    Write-Host "正在上传附件: $fileName ($sizeMb MB) 到 Gitee..." -ForegroundColor Yellow

    $curlOut = & curl.exe -s -w "\n%{http_code}" -X POST -F "access_token=$Token" -F "file=@$file" $uploadUrl

    $lines = $curlOut.Trim().Split("\n")
    $statusCode = $lines[$lines.Length - 1].Trim()

    if ($statusCode -eq "201" -or $statusCode -eq "200") {
        Write-Host "✅ 上传成功: $fileName" -ForegroundColor Green
    } else {
        $errMsg = $lines[0..($lines.Length - 2)] -join " "
        Write-Host "⚠️ 上传状态码 ${statusCode}: ${errMsg}" -ForegroundColor Red
    }
}

Write-Host "\n🎉 Gitee 发行版与安装包同步完成！" -ForegroundColor Green
Write-Host "访问地址: https://gitee.com/$Owner/$Repo/releases/tag/$Tag" -ForegroundColor Cyan

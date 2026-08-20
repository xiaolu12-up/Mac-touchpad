<#
.SYNOPSIS
    MacTouchpad 自动化 Release Notes 生成工具
.DESCRIPTION
    基于 Git Commit 差异和 PR 记录，自动生成符合标准结构的发布日志（Markdown），
    并支持保存至 docs/release-notes/vX.Y.Z.md 及同步至 CHANGELOG.md。
.PARAMETER Version
    发布的版本号，如 "0.1.6" 或 "v0.1.6"
.PARAMETER FromTag
    起始 Tag（默认为前一个最新 Tag）
.PARAMETER UpdateChangelog
    是否自动同步追加到 CHANGELOG.md 开头
.PARAMETER PreviewOnly
    仅在控制台预览，不写入文件
#>

param (
    [Parameter(Mandatory=$true)]
    [string]$Version,

    [string]$FromTag,

    [switch]$UpdateChangelog,

    [switch]$PreviewOnly
)

# 1. 规范化版本号
$ver = $Version.TrimStart('v').TrimStart('V')
if ($ver -notmatch '^\d+\.\d+\.\d+$') {
    Write-Error "版本号格式错误，应为 X.Y.Z 格式（例如 0.1.6）"
    exit 1
}
$tag = "v$ver"

# 2. 确定比较的起始 Tag
if ([string]::IsNullOrWhiteSpace($FromTag)) {
    $allTags = @(git tag --sort=-creatordate)
    if ($allTags.Count -gt 0) {
        $FromTag = $allTags | Where-Object { $_ -ne $tag -and $_ -ne $ver } | Select-Object -First 1
    }
}

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "🚀 MacTouchpad Release Notes 生成器" -ForegroundColor Cyan
Write-Host "目标版本 : v$ver" -ForegroundColor Yellow
Write-Host "对比基线 : $(if ($FromTag) { $FromTag } else { '项目起点' })" -ForegroundColor Yellow
Write-Host "==========================================" -ForegroundColor Cyan

# 3. 提取 Git 提交记录
$range = if ($FromTag) { "$FromTag..HEAD" } else { "HEAD" }
$rawLogs = @(git log $range --pretty=format:"%s|%h|%an" 2>$null)

$feats = @()
$fixes = @()
$others = @()
$contributors = [System.Collections.Generic.HashSet[string]]::new()

if ($rawLogs.Count -gt 0) {
    foreach ($line in $rawLogs) {
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        $parts = $line.Split('|')
        if ($parts.Count -ge 3) {
            $msg = $parts[0].Trim()
            $hash = $parts[1].Trim()
            $author = $parts[2].Trim()
            if ($author) { [void]$contributors.Add($author) }

            # 忽略日常合并和 chore tag 提交
            if ($msg -match '^(Merge branch|chore\(release\)|chore: release|bump version)') {
                continue
            }

            if ($msg -match '^(feat|Feat|新增|功能)') {
                $cleanMsg = $msg -replace '^(feat|Feat)(\([^)]+\))?:\s*', ''
                $feats += "- **$cleanMsg**"
            } elseif ($msg -match '^(fix|Fix|修复|Bug)') {
                $cleanMsg = $msg -replace '^(fix|Fix)(\([^)]+\))?:\s*', ''
                $fixes += "- **$cleanMsg**"
            } else {
                $others += "- $msg ($hash)"
            }
        }
    }
}

# 默认回退内容，防止初次或无特定前缀 commit 时空白
if ($feats.Count -eq 0) {
    $feats += "- **核心功能优化**：请在此处填入本版本主要新增功能特性。"
}
if ($fixes.Count -eq 0) {
    $fixes += "- **稳定性与兼容性修复**：修复已知触控与滚动交互问题。"
}

$featText = $feats -join "`n"
$fixText = $fixes -join "`n"
$today = (Get-Date).ToString("yyyy-MM-dd")

$contribList = @($contributors | ForEach-Object { "@$_" })
$contribStr = if ($contribList.Count -gt 0) { $contribList -join ", " } else { "所有提出反馈与建议的社区用户！" }

# 4. 组装符合 CC Switch 风格的 Markdown 正文
$markdown = @"
# MacTouchpad v$ver

本版主要包含以下改动与优化：

[English →](https://github.com/xiaolu12-up/Mac-touchpad/blob/v$ver/README_EN.md) | [更新日志 →](https://github.com/xiaolu12-up/Mac-touchpad/blob/v$ver/CHANGELOG.md)

---

### 🌟 重点内容：你现在可以
$featText
$fixText

---

### ⚠️ 唯一官方渠道声明（安全防伪提示）
MacTouchpad 是完全免费、开源的 Windows 触控板增强工具，不收取任何费用。请仅通过官方渠道获取：
| 类别 | 官方渠道 |
| :--- | :--- |
| **源码仓库** | [github.com/xiaolu12-up/Mac-touchpad](https://github.com/xiaolu12-up/Mac-touchpad) |
| **官方发布页** | [GitHub Releases](https://github.com/xiaolu12-up/Mac-touchpad/releases) |
| **问题与建议反馈** | [GitHub Issues](https://github.com/xiaolu12-up/Mac-touchpad/issues) |

---

### 📝 深度详解与改动

#### 🚀 新功能 (Features)
$featText

#### 🐛 修复与优化 (Bug Fixes & Improvements)
$fixText

---

### 🚨 升级与使用提醒
- **配置兼容**：本版完全向下兼容既有配置，覆盖升级即可无缝使用。
- **权限与拦截**：如遇手势未响应，请检查是否有安全软件或其它触控工具拦截了 RawInput 报文。

---

### 💖 致谢
感谢以下贡献者对本版本的代码贡献与测试反馈：
$contribStr

---

### 📦 资产下载与系统要求

#### 系统要求
- **系统**：Windows 10 / Windows 11 (64-bit)
- **硬件**：支持 Windows 精准触控板 (Precision Touchpad, PTP) 的笔记本或触控板外设。

#### 下载安装
| 文件 | 格式 | 说明 |
| :--- | :--- | :--- |
| `MacTouchpad_${ver}_x64-setup.exe` | NSIS 安装包 | **推荐**。中文安装向导，自动配置开机自启与系统托盘。 |
| `MacTouchpad_${ver}_portable.zip` | 便携绿色版 | 解压即用，配置保存在运行目录。 |
"@

# 5. 输出或写入文件
if ($PreviewOnly) {
    Write-Host "`n----- [PREVIEW] Release Notes Markdown -----" -ForegroundColor Green
    Write-Host $markdown
    Write-Host "---------------------------------------------" -ForegroundColor Green
    return
}

# 辅助函数：以 UTF-8 无 BOM 格式写入
function Write-Utf8NoBom ($FilePath, $Content) {
    $Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    $AbsolutePath = [System.IO.Path]::GetFullPath($FilePath)
    $dir = [System.IO.Path]::GetDirectoryName($AbsolutePath)
    if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    [System.IO.File]::WriteAllText($AbsolutePath, $Content, $Utf8NoBom)
}

# 写入 docs/release-notes/vX.Y.Z.md
$docPath = "docs/release-notes/v$ver.md"
Write-Utf8NoBom $docPath $markdown
Write-Host "[OK] 已生成发布日志文档 : $docPath" -ForegroundColor Green

# 同步追加到 CHANGELOG.md
if ($UpdateChangelog) {
    $changelogPath = "CHANGELOG.md"
    if (Test-Path $changelogPath) {
        $existing = Get-Content $changelogPath -Raw -Encoding utf8
        $changelogEntry = @"
## [$ver] - $today

### 新增与优化 (Added & Improved)
$featText

### 修复 (Fixed)
$fixText

"@
        if ($existing -match '(?m)^## \[') {
            $newChangelog = $existing -replace '(?m)^## \[', "$changelogEntry`n## ["
        } else {
            $newChangelog = "# 变更日志 (Changelog)`n`n$changelogEntry`n$existing"
        }
        Write-Utf8NoBom $changelogPath $newChangelog
        Write-Host "[OK] 已同步更新 CHANGELOG.md" -ForegroundColor Green
    }
}

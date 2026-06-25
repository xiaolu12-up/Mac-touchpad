# MacTouchpad

### 适用于 Windows 系统的 macOS 级触控板手势与鼠标滚轮平滑一体化管理器

[![Version](https://img.shields.io/github/v/release/xiaolu12-up/Mac-touchpad?label=版本&color=7c6cf0)](https://github.com/xiaolu12-up/Mac-touchpad/releases)
[![Platform](https://img.shields.io/badge/平台-Windows-blue?color=0078d4)](https://github.com/xiaolu12-up/Mac-touchpad/releases)
[![Tauri](https://img.shields.io/badge/基于-Tauri%202-red?color=ffc107)](https://tauri.app/)
[![Download](https://img.shields.io/github/downloads/xiaolu12-up/Mac-touchpad/total?label=下载&color=4ade80)](https://github.com/xiaolu12-up/Mac-touchpad/releases)

[English](./README_EN.md) | **中文** | [变更日志](./CHANGELOG.md)

---

## ❓ 为什么选择 MacTouchpad？

现代多任务办公极度依赖触控板手势与高频的页面滚动，然而在 Windows 系统下：
1. **手势功能单一**：Windows 的原生触控板手势配置简单，缺乏 macOS 上广受欢迎的**三指拖移（滑动移动窗口/选中文本）**以及**边缘滑动快速调节音量**等高级手势。
2. **滚轮滚动生硬**：外接普通点击式鼠标的滚轮滚动段落感强、生硬且没有惯性；而市面上大多数“滚轮平滑滚动”软件会无差别地拦截所有滚动事件，导致**触控板的高精度原生惯性滚动受到二次平滑干扰**，产生严重的延迟、卡顿和操作冲突。

**MacTouchpad 完美解决了这些痛点**。它通过 Windows 低级钩子与原始 HID 报文监听，为 Windows 注入了 macOS 级别的多指触控板手势，并内置了高精度的**双速度物理阻尼平滑滚动引擎**。它能精准识别触控板原生滑动与惯性衰减长尾，只对物理点击式鼠标滚轮进行顺滑处理，触控板滚动绝无丝毫卡顿，带来前所未有的流畅手势与滚动体验。

---

## 🚀 核心特性

- **三指拖移 (Three-Finger Drag)** — 在触控板上使用三根手指滑动，即可轻松拖移窗口、选择文本或拖动文件；抬起手指后支持自定义的释放延迟保护，防止因触控板边缘中断操作。
- **多指手势自定义 (Four-Finger Gestures)** — 支持四指上滑（任务视图）、下滑（开始菜单）、左滑/右滑（快速切换虚拟桌面或活动应用）、捏合与张开（显示桌面与激活 Launchpad），可灵活绑定自定义快捷键动作。
- **边缘滑动音量调节 (Left-Edge Volume)** — 在触控板最左侧边缘上下滑动即可调节系统音量，支持反转滑动方向以契合不同使用习惯。
- **鼠标滚轮平滑滚动 (Mouse Smooth Scroll)** — 智能拦截物理鼠标滚轮 tick 事件，通过双速度物理阻尼模型，将生硬的滚动转换为细腻、高帧率的微步长惯性过渡。
- **智能设备旁路过滤 (Smart Device Bypass)** — **独创的双重旁路算法**。结合滚轮物理步长取模（120）与触控板静止时长阈值（1000ms），完美过滤并放行触控板滑动及其松手后的原生惯性滚动阶段，且对高精度无阻尼鼠标飞轮（如罗技 MX Master）直接放行，两者和谐共存。
- **精美控制面板 (Modern Control Panel)** — 基于 Tauri 2 构建的原生桌面应用。支持深色/浅色/系统主题，内置 macOS 同款的矢量微动画演示，手指运行状态一目了然。

---

## 💻 界面截图

| 主界面 - 三指拖移设置 | 滚动与平滑设置 |
| :---: | :---: |
| ![三指拖移](./scratch/preview_drag.png) | ![滚动设置](./scratch/preview_scroll.png) |

---

## 📥 下载和安装

### 系统要求
*   **Windows**：Windows 10 及以上版本（64位）。

### 下载地址
您可以前往 [Releases 页面](https://github.com/xiaolu12-up/Mac-touchpad/releases) 下载：
*   **安装包版本**：`MacTouchpad-v{version}-Windows.msi`
*   **绿色便携版**：`MacTouchpad-v{version}-Windows-Portable.zip`

---

## 🛠️ 开发与构建指南

### 项目结构
```text
mac-touchpad/
├── crates/
│   └── core/          # 核心手势识别与 Windows HID 报文处理库 (Rust)
├── src-tauri/
│   ├── src/           # Tauri 主程序与系统 API 绑定
│   └── tauri.conf.json# Tauri 2 配置文件
└── ui/
    └── index.html     # 控制面板前端页面 (HTML/CSS/JS + SVG 动画)
```

### 本地开发运行
1. 确保已安装 Rust 开发环境（Edition 2021）以及 Windows C++ 构建工具。
2. 克隆项目并进入根目录：
   ```bash
   git clone https://github.com/xiaolu12-up/Mac-touchpad.git
   cd mac-touchpad
   ```
3. 运行开发调试版：
   ```bash
   cargo run --manifest-path src-tauri/Cargo.toml
   # 或者使用 Tauri CLI
   cargo tauri dev
   ```

### 生产打包编译
```bash
cargo tauri build
```
编译生成的安装包将存放在 `src-tauri/target/release/bundle/msi/` 目录下。

---

## ⚖️ 开源协议

本项目采用 [MIT License](LICENSE) 开源协议。

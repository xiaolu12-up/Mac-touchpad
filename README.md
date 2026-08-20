# MacTouchpad

### 适用于 Windows 系统的 macOS 级触控板手势与鼠标滚轮平滑一体化管理器

[![Version](https://img.shields.io/github/v/release/xiaolu12-up/Mac-touchpad?label=版本&color=7c6cf0)](https://github.com/xiaolu12-up/Mac-touchpad/releases)
[![Platform](https://img.shields.io/badge/平台-Windows%2010%2B-blue?color=0078d4)](https://github.com/xiaolu12-up/Mac-touchpad/releases)
[![Tauri](https://img.shields.io/badge/基于-Tauri%202-red?color=ffc107)](https://tauri.app/)
[![License](https://img.shields.io/badge/协议-MIT-green)](LICENSE)
[![Download](https://img.shields.io/github/downloads/xiaolu12-up/Mac-touchpad/total?label=下载量&color=4ade80)](https://github.com/xiaolu12-up/Mac-touchpad/releases)

[English](./README_EN.md) | **中文** | [变更日志](./CHANGELOG.md)

---

## ❓ 为什么选择 MacTouchpad？

现代多任务办公极度依赖触控板手势与高频的页面滚动，然而在 Windows 系统下：
1. **手势功能单一**：Windows 的原生触控板手势配置简单，缺乏 macOS 上广受欢迎的**三指拖移（滑动移动窗口/选中文本）**以及**边缘滑动快速调节音量**等高级手势。
2. **滚轮滚动生硬**：外接普通点击式鼠标的滚轮滚动段落感强、生硬且没有惯性；而市面上大多数“滚轮平滑滚动”软件会无差别地拦截所有滚动事件，导致**触控板的高精度原生惯性滚动受到二次平滑干扰**，产生严重的延迟、卡顿和操作冲突。

**MacTouchpad 完美解决了这些痛点**。它通过 Windows 低级钩子与原始 HID 报文监听，为 Windows 注入了 macOS 级别的多指触控板手势，并内置了高精度的**双速度物理阻尼平滑滚动引擎**。它能精准识别触控板原生滑动与惯性衰减长尾，只对物理点击式鼠标滚轮进行顺滑处理，触控板滚动绝无丝毫卡顿，带来前所未有的流畅手势与滚动体验。

---

## 🚀 核心功能详解

### 1. 触控板手势 (Touchpad Gestures)

| 手势名称 | 操作手法 | 默认动作 | 功能说明 |
| :--- | :--- | :--- | :--- |
| **三指拖移**<br>*(Three-Finger Drag)* | 三指贴合在触控板上滑动 | 移动窗口 / 选中文本 | 模拟鼠标左键按住拖动。支持**释放后重新开始延迟**（200~600ms），在触控板边缘抬指后放回可无缝继续拖移。 |
| **三指轻点**<br>*(Three-Finger Tap)* | 三指快速轻叩触控板表面 | 打开 Windows 搜索 | 触发即时轻点动作，可自定义绑定系统搜索、中键点击或任意快捷键。 |
| **四指滑动**<br>*(Four-Finger Swipe)* | 四指同时向指定方向滑移 | 上滑: 任务视图<br>下滑: 开始菜单<br>左/右滑: 切换虚拟桌面/应用 | 灵敏的方向判定算法，精准识别水平/垂直多指切换操作。 |
| **四指捏合 / 张开**<br>*(Pinch & Spread)* | 拇指与其余三指收拢或张开 | 张开: 显示桌面<br>捏合: 开始菜单 | 类似 macOS 的 Launchpad 和显示桌面手势。 |
| **边缘滑动调节音量**<br>*(Left-Edge Volume)* | **一指在左边缘，一指在中间，松开中间指后滑动** | 调节系统主音量 | 独特防误触设计。在左侧边缘上下滑动即可平滑调节音量并呼出 Windows 屏幕 OSD，支持反转方向。 |

> [!IMPORTANT]
> ⚠️ **重要配置提示**：
> 为避免系统原生手势与 MacTouchpad 产生冲突，请在 **Windows 系统设置 → 蓝牙和其他设备 → 触控板** 中，将 Windows 自带的「三指手势」和「四指手势」均设置为 **“无”**。
>
> ![系统手势设置示例](./scratch/gesture-settings-example.png)

---

### 2. 鼠标滚轮平滑滚动引擎 (Mouse Smooth Scroll)

针对外接物理点击式滚轮（段落滚轮）设计的高性能物理模拟系统：

- **双速度物理阻尼模型 (Dual-Velocity Damping)**：根据拨动滚轮的频率和速度动态调整加速度。慢滚微调精准到像素级，快滚大幅滑行顺畅如丝。
- **独创双重设备旁路算法 (Smart Device Bypass)**：
  - **刻度模数识别**：物理鼠标滚轮的标准事件步长为 120 整数倍；
  - **触屏安全时间窗 (1000ms)**：当检测到触控板处于活跃状态或处于原生惯性衰减阶段时，系统自动旁路放行，绝不干扰触控板的原生高刷滚动。
  - **飞轮鼠标兼容**：对罗技 MX Master 等自带无阻尼飞轮模式的高精度滚轮自动放行。
- **丰富的滚动参数调节**：
  - 滚动速度 (`Speed`) / 平滑阻尼系数 (`Smoothness`) / 惯性衰减率 (`Deceleration`) / 基础缩放步长 (`Base Scale`) / 最大单次位移 (`Max Delta`) / 死区过滤 (`Deadzone`) / 自然滚动反转 (`Natural Scroll`)。

---

### 3. 智能滚动生效策略 (Application Policies)

MacTouchpad 允许您根据当前前台活动窗口灵活控制平滑滚动是否生效：

- 🎮 **全屏应用自动禁用**：运行全屏 3D 游戏或视频播放时，自动暂停平滑滚动，避免游戏内切枪/缩放视角产生阻尼感。
- 🌐 **仅在浏览器生效模式**：一键开启后，平滑滚动仅对已知主流浏览器（Chrome, Edge, Firefox, Brave, Arc, Opera 等）生效。
- 📋 **进程黑名单与白名单管理**：
  - 内置可视化应用选择器，支持一键浏览并提取 `.exe` 的图标、版本与详细信息；
  - 精准基于进程绝对路径与可执行文件名进行链式判定。

---

### 4. 隐蔽开发者调试诊断工具 (Diagnostics Mode)

针对不同品牌触控板硬件差异（Synaptics, ELAN, Goodix, ALPS 等），内置了强大的诊断子系统：

- **激活方式**：进入软件 **「关于」** 页面，在 2 秒内**连续点击版本号文字 5 次**即可展开调试面板（右上角点击 `✕` 随时退出）。
- **零开销运行 (Zero Overhead)**：关闭调试模式时，底层通过单指令原子标志秒级跳过所有格式化与 I/O 操作，日常运行无任何额外 CPU/内存开销。
- **硬件诊断看板**：
  - 实时显示触控板硬件连接状态、`Vendor ID` (厂商 ID)、`Product ID` (产品 ID)、逻辑坐标范围 (`X/Y Range`) 以及管理员权限状态。
  - **实时触点监视 (Live Monitor)**：实时呈现当前手指数量及每个触点的 `(contact_id, x, y)` 物理坐标流。
- **📄 一键导出完整日志**：
  - 点击「导出完整诊断日志 (.log)」可自动将完整软硬件报告与未截断的触控日志保存至 `%APPDATA%\MacTouchpad\logs\` 并在资源管理器中直接高亮定位。

---

## 💻 界面预览

| 三指拖移设置界面 | 滚动平滑与生效策略 |
| :---: | :---: |
| ![三指拖移](./scratch/preview_drag.png) | ![滚动设置](./scratch/preview_scroll.png) |

---

## 📥 下载与安装

### 系统要求
*   **操作系统**：Windows 10 / Windows 11 (64位)。
*   **触控板要求**：支持 Windows Precision Touchpad (PTP) 规范的触控板。

### 下载渠道
前往 [GitHub Releases 页面](https://github.com/xiaolu12-up/Mac-touchpad/releases) 下载最新版本的安装程序：
*   **安装程序 (`.exe` / `.msi`)**：内置全流程简体中文向导，支持自动检测 WebView2 环境并配置开机自启。

---

## ⚙️ 配置与自启动原理

- **配置文件路径**：`%APPDATA%\MacTouchpad\config.json`
- **运行时日志路径**：`%APPDATA%\MacTouchpad\logs\`
- **开机自启动机制**：
  - MacTouchpad 使用 Windows **任务计划程序 (`schtasks.exe`)** 注册自启动任务，拥有管理员权限凭据；
  - 开机自动随系统后台静默启动（附带 `--autostart` 参数，启动后窗口保持隐藏），**无需每次弹出 Windows UAC 用户账户控制提示**。

---

## 🛠️ 本地开发与构建

### 架构设计
```text
mac-touchpad/
├── crates/
│   └── core/          # 核心手势识别、Win32 LowLevel Hook 与 PTP 报文重组引擎 (Rust)
├── src-tauri/
│   ├── src/           # Tauri 2 主进程、IPC 命令分发与系统托盘管理
│   ├── nsis/          # NSIS 安装包多语言与汉化脚本
│   └── tauri.conf.json# Tauri 2 配置文件
└── ui/
    └── index.html     # 单文件响应式前端 (HTML5 / CSS3 动效 / 原生 JS)
```

### 开发环境准备
1. 安装 [Rust](https://www.rust-lang.org/) (Edition 2021) 与 Visual Studio C++ 构建工具。
2. 安装 [Node.js](https://nodejs.org/) (可选，用于 Tauri CLI)。

### 编译与运行
```bash
# 1. 克隆代码仓库
git clone https://github.com/xiaolu12-up/Mac-touchpad.git
cd mac-touchpad

# 2. 运行核心库单元测试
cargo test -p mac-touchpad-core

# 3. 启动开发调试版
cargo run --manifest-path src-tauri/Cargo.toml

# 4. 构建发布安装包
cargo tauri build
```

---

## 🤝 参与贡献与问题反馈

欢迎提交 Issue 或 Pull Request！
- **问题反馈**：[GitHub Issues](https://github.com/xiaolu12-up/Mac-touchpad/issues) / [Gitee Issues](https://gitee.com/lu52/Mac-touchpad/issues)
- 提交手势异常时，建议在「关于」页面连点 5 次版本号，导出 `.log` 诊断文件一并附上，以便快速分析设备型号与 HID 报文流。

---

## ⚖️ 开源协议

本项目采用 [MIT License](LICENSE) 授权开源。

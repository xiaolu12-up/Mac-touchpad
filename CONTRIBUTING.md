# 贡献指南 (Contributing Guide)

感谢您有兴趣为 MacTouchpad 做出贡献！无论是修复 Bug、改进文档，还是添加新功能，我们都非常欢迎您的参与。

为了确保顺畅的协作体验，请在提交代码前阅读以下指南。

---

## 🛠️ 开发环境配置

本软件基于 **Tauri 2** 和 **Rust** 构建，前端采用纯 HTML/CSS/JS，因此开发配置非常简单。

1. **安装 Rust**：
   * 请前往 [Rust 官网](https://www.rust-lang.org/) 安装 Rust 编译链（推荐 Stable 版本）。
   * 确保 `cargo` 可以在命令行中运行。
2. **安装 Windows C++ 构建工具**：
   * 需要 Visual Studio Build Tools 并勾选 "C++ 桌面开发" 组件。
3. **安装 Tauri CLI**（推荐）：
   * 在终端中执行以下命令安装 Tauri 命令行工具：
     ```bash
     cargo install tauri-cli --version "^2.0.0"
     ```

---

## 🚀 编译与调试命令

在项目根目录下，您可以使用以下命令：

### 本地开发运行 (带热重载/调试输出)：
```bash
cargo tauri dev
```
此命令会编译 Rust 核心及 Tauri 壳程序，并自动监测 `ui/` 和 `src-tauri/` 的代码变动。

### 静态分析与检查：
```bash
cargo check
# 运行 linter 检查代码风格
cargo clippy
```

### 生产打包编译：
```bash
cargo tauri build
```
编译生成的 MSI 安装包和免安装绿色版压缩包将存放于 `src-tauri/target/release/bundle/` 下。

---

## 📂 项目结构简介

* `crates/core`：核心手势识别与 Windows HID 原始数据/钩子处理逻辑 (Rust 纯逻辑库)。
* `src-tauri`：Tauri 主程序、系统托盘菜单、开机自启系统绑定、与前端的数据通信。
* `ui`：前端控制面板页面（仅包含单文件 `index.html`，使用纯 CSS 和 vanilla JS 处理交互与微动画演示）。

---

## 🤝 提交贡献流程

1. **Fork 本仓库**：将本项目 Fork 到您自己的 GitHub 账号下。
2. **创建分支**：基于 `main` 分支创建一个新的功能/修复分支：
   ```bash
   git checkout -b feature/your-feature-name
   # 或
   git checkout -b fix/bug-description
   ```
3. **编写代码**：
   * 保持代码风格的一致性。
   * 确保所有的代码都经过了 `cargo check` 和 `cargo clippy` 的检查。
4. **提交 commit**：
   * 推荐使用清晰的 Commit 消息（如：`fix: 修复了自启动未生效的问题`，`feat: 新增四指轻扫自定义动作`）。
5. **发起 Pull Request**：
   * 提交分支到您的 Fork 仓库，然后向主仓库的 `main` 分支发起 Pull Request。
   * 请详细填写 Pull Request 模板中的各项内容。

---

## 📝 行为准则

在参与本项目的所有互动时，请遵守我们的 [Code of Conduct](CODE_OF_CONDUCT.md)。尊重他人，保持友善和建设性的交流。

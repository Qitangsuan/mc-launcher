# MC Launcher

一个用 **Rust + Tauri 2 + Vue 3** 写的 Minecraft 启动器。

启动链路自己实现（不依赖第三方启动器库）：版本 JSON 解析与 `inheritsFrom` 合并、
`rules` 过滤、并发下载 + SHA1 校验、natives 解压、启动参数拼接、Java 运行时管理。

## 功能

| 功能 | 状态 |
|---|---|
| 版本安装（正式版 / 快照分类、实时进度） | ✅ |
| 启动游戏（完整参数拼接、rules 过滤） | ✅ |
| Fabric / Quilt 加载器 | ✅ Fabric |
| Forge / NeoForge | ✅ |
| 离线登录（UUID 与 Java `nameUUIDFromBytes` 逐字节一致） | ✅ |
| 多账号管理（切换 / 删除） | ✅ |
| Java 自动检测 + 自动下载（Adoptium Temurin） | ✅ |
| 游戏日志接入界面 + 落盘 | ✅ |
| 微软正版登录 | ⚠️ 代码完成，需自备 Azure `client_id`（见下） |
| 下载断点续传（SHA1 命中跳过） | ✅ |
| 下载失败重试（3 次指数退避） | ✅ |

## 截图

> 待补充

## 安装

### 直接使用

从 Releases 下载安装包：

- `MC Launcher_x.y.z_x64-setup.exe` —— NSIS 安装包（推荐）
- `MC Launcher_x.y.z_x64_en-US.msi` —— MSI

安装包是 **per-machine** 模式，会装到 `C:\Program Files\MC Launcher\`。

### 从源码构建

环境要求：

- **Rust**（MSVC 工具链）
- **Visual Studio Build Tools**（勾选「使用 C++ 的桌面开发」）
- **Node.js 20+ / pnpm**
- **WebView2 Runtime**（Win11 自带）

```bash
pnpm install
pnpm tauri build
```

产物在 `src-tauri/target/release/bundle/`。

### 开发

```bash
pnpm install
pnpm tauri dev
```

## 数据位置

| 内容 | 路径 |
|---|---|
| 游戏本体 | `%APPDATA%\.minecraft` |
| 启动器配置 / 账号 / 自动下载的 Java | `%APPDATA%\mc-launcher` |
| WebView2 缓存 | `%LOCALAPPDATA%\com.qitangsuan.mclauncher` |

## 微软正版登录

代码已完整实现（Device Code Flow → Xbox Live → XSTS → Minecraft Token → Profile，
含 refresh token 续期与 XSTS 错误码翻译），但需要一个 **Azure 应用的 client_id**：

1. 在 [Azure Portal](https://portal.azure.com) 注册应用
2. 受支持的账户类型选「任何组织目录中的账户和个人 Microsoft 账户」
3. 「身份验证」→ 允许公共客户端流 → 是
4. 把「应用程序(客户端) ID」填进启动器的账号面板

个人微软账号现在无法直接注册 Azure 应用（需要 Entra 目录 / 订阅），
所以这一步默认留空。

## 已知问题

- **Windows 11 SmartScreen 隔离**：非系统目录里的新签名 exe 会被 SmartScreen
  用 AppContainer 沙箱运行，导致无法写 `%APPDATA%` 而崩溃。
  这是把程序装进 `Program Files` 的原因。
- 1.12.2 及更早版本用 legacy assets，需要映射到 `assets/virtual/legacy/`，尚未实现
  → 能启动但没声音没材质。
- 没做下载取消 / 暂停。

## 项目结构

```
src/                      Vue 3 前端
src-tauri/src/
  core/                   核心逻辑（与 Tauri 无关，可单独测试）
    version.rs            版本 JSON 解析、inheritsFrom 合并、rules 过滤
    download.rs           并发下载、SHA1 校验、重试
    prepare.rs            下载任务生成
    args.rs               启动参数拼接
    natives.rs            natives 解压
    java.rs / java_download.rs   Java 检测与下载
    fabric.rs / forge.rs  加载器安装
    account.rs            账号存储、离线 UUID
    microsoft.rs          微软登录
  commands/               Tauri 命令
examples/                 无 GUI 的安装实测
```

## 测试

```bash
cd src-tauri
cargo test
```

覆盖 rules 过滤、classpath 组装、离线 UUID（与 Java 逐字节对齐）、
版本号排序、assets 路径生成等纯函数。

## 许可证

[MIT](LICENSE)

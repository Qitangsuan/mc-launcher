# MC Launcher

一个用 Rust + Tauri 2 + Vue 3 写的 Minecraft 启动器（Windows）。

## 环境要求

| 依赖 | 说明 |
|---|---|
| Rust (MSVC 工具链) | Tauri 编译必需 |
| Visual Studio Build Tools | Windows 上链接 Rust 必需，勾选「使用 C++ 的桌面开发」 |
| Node.js 20+ / pnpm | 前端构建 |
| WebView2 Runtime | Win11 自带，Win10 可能需手动装 |

### 安装 Rust

```powershell
winget install --id Rustlang.Rustup -e --accept-package-agreements --accept-source-agreements
```

装完新开终端验证：

```powershell
rustc --version
cargo --version
```

### 安装 VS Build Tools（必须以管理员身份运行）

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools -e --override "--wait --quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

## 开发命令

```powershell
pnpm install          # 装前端依赖
pnpm dev              # 只跑前端（浏览器里看界面，无 Rust 后端）
pnpm tauri dev        # 跑完整应用（需要 Rust 环境）
pnpm tauri build      # 打包 exe / msi
```

## 目录结构

```
src/                       Vue 前端
  App.vue                  主界面
  style.css                样式
src-tauri/                 Rust 后端
  src/main.rs              入口
  src/lib.rs               注册命令
  src/commands/version.rs  scan_versions / launch_game
  src/core/paths.rs        .minecraft 目录定位
  src/core/manifest.rs     版本扫描与解析
  tauri.conf.json          Tauri 配置
scripts/gen_icon.py        图标生成
```

## 进度

- [x] 阶段 1：项目骨架、前端界面、版本扫描
- [ ] 阶段 2：资源下载（libraries / assets / client.jar，SHA1 校验）
- [ ] 阶段 3：参数拼接 + 启动 Java 进程
- [ ] 阶段 4：离线登录 + 微软正版登录
- [ ] 阶段 5：Java 运行时检测
- [ ] 阶段 6：Fabric / Forge 支持

## 已知坑

- 游戏目录含中文时，classpath 参数必须整体加引号（代码里统一处理，别手拼）。
- 启动游戏优先用 `java.exe` 而不是 `javaw.exe`，否则拿不到崩溃日志。

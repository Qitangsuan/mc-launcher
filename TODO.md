# 待办

## 下次继续：下载安装完整实测

用户指定的最后一项。三个场景：

1. **装新版本**（建议 1.12.2，约 100 MB）——进度条推进是否正常、装完能否启动
2. **重复安装同一版本**——应秒完成（文件存在 + SHA1 命中 → 跳过）
3. **断网装一半**——报错是否可读，恢复网络后能否续上（可选）

## 已完成

- [x] 项目骨架（Vue 3 + Vite + Tauri 2，Rust 后端自己写）
- [x] 版本安装（正式版/快照分类 + 进度条）
- [x] SHA1 校验 + 8 并发下载 + 已存在跳过
- [x] 启动游戏（参数拼接、rules 过滤、inheritsFrom 合并、natives 解压）
- [x] 离线登录（UUID 与 Java nameUUIDFromBytes 逐字节验证一致）
- [x] 多账号（切换 / 删除 / 旧单账号格式自动迁移）
- [x] Java 自动下载（Adoptium Temurin JRE，按版本要求选）
- [x] Fabric 支持
- [x] **Forge / NeoForge 支持**（四个坑已修，见下）
- [x] 游戏日志接界面（管道捕获 + 落盘 launcher-latest.log + 自动滚动）
- [x] mods 文件夹按钮
- [x] 打包 exe / msi / nsis

## Forge 踩过的坑（已修，留档）

1. **查版本**：不能用 `promotions_slim.json`（只收 latest/recommended，1.20.1 没条目），
   要拉 `maven-metadata.xml` 全量清单再按 `{游戏版本}-` 前缀过滤
2. **installer 前置**：`.minecraft/launcher_profiles.json` 必须存在，否则报
   "There is no Minecraft launcher profile" 直接退出 → 安装前自动补最小文件
3. **classpath 冲突**：Forge 系不能把原版 client.jar 塞进 classpath，
   它的 patched jar 已在 libraries 里，两个都带 net.minecraft 会撞
   `Module minecraft contains package net.minecraft.server` → forge 系传 None
4. **Java 版本**：installer 的 Java 要按游戏版本 JSON 的 javaVersion 选，别写死

## 环境坑（与本项目无关但值得记）

- **DSH 在 Session 6，用户桌面在 Session 2**：从 DSH 的 pwsh 启动的 GUI 程序
  无法创建窗口（`拒绝访问 os error 5` / `WebView2 ERROR_BUSY`）。
  绕法：用户手动双击 exe，或先 `pnpm dev` 起前端服务再让用户双击。
- **`& explorer.exe <路径>` 会永久阻塞**：explorer 是 GUI 程序不返回退出码，
  PowerShell 同步调用会一直挂着（踩过一次，挂了 2 小时）。
  必须用 `Start-Process explorer.exe -ArgumentList "<路径>"`。
- **WebView2 157.0.4341.60 有崩溃 bug**（msedge_elf.dll 断点异常）。
  winget 静默安装 158 会「报成功但没装上」（Program Files 需要管理员），
  必须手动跑 installer 并点 UAC。现在是 158.0.4370.33。

## 搁置

- [ ] 微软正版登录 —— 代码完成但微软政策封死个人账号
  - 个人账号无 Entra 目录 → 不能注册应用
  - M365 开发者计划审核不过
  - 唯一正规路径是 Azure 免费账户绑国际信用卡
  - 代码在 `core/microsoft.rs`，将来填 client_id 就能用

## 2026-11-25 代码审查 + 测试补齐

子代理 `agent-tests` 补了 **37 个单元测试**（6 个文件，`cargo test` 全绿），
`agent-review` 通读了 20 个文件约 2900 行，产出问题清单。

### 已修（严重）

| 问题 | 修法 |
|---|---|
| `java::pick_for` 兜底 `all.last()`：本机只有低版本 Java 时会返回它，**跳过自动下载**，用错 Java 跑游戏崩 | 删掉兜底，低于 required 返回 None |
| `asset_summary.failed` 从不检查：资源下载失败静默跳过，显示「安装完成」但没声音没材质 | 失败即报错并列出前 3 条 |
| NeoForge 版本用字符串排序：`21.1.100` 排在 `21.1.66` 前面，选错最新版 | 改用 `version_key` 数字段排序 |
| `open_url` 命令注入：`cmd /C start "" <url>` 直接拼外部 API 返回的 URL | 只允许 http/https + 拒绝 `& \| ^ < > % "` |
| 路径穿越：`version_id` / `loader_version` 未校验就拼路径 | 新增 `paths::validate_id`，四处命令入口全部校验 |

### 已修（中等）

- `download.rs`：sha1 为空的任务永远不跳过 → Fabric 库每次启动重下；现在按「非空文件」跳过
- `java::probe`：只取 stderr 第一行，设了 `JAVA_TOOL_OPTIONS` 时全盘判失败；改为找含 `version "` 的行
- `forge::list_neoforge_versions`：两段版本号（`1.21`）被误判为格式错；按 `21.0` 处理
- `forge::install`：重装同一版本时「目录没新增」被当成失败；改为允许已存在
- `account::remove`：删掉当前账号之前的账号时，当前账号索引漂移
- `install_version`：只看 json 存在不校验可解析，半截 JSON 会永久卡死
- `fabric::install`：远程响应里的 id 直接当目录名，补 id 校验

### 记录了但没修（评估后收益低或改动大）

- `windows_version_string` 硬编码 Win10 版本号（要加 winreg 依赖，MC 的 Windows 规则几乎不用 os.version）
- 下载器无重试；`sha1_file` 用同步读阻塞 tokio；`regex` 每次 rule 都重新编译
- asset index 下载后不校验 sha1；`java_download` 的 zip 防护可被 `a/../../b` 绕过
- `find_all` 一次启动扫描 3 次 + 每次 `java -version`（启动白等几秒）
- 各处 `reqwest::Client::new()` 无超时
- 游戏进程无法停止（没保存 Child 句柄）
- `${clientid}` / `${auth_xuid}` 恒空（微软账号缺 xuid）
- `expand` 按 HashMap 顺序替换，替换值含 `${}` 会被二次替换
- 日志固定写 `launcher-latest.log`，同时跑两个版本会互相覆盖



<script setup lang="ts">
import { ref, computed, onMounted, nextTick, watch } from "vue";

interface VersionInfo {
  id: string;
  gameVersion: string;
  loader: string;
  javaMajor: number | null;
}

interface RemoteVersion {
  id: string;
  type: string;
  releaseTime: string;
}

interface DownloadProgress {
  stage: string;
  done: number;
  total: number;
}

interface Account {
  username: string;
  uuid: string;
  accessToken: string;
  userType: string;
  kind: string;
}

interface AccountStore {
  accounts: Account[];
  current: number;
}

interface DeviceCode {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
}

interface LoaderVersions {
  latest: string | null;
  recommended: string | null;
  all: string[];
}

type Page = "launch" | "install" | "settings";

const page = ref<Page>("launch");

const versions = ref<VersionInfo[]>([]);
const selectedId = ref("");
const loading = ref(true);
const launching = ref(false);
const status = ref("正在扫描本地版本…");
const gameDir = ref("");
const logs = ref<string[]>([]);

const remoteVersions = ref<RemoteVersion[]>([]);
const remoteLoading = ref(false);
const installingId = ref("");
const progress = ref<DownloadProgress | null>(null);

const showPicker = ref(false);
const showLogs = ref(false);

const accountStore = ref<AccountStore>({ accounts: [], current: 0 });
const account = computed(
  () => accountStore.value.accounts[accountStore.value.current] ?? null
);
const nameInput = ref("");

const msStep = ref<"idle" | "needClientId" | "waiting">("idle");
const msCode = ref<DeviceCode | null>(null);
const clientIdInput = ref("");

const isTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/**
 * 微软正版登录开关。
 *
 * 后端（core/microsoft.rs）实现是完整的：Device Code Flow → Xbox Live → XSTS
 * → Minecraft Token → Profile，含 refresh token 续期和 XSTS 错误码翻译。
 *
 * 但注册 Azure 应用需要一个 Entra 目录，个人微软账号目前拿不到，
 * 所以前端默认关闭。等你有 client_id 了，把这里改成 true 就能用。
 */
const MS_LOGIN_ENABLED = false;

const selected = computed(
  () => versions.value.find((v) => v.id === selectedId.value) ?? null
);

const groupedRemote = computed(() => {
  const releases = remoteVersions.value.filter((v) => v.type === "release");
  const others = remoteVersions.value.filter((v) => v.type !== "release");
  const groups: { label: string; items: RemoteVersion[] }[] = [];
  if (releases.length) {
    groups.push({ label: `正式版 · ${releases.length}`, items: releases });
  }
  if (others.length) {
    groups.push({ label: `快照与旧版 · ${others.length}`, items: others });
  }
  return groups;
});

const installedCount = computed(() => versions.value.length);

const pct = computed(() => {
  const p = progress.value;
  if (!p || p.total <= 0) return 0;
  return Math.min(100, Math.round((p.done / p.total) * 100));
});

const logEl = ref<HTMLElement | null>(null);

watch(
  () => logs.value.length,
  async () => {
    await nextTick();
    if (logEl.value) logEl.value.scrollTop = logEl.value.scrollHeight;
  }
);

function pushLog(msg: string) {
  const t = new Date().toLocaleTimeString("zh-CN", { hour12: false });
  logs.value.push(`[${t}] ${msg}`);
  if (logs.value.length > 400) logs.value.shift();
}

const MOCK: VersionInfo[] = [
  { id: "1.21.4", gameVersion: "1.21.4", loader: "vanilla", javaMajor: 21 },
  {
    id: "1.20.1-forge-47.2.0",
    gameVersion: "1.20.1",
    loader: "forge",
    javaMajor: 17,
  },
  { id: "1.12.2", gameVersion: "1.12.2", loader: "vanilla", javaMajor: 8 },
];

const MOCK_REMOTE: RemoteVersion[] = [
  { id: "1.21.4", type: "release", releaseTime: "" },
  { id: "1.21.3", type: "release", releaseTime: "" },
  { id: "1.20.1", type: "release", releaseTime: "" },
  { id: "25w03a", type: "snapshot", releaseTime: "" },
];

async function loadVersions() {
  loading.value = true;
  try {
    if (isTauri) {
      const { invoke } = await import("@tauri-apps/api/core");
      const res = await invoke<{ gameDir: string; versions: VersionInfo[] }>(
        "scan_versions"
      );
      versions.value = res.versions;
      gameDir.value = res.gameDir;
      status.value = `已找到 ${res.versions.length} 个版本`;
    } else {
      versions.value = MOCK;
      gameDir.value = "%APPDATA%\\.minecraft";
      status.value = "浏览器预览模式（未连接 Rust 后端）";
    }
    if (versions.value.length && !selectedId.value) {
      selectedId.value = versions.value[0].id;
    }
  } catch (e) {
    status.value = "扫描失败";
    pushLog("扫描失败: " + String(e));
  } finally {
    loading.value = false;
  }
}

async function loadAccounts() {
  if (!isTauri) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    accountStore.value = await invoke<AccountStore>("get_accounts");
  } catch (e) {
    pushLog("读取账号失败: " + String(e));
  }
}

async function doLoginOffline() {
  if (!isTauri) {
    pushLog("浏览器预览模式不能登录");
    return;
  }
  const name = nameInput.value.trim();
  if (!name) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const acc = await invoke<Account>("login_offline", { username: name });
    await loadAccounts();
    nameInput.value = "";
    pushLog(`离线登录：${acc.username}（UUID ${acc.uuid}）`);
  } catch (e) {
    pushLog("登录失败: " + String(e));
  }
}

async function doLogout() {
  if (!isTauri) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("logout");
    await loadAccounts();
    pushLog("已退出登录");
  } catch (e) {
    pushLog("退出失败: " + String(e));
  }
}

async function switchAccount(index: number) {
  if (!isTauri) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("switch_account", { index });
    await loadAccounts();
  } catch (e) {
    pushLog("切换账号失败: " + String(e));
  }
}

async function removeAccount(index: number) {
  if (!isTauri) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("remove_account", { index });
    await loadAccounts();
    pushLog("账号已删除");
  } catch (e) {
    pushLog("删除账号失败: " + String(e));
  }
}

async function openMods() {
  if (!isTauri) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const path = await invoke<string>("open_mods_dir");
    pushLog("已打开 mods 目录：" + path);
  } catch (e) {
    pushLog("打开 mods 目录失败: " + String(e));
  }
}

async function startMsLogin() {
  if (!isTauri) {
    pushLog("浏览器预览模式不能登录");
    return;
  }
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const cid = await invoke<string>("get_client_id");
    if (!cid) {
      msStep.value = "needClientId";
      return;
    }
    await requestMsCode();
  } catch (e) {
    pushLog("微软登录失败: " + String(e));
  }
}

async function requestMsCode() {
  const { invoke } = await import("@tauri-apps/api/core");
  const code = await invoke<DeviceCode>("ms_request_code");
  msCode.value = code;
  msStep.value = "waiting";
  pushLog(`微软登录：打开 ${code.verificationUri} 输入 ${code.userCode}`);
  await invoke("open_url", { url: code.verificationUri }).catch(() => {});

  try {
    const acc = await invoke<Account>("ms_complete_login", {
      deviceCode: code.deviceCode,
      interval: code.interval,
      expiresIn: code.expiresIn,
    });
    await loadAccounts();
    msStep.value = "idle";
    msCode.value = null;
    pushLog(`微软登录成功：${acc.username}（${acc.uuid}）`);
    status.value = `已登录 ${acc.username}`;
  } catch (e) {
    pushLog("微软登录失败: " + String(e));
    msStep.value = "idle";
    msCode.value = null;
  }
}

async function openMsUrl() {
  if (!msCode.value) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("open_url", { url: msCode.value.verificationUri }).catch(
    () => {}
  );
}

async function saveClientId() {
  if (!isTauri) return;
  const id = clientIdInput.value.trim();
  if (!id) return;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("set_client_id", { clientId: id });
    pushLog("client_id 已保存");
    await requestMsCode();
  } catch (e) {
    pushLog("启动微软登录失败: " + String(e));
  }
}

async function openInstallPage() {
  page.value = "install";
  if (!isTauri) {
    remoteVersions.value = MOCK_REMOTE;
    return;
  }
  if (remoteVersions.value.length) return;
  remoteLoading.value = true;
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    remoteVersions.value = await invoke<RemoteVersion[]>(
      "list_remote_versions"
    );
    const releases = remoteVersions.value.filter(
      (v) => v.type === "release"
    ).length;
    pushLog(
      `版本清单已加载：正式版 ${releases} 个，快照 ${remoteVersions.value.length - releases} 个`
    );
  } catch (e) {
    pushLog("获取版本清单失败: " + String(e));
  } finally {
    remoteLoading.value = false;
  }
}

async function install(id: string) {
  if (!isTauri || installingId.value) return;
  installingId.value = id;
  status.value = `正在安装 ${id}…`;
  pushLog(`开始安装 ${id}`);
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const count = await invoke<number>("install_version", { versionId: id });
    pushLog(`安装完成：${id}（下载 ${count} 个文件）`);
    status.value = `安装完成 ${id}`;
    await loadVersions();
    selectedId.value = id;
    page.value = "launch";
  } catch (e) {
    pushLog("安装失败: " + String(e));
    status.value = "安装失败";
  } finally {
    installingId.value = "";
    progress.value = null;
  }
}

async function installFabric() {
  const v = selected.value;
  if (!v || installingId.value || !isTauri) return;
  installingId.value = v.id;
  status.value = `正在给 ${v.id} 装 Fabric…`;
  pushLog(`给 ${v.gameVersion} 装 Fabric`);
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const newId = await invoke<string>("install_fabric", {
      gameVersion: v.gameVersion,
    });
    pushLog(`Fabric 安装完成：${newId}`);
    await loadVersions();
    selectedId.value = newId;
  } catch (e) {
    pushLog("Fabric 安装失败: " + String(e));
  } finally {
    installingId.value = "";
    progress.value = null;
  }
}

async function installForge() {
  const v = selected.value;
  if (!v || installingId.value || !isTauri) return;
  installingId.value = v.id;
  status.value = `正在给 ${v.id} 装 Forge…`;
  pushLog(`查询 ${v.gameVersion} 的加载器版本`);

  let useNeo = false;
  let versionsData: LoaderVersions;

  try {
    const { invoke } = await import("@tauri-apps/api/core");
    versionsData = await invoke<LoaderVersions>("list_forge_versions", {
      gameVersion: v.gameVersion,
    });
  } catch (e) {
    pushLog("Forge 没有这个版本，改试 NeoForge：" + String(e));
    useNeo = true;
    const { invoke } = await import("@tauri-apps/api/core");
    versionsData = await invoke<LoaderVersions>("list_neoforge_versions", {
      gameVersion: v.gameVersion,
    });
  }

  const target =
    versionsData.recommended ??
    versionsData.latest ??
    versionsData.all[versionsData.all.length - 1];

  pushLog(`准备安装 ${useNeo ? "NeoForge" : "Forge"} ${target}`);

  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const newId = await invoke<string>("install_forge", {
      gameVersion: v.gameVersion,
      loaderVersion: target,
      neoforge: useNeo,
    });
    pushLog(`安装完成：${newId}`);
    await loadVersions();
    selectedId.value = newId;
  } catch (e) {
    pushLog("Forge 安装失败: " + String(e));
  } finally {
    installingId.value = "";
    progress.value = null;
  }
}

async function launch() {
  const v = selected.value;
  if (!v || launching.value) return;
  launching.value = true;
  showLogs.value = true;
  status.value = `正在启动 ${v.id}…`;
  pushLog(
    `准备启动 ${v.id}（账号 ${account.value?.username ?? "Player"}，Java ${v.javaMajor ?? "?"}）`
  );
  try {
    if (isTauri) {
      const { invoke } = await import("@tauri-apps/api/core");
      const report = await invoke<{
        javaPath: string;
        javaMajor: number;
        downloaded: number;
        skipped: number;
        nativesExtracted: number;
      }>("launch_game", { versionId: v.id });
      pushLog(`使用 Java ${report.javaMajor}：${report.javaPath}`);
      pushLog(
        `下载 ${report.downloaded} 个，跳过 ${report.skipped} 个，解压 natives ${report.nativesExtracted} 个`
      );
    } else {
      await new Promise((r) => setTimeout(r, 800));
    }
    pushLog("游戏进程已拉起");
    status.value = "游戏已启动";
  } catch (e) {
    pushLog("启动失败: " + String(e));
    status.value = "启动失败";
  } finally {
    launching.value = false;
    progress.value = null;
  }
}

onMounted(async () => {
  await loadVersions();
  await loadAccounts();

  if (isTauri) {
    try {
      const { listen } = await import("@tauri-apps/api/event");
      await listen<DownloadProgress>("download-progress", (event) => {
        progress.value = event.payload;
      });
      await listen<{ message: string }>("launcher-log", (event) => {
        pushLog(event.payload.message);
      });
      await listen<{ line: string }>("game-log", (event) => {
        pushLog(event.payload.line);
      });
      await listen<{ version: string; code: number }>("game-exit", (event) => {
        const { version, code } = event.payload;
        pushLog(`游戏已退出（${version}，退出码 ${code}）`);
        status.value = code === 0 ? "游戏正常退出" : `游戏退出，代码 ${code}`;
      });
    } catch (e) {
      pushLog("事件监听注册失败: " + String(e));
    }
  }
});
</script>

<template>
  <div class="app">
    <!-- 顶栏 -->
    <header class="topbar">
      <div class="brand">
        <span class="brand-mark">M</span>
        <span>MC Launcher</span>
      </div>
      <div class="topbar-actions">
        <button class="btn" @click="page = 'settings'">
          {{ account ? account.username : "未登录" }}
        </button>
      </div>
    </header>

    <div class="body-row">
      <!-- 左侧导航 -->
      <nav class="sidenav">
        <button
          class="nav-item"
          :class="{ active: page === 'launch' }"
          title="启动"
          @click="page = 'launch'"
        >
          <svg viewBox="0 0 24 24"><polygon points="6 4 20 12 6 20" /></svg>
        </button>
        <button
          class="nav-item"
          :class="{ active: page === 'install' }"
          title="安装版本"
          @click="openInstallPage"
        >
          <svg viewBox="0 0 24 24">
            <path d="M12 3v12" />
            <path d="m7 10 5 5 5-5" />
            <path d="M5 21h14" />
          </svg>
        </button>
        <button
          class="nav-item"
          :class="{ active: page === 'settings' }"
          title="设置"
          @click="page = 'settings'"
        >
          <svg viewBox="0 0 24 24">
            <circle cx="12" cy="12" r="3" />
            <path
              d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
            />
          </svg>
        </button>
      </nav>

      <!-- 页面主体 -->
      <main class="stage">
        <!-- ─── 启动页 ─── -->
        <template v-if="page === 'launch'">
          <div v-if="loading" class="empty-hero">正在扫描本地版本…</div>

          <div v-else-if="!selected" class="empty-hero">
            还没有安装任何版本<br />
            点左边「安装版本」开始
          </div>

          <template v-else>
            <div class="version-hero">
              <div class="hero-mark">⛏</div>
              <h1 class="hero-title">{{ selected.id }}</h1>
              <div class="hero-sub">
                <span class="hero-badge">
                  <span class="dot"></span>{{ selected.loader }}
                </span>
                <span class="sep"></span>
                <span>Java {{ selected.javaMajor ?? "?" }}</span>
                <span class="sep"></span>
                <span>游戏 {{ selected.gameVersion }}</span>
              </div>
            </div>

            <div class="launch-wrap">
              <div v-if="progress && progress.total > 0" class="progress">
                <div class="progress-head">
                  <span class="progress-stage">{{ progress.stage }}</span>
                  <span class="progress-num"
                    >{{ progress.done }} / {{ progress.total }} ·
                    {{ pct }}%</span
                  >
                </div>
                <div class="progress-track">
                  <div
                    class="progress-fill"
                    :style="{ width: pct + '%' }"
                  ></div>
                </div>
              </div>

              <button
                class="btn-launch"
                :disabled="launching || !!installingId"
                @click="launch"
              >
                {{ launching ? "启动中…" : "启动游戏" }}
              </button>
              <div class="launch-hint">
                {{ account ? `以 ${account.username} 身份启动` : "当前离线模式" }}
              </div>
            </div>

            <div class="action-row">
              <div class="version-picker">
                <button class="btn" @click="showPicker = !showPicker">
                  切换版本
                </button>
                <div v-if="showPicker" class="picker-menu">
                  <div v-if="!versions.length" class="picker-empty">
                    没有已安装版本
                  </div>
                  <div
                    v-for="v in versions"
                    :key="v.id"
                    class="picker-item"
                    :class="{ active: v.id === selectedId }"
                    @click="
                      selectedId = v.id;
                      showPicker = false;
                    "
                  >
                    <span class="v-name">{{ v.id }}</span>
                    <span class="tag">{{ v.loader }}</span>
                  </div>
                </div>
              </div>

              <button
                v-if="selected.loader === 'vanilla'"
                class="btn"
                :disabled="!!installingId"
                @click="installFabric"
              >
                装 Fabric
              </button>
              <button
                v-if="selected.loader === 'vanilla'"
                class="btn"
                :disabled="!!installingId"
                @click="installForge"
              >
                装 Forge
              </button>
              <button class="btn" @click="openMods">mods</button>
            </div>
          </template>
        </template>

        <!-- ─── 安装页 ─── -->
        <template v-else-if="page === 'install'">
          <div class="page-head">
            <h1 class="page-title">安装版本</h1>
            <p class="page-desc">从 Mojang 官方清单选择要安装的游戏版本</p>
          </div>

          <div v-if="remoteLoading" class="empty-hero">获取版本清单…</div>
          <div v-else-if="!remoteVersions.length" class="empty-hero">
            没拿到版本清单
          </div>
          <div v-else class="install-list">
            <div v-for="g in groupedRemote" :key="g.label">
              <div class="group-head">{{ g.label }}</div>
              <div
                v-for="r in g.items"
                :key="r.id"
                class="install-item"
                :class="{ active: r.id === installingId }"
                @click="install(r.id)"
              >
                <div class="install-main">
                  <span class="install-name">{{ r.id }}</span>
                  <span class="tag">{{
                    r.type === "release" ? "正式" : "快照"
                  }}</span>
                </div>
                <span class="install-action">{{
                  r.id === installingId ? "安装中…" : "安装 →"
                }}</span>
              </div>
            </div>
          </div>
        </template>

        <!-- ─── 设置页 ─── -->
        <template v-else>
          <div class="page-head">
            <h1 class="page-title">设置</h1>
            <p class="page-desc">账号、游戏目录与启动选项</p>
          </div>

          <div class="settings-stack">
            <!-- 账号 -->
            <section class="card">
              <div class="card-head">
                <h2 class="card-title">账号</h2>
                <span class="card-sub"
                  >{{ accountStore.accounts.length }} 个已保存</span
                >
              </div>

              <ul v-if="accountStore.accounts.length" class="account-list">
                <li
                  v-for="(acc, i) in accountStore.accounts"
                  :key="acc.uuid"
                  class="account-item"
                  :class="{ active: i === accountStore.current }"
                  @click="switchAccount(i)"
                >
                  <span class="acc-name">{{ acc.username }}</span>
                  <span class="acc-kind">{{
                    acc.kind === "microsoft" ? "正版" : "离线"
                  }}</span>
                  <button
                    class="acc-del"
                    title="删除"
                    @click.stop="removeAccount(i)"
                  >
                    ×
                  </button>
                </li>
              </ul>

              <template v-if="MS_LOGIN_ENABLED && msStep === 'waiting'">
                <p class="pop-hint">在浏览器页面输入这个代码：</p>
                <div class="ms-code">{{ msCode?.userCode }}</div>
                <button class="btn" @click="openMsUrl">重新打开登录页</button>
                <p class="pop-hint" style="text-align: center">
                  等待你在浏览器完成…
                </p>
              </template>

              <template
                v-else-if="MS_LOGIN_ENABLED && msStep === 'needClientId'"
              >
                <p class="pop-hint">先填 Azure client_id：</p>
                <input
                  v-model="clientIdInput"
                  class="pop-input"
                  placeholder="应用程序(客户端) ID"
                />
                <button class="btn btn-primary" @click="saveClientId">
                  保存并登录
                </button>
              </template>

              <template v-else>
                <div class="form-row">
                  <input
                    v-model="nameInput"
                    class="pop-input"
                    maxlength="16"
                    placeholder="离线用户名（字母数字下划线）"
                    @keyup.enter="doLoginOffline"
                  />
                  <button class="btn btn-primary" @click="doLoginOffline">
                    离线登录
                  </button>
                </div>
                <div class="form-row">
                  <button
                    v-if="MS_LOGIN_ENABLED"
                    class="btn"
                    @click="startMsLogin"
                  >
                    微软正版登录
                  </button>
                  <button
                    v-if="account"
                    class="btn btn-ghost"
                    @click="doLogout"
                  >
                    退出当前账号
                  </button>
                </div>
              </template>
            </section>

            <!-- 游戏目录 -->
            <section class="card">
              <div class="card-head">
                <h2 class="card-title">游戏目录</h2>
              </div>
              <code class="path-code">{{ gameDir || "%APPDATA%\\.minecraft" }}</code>
              <button class="btn" style="margin-top: 10px" @click="openMods">
                打开 mods 文件夹
              </button>
            </section>

            <!-- 关于 -->
            <section class="card">
              <div class="card-head">
                <h2 class="card-title">关于</h2>
              </div>
              <p class="pop-hint">
                MC Launcher 0.1.0 · Rust + Tauri 2 + Vue 3<br />
                已安装版本 {{ installedCount }} 个
              </p>
            </section>
          </div>
        </template>
      </main>
    </div>

    <!-- 日志 -->
    <section class="logs">
      <button
        class="logs-toggle"
        :class="{ open: showLogs }"
        @click="showLogs = !showLogs"
      >
        <span class="chev">▶</span>
        <span>运行日志</span>
        <span v-if="logs.length" style="opacity: 0.6"
          >· {{ logs.length }} 行</span
        >
      </button>
      <pre v-if="showLogs" ref="logEl" class="logs-body">{{
        logs.join("\n") || "（空）"
      }}</pre>
    </section>

    <!-- 状态栏 -->
    <footer class="statusbar">
      <span>{{ status }}</span>
      <span class="path">{{ gameDir }}</span>
    </footer>
  </div>
</template>

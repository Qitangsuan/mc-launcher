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

const versions = ref<VersionInfo[]>([]);
const selectedId = ref("");
const loading = ref(true);
const launching = ref(false);
const status = ref("正在扫描本地版本…");
const gameDir = ref("");
const logs = ref<string[]>([]);

const showInstall = ref(false);
const remoteVersions = ref<RemoteVersion[]>([]);
const remoteLoading = ref(false);
const installingId = ref("");
const progress = ref<DownloadProgress | null>(null);

interface AccountStore {
  accounts: Account[];
  current: number;
}

const accountStore = ref<AccountStore>({ accounts: [], current: 0 });
const account = computed(
  () => accountStore.value.accounts[accountStore.value.current] ?? null
);
const showAccount = ref(false);
const nameInput = ref("");

interface DeviceCode {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
}

const msStep = ref<"idle" | "needClientId" | "waiting">("idle");
const msCode = ref<DeviceCode | null>(null);
const clientIdInput = ref("");

const isTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

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
    if (logEl.value) {
      logEl.value.scrollTop = logEl.value.scrollHeight;
    }
  }
);

function pushLog(msg: string) {
  const t = new Date().toLocaleTimeString("zh-CN", { hour12: false });
  logs.value.push(`[${t}] ${msg}`);
  if (logs.value.length > 300) logs.value.shift();
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
    pushLog(`离线登录：${acc.username}（UUID ${acc.uuid}）`);
    showAccount.value = false;
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
    nameInput.value = "";
    pushLog("已退出登录");
    showAccount.value = false;
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
    showAccount.value = false;
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
    showAccount.value = false;
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
    status.value = `Fabric 已安装 ${newId}`;
    await loadVersions();
    selectedId.value = newId;
  } catch (e) {
    pushLog("Fabric 安装失败: " + String(e));
    status.value = "Fabric 安装失败";
  } finally {
    installingId.value = "";
    progress.value = null;
  }
}

interface LoaderVersions {
  latest: string | null;
  recommended: string | null;
  all: string[];
}

async function installForge() {
  const v = selected.value;
  if (!v || installingId.value || !isTauri) return;
  installingId.value = v.id;
  status.value = `正在给 ${v.id} 装 Forge…`;
  pushLog(`查询 ${v.gameVersion} 的加载器版本`);

  let useNeo = false;
  let versions: LoaderVersions;

  try {
    const { invoke } = await import("@tauri-apps/api/core");
    versions = await invoke<LoaderVersions>("list_forge_versions", {
      gameVersion: v.gameVersion,
    });
  } catch (e) {
    pushLog("Forge 没有这个版本，改试 NeoForge：" + String(e));
    useNeo = true;
    const { invoke } = await import("@tauri-apps/api/core");
    versions = await invoke<LoaderVersions>("list_neoforge_versions", {
      gameVersion: v.gameVersion,
    });
  }

  const target =
    versions.recommended ??
    versions.latest ??
    versions.all[versions.all.length - 1];

  pushLog(`准备安装 ${useNeo ? "NeoForge" : "Forge"} ${target}`);

  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const newId = await invoke<string>("install_forge", {
      gameVersion: v.gameVersion,
      loaderVersion: target,
      neoforge: useNeo,
    });
    pushLog(`安装完成：${newId}`);
    status.value = `${useNeo ? "NeoForge" : "Forge"} 已安装 ${newId}`;
    await loadVersions();
    selectedId.value = newId;
  } catch (e) {
    pushLog("Forge 安装失败: " + String(e));
    status.value = "Forge 安装失败";
  } finally {
    installingId.value = "";
    progress.value = null;
  }
}

async function openInstall() {  showInstall.value = true;
  if (!isTauri) {
    remoteVersions.value = MOCK_REMOTE;
    return;
  }
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
    showInstall.value = false;
    selectedId.value = id;
    await loadVersions();
  } catch (e) {
    pushLog("安装失败: " + String(e));
    status.value = "安装失败";
  } finally {
    installingId.value = "";
    progress.value = null;
  }
}

async function launch() {
  const v = selected.value;
  if (!v || launching.value) return;
  launching.value = true;
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
    <header class="titlebar">
      <div class="brand">
        <span class="logo">⛏</span>
        <span class="name">MC Launcher</span>
      </div>

      <div class="account-wrap">
        <button class="account" @click="showAccount = !showAccount">
          <span class="dot" :class="{ online: !!account }"></span>
          {{ account ? account.username : "未登录" }}
        </button>

        <div v-if="showAccount" class="account-pop">
          <!-- 等你在浏览器里输入设备码 -->
          <template v-if="msStep === 'waiting'">
            <div class="pop-title">微软登录</div>
            <p class="ms-tip">在浏览器页面输入这个代码：</p>
            <div class="ms-code">{{ msCode?.userCode }}</div>
            <button class="pop-btn" @click="openMsUrl">重新打开登录页</button>
            <p class="pop-hint">等待你在浏览器完成…</p>
          </template>

          <!-- 还没配 client_id -->
          <template v-else-if="msStep === 'needClientId'">
            <div class="pop-title">先填 Azure client_id</div>
            <input
              v-model="clientIdInput"
              class="pop-input"
              placeholder="应用程序(客户端) ID"
            />
            <button class="pop-btn primary" @click="saveClientId">
              保存并登录
            </button>
            <p class="pop-hint">在 Azure 应用注册的概览页复制</p>
          </template>

          <!-- 常规账号面板 -->
          <template v-else>
            <div class="pop-title">账号</div>
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

            <div class="pop-title mt">添加账号</div>
            <input
              v-model="nameInput"
              class="pop-input"
              maxlength="16"
              placeholder="离线用户名（字母数字下划线）"
              @keyup.enter="doLoginOffline"
            />
            <button class="pop-btn primary" @click="doLoginOffline">
              离线登录
            </button>
            <button class="pop-btn" @click="startMsLogin">微软正版登录</button>
          </template>
        </div>
      </div>
    </header>

    <main class="body">
      <aside class="sidebar">
        <template v-if="showInstall">
          <div class="sidebar-head">
            <button class="icon-btn" title="返回" @click="showInstall = false">
              ←
            </button>
            <span>安装版本</span>
          </div>
          <div v-if="remoteLoading" class="empty">获取版本清单…</div>
          <div v-else-if="!remoteVersions.length" class="empty">
            没拿到版本清单
          </div>
          <div v-else class="remote-groups">
            <div
              v-for="group in groupedRemote"
              :key="group.label"
              class="remote-group"
            >
              <div class="group-head">{{ group.label }}</div>
              <ul class="remote-list">
                <li
                  v-for="r in group.items"
                  :key="r.id"
                  class="version-item"
                  :class="{ active: r.id === installingId }"
                  @click="install(r.id)"
                >
                  <span class="v-name">{{ r.id }}</span>
                  <span class="v-tag">{{
                    r.type === "release" ? "正式" : "快照"
                  }}</span>
                </li>
              </ul>
            </div>
          </div>
        </template>

        <template v-else>
          <div class="sidebar-head">
            <span>版本列表</span>
            <button class="icon-btn" title="重新扫描" @click="loadVersions">
              ⟳
            </button>
          </div>
          <div v-if="loading" class="empty">扫描中…</div>
          <div v-else-if="!versions.length" class="empty">没找到已安装版本</div>
          <ul v-else class="version-list">
            <li
              v-for="v in versions"
              :key="v.id"
              :class="['version-item', { active: v.id === selectedId }]"
              @click="selectedId = v.id"
            >
              <span class="v-name">{{ v.id }}</span>
              <span class="v-tag">{{ v.loader }}</span>
            </li>
          </ul>
          <button class="install-btn" @click="openInstall">+ 安装新版本</button>
        </template>
      </aside>

      <section class="content">
        <div v-if="progress && progress.total > 0" class="progress-panel">
          <div class="progress-head">
            <span class="progress-stage">{{ progress.stage }}</span>
            <span class="progress-num">
              {{ progress.done }} / {{ progress.total }} · {{ pct }}%
            </span>
          </div>
          <div class="progress-track">
            <div class="progress-fill" :style="{ width: pct + '%' }"></div>
          </div>
        </div>

        <div v-if="selected" class="detail">
          <h1>{{ selected.id }}</h1>
          <div class="meta">
            <span class="chip">{{ selected.loader }}</span>
            <span class="chip">Java {{ selected.javaMajor ?? "?" }}</span>
            <span class="chip">游戏 {{ selected.gameVersion }}</span>
            <span class="chip">{{ account ? account.username : "离线" }}</span>
          </div>
          <div class="detail-actions">
            <button class="launch-btn" :disabled="launching" @click="launch">
              {{ launching ? "启动中…" : "▶ 启动游戏" }}
            </button>
            <button
              v-if="selected.loader === 'vanilla'"
              class="secondary-btn"
              :disabled="!!installingId"
              @click="installFabric"
            >
              装 Fabric
            </button>
            <button
              v-if="selected.loader === 'vanilla'"
              class="secondary-btn"
              :disabled="!!installingId"
              @click="installForge"
            >
              装 Forge
            </button>
            <button class="secondary-btn" @click="openMods">mods 文件夹</button>
          </div>
        </div>
        <div v-else class="detail empty">左边选一个版本，或者先装一个</div>

        <div class="logs">
          <div class="logs-title">运行日志</div>
          <pre ref="logEl" class="log-lines">{{ logs.join("\n") || "（空）" }}</pre>
        </div>
      </section>
    </main>

    <footer class="statusbar">
      <span>{{ status }}</span>
      <span class="path">{{ gameDir }}</span>
    </footer>
  </div>
</template>

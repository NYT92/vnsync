<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useRouter } from "vue-router";
import { getVersion } from "@tauri-apps/api/app";

import type { NavigationMenuItem, ContextMenuItem } from "@nuxt/ui";

import TitleBar from "./components/TitleBar.vue";
import AppLogo from "./components/AppLogo.vue";
import { VnSyncApi } from "./services/vnsyncApi";
import { WindowService } from "./services/windowService";

const router = useRouter();
const appVersion = ref("Desktop");
const deviceName = ref("Unknown Device");
const activeProvider = ref("Local Vault");

const setTheme = (mode: "light" | "dark" | "system") => {
  const isDark =
    mode === "dark" ||
    (mode === "system" &&
      window.matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.classList.toggle("dark", isDark);
};

const globalContextItems = computed<ContextMenuItem[][]>(() => [
  [
    {
      label: "VNSync Quick Actions",
      type: "label",
    },
    {
      label: "Add Game",
      icon: "i-heroicons-plus-circle",
      onSelect: () => router.push("/add-game"),
    },
    {
      label: "Go to Game Library",
      icon: "i-heroicons-rectangle-stack",
      onSelect: () => router.push("/"),
    },
    {
      label: "Cloud & Storage Settings",
      icon: "i-heroicons-cloud-arrow-up",
      onSelect: () => router.push("/settings"),
    },
  ],
  [
    {
      label: "Appearance",
      icon: "i-heroicons-swatch",
      children: [
        {
          label: "Light Theme",
          icon: "i-heroicons-sun",
          onSelect: () => setTheme("light"),
        },
        {
          label: "Dark Theme",
          icon: "i-heroicons-moon",
          onSelect: () => setTheme("dark"),
        },
        {
          label: "System Default",
          icon: "i-heroicons-computer-desktop",
          onSelect: () => setTheme("system"),
        },
      ],
    },
    {
      label: "Reload Interface",
      icon: "i-heroicons-arrow-path",
      onSelect: () => window.location.reload(),
    },
  ],
  [
    {
      label: "Hide to System Tray",
      icon: "i-heroicons-arrow-down-on-square",
      onSelect: () => WindowService.hideToTray(),
    },
    {
      label: "Exit VNSync",
      icon: "i-heroicons-power",
      color: "error",
      onSelect: () => WindowService.quitApp(),
    },
  ],
]);

const loadSidebarStatus = async () => {
  try {
    const settings = await VnSyncApi.getAppSettings();
    if (settings?.deviceName) deviceName.value = settings.deviceName;
    const profiles = await VnSyncApi.getStorageProfiles();
    const active = profiles.find((p) => p.isActive);
    if (active)
      activeProvider.value = `${active.name} (${active.providerType.toUpperCase()})`;
  } catch {
    // fallback in browser preview
  }
};

onMounted(loadSidebarStatus);
onMounted(async () => {
  try {
    const version = await getVersion();
    appVersion.value = `v${version}`;
  } catch {
    // not running inside Tauri; keep static version
  }
});

const navItems: NavigationMenuItem[][] = [
  [
    {
      label: "Library",
      icon: "i-heroicons-rectangle-stack",
      to: "/",
    },
    {
      label: "Add Game",
      icon: "i-heroicons-plus-circle",
      to: "/add-game",
    },
    {
      label: "Save Storage",
      icon: "i-heroicons-circle-stack",
      to: "/storage",
    },
    {
      label: "Cloud & Settings",
      icon: "i-heroicons-cog-6-tooth",
      to: "/settings",
    },
  ],
  [
    {
      label: "About VNSync",
      icon: "i-heroicons-information-circle",
      to: "/about",
    },
  ],
];
</script>

<template>
  <UApp>
    <UContextMenu :items="globalContextItems" :ui="{ content: 'w-56' }">
      <div class="h-screen w-screen overflow-hidden" @contextmenu.prevent>
        <TitleBar />
        <UDashboardGroup
          :ui="{ base: 'fixed top-8 inset-x-0 bottom-0 flex overflow-hidden' }"
        >
          <UDashboardSidebar
            collapsible
            resizable
            :min-size="16"
            :default-size="20"
            :max-size="28"
            :ui="{ root: 'min-h-0 h-full' }"
            class="bg-elevated/40"
          >
            <template #header="{ collapsed }">
              <div
                class="flex items-center gap-2.5 py-1"
                :class="collapsed ? 'justify-center w-full' : 'px-1'"
              >
                <div
                  class="size-7 rounded-full bg-primary/15 text-primary flex items-center justify-center shrink-0"
                >
                  <AppLogo class="size-4.5" />
                </div>
                <div v-if="!collapsed" class="min-w-0">
                  <p class="text-sm font-bold text-highlighted leading-none">
                    VNSync
                  </p>
                  <p class="text-[11px] text-muted truncate mt-0.5 shrink-0">
                    {{ appVersion }}
                  </p>
                </div>
              </div>
            </template>

            <template #default="{ collapsed }">
              <UNavigationMenu
                :collapsed="collapsed"
                :items="navItems[0]"
                orientation="vertical"
              />

              <UNavigationMenu
                :collapsed="collapsed"
                :items="navItems[1]"
                orientation="vertical"
                class="mt-auto"
              />
            </template>

            <template #footer="{ collapsed }">
              <div
                class="flex items-center w-full gap-2 py-1 rounded-md"
                :class="
                  collapsed
                    ? 'flex-col justify-center'
                    : 'justify-between px-1 bg-elevated/60'
                "
              >
                <div v-if="!collapsed" class="flex items-center gap-2 min-w-0">
                  <UIcon
                    name="i-heroicons-computer-desktop"
                    class="size-4 text-primary shrink-0"
                  />
                  <div class="min-w-0">
                    <p class="text-xs font-medium text-highlighted truncate">
                      {{ deviceName }}
                    </p>
                    <p class="text-[10px] text-muted truncate">
                      {{ activeProvider }}
                    </p>
                  </div>
                </div>
                <UColorModeButton size="lg" />
              </div>
            </template>
          </UDashboardSidebar>

          <RouterView />
        </UDashboardGroup>
      </div>
    </UContextMenu>
  </UApp>
</template>

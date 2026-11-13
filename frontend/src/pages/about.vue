<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import AppLogo from '@/components/AppLogo.vue'

const features = [
  {
    icon: 'i-tabler-device-gamepad',
    title: 'Cross-OS & Proton Save Resolver',
    description:
      'Auto-detects Ren\'Py, RPG Maker, Kirikiri, and Unity saves and translates paths across Windows, macOS, and Linux/Steam Deck Proton prefixes.'
  },
  {
    icon: 'i-heroicons-archive-box',
    title: 'Content-Addressable Vault',
    description:
      'Packs saves into deterministic zstd-compressed tar snapshots verified with BLAKE3 tree hashes.'
  },
  {
    icon: 'i-tabler-bucket',
    title: 'Unified Cloud Storage (OpenDAL)',
    description:
      'Supports Cloudflare R2, AWS S3, MinIO, Backblaze B2, WebDAV (Nextcloud), and local/NAS folders.'
  },
  {
    icon: 'i-heroicons-arrows-right-left',
    title: 'Lineage Conflict Protection',
    description:
      'Detects diverged save timelines across devices and lets you inspect or restore any historical slot backup.'
  },
  {
    icon: 'i-heroicons-magnifying-glass',
    title: 'VNDB Metadata Integration',
    description:
      'Fetches visual novel cover art, original titles, developers, and release dates directly from VNDB API v2.'
  },
  {
    icon: 'i-heroicons-play',
    title: 'Process Watcher & Playtime',
    description:
      'Launches games with automatic pre-launch save pull, tracks playtime, and snapshots saves on exit.'
  }
]

const technologies = [
  { name: 'Tauri v2', version: '2.2', url: 'https://v2.tauri.app/' },
  { name: 'Rust', version: '2021 Edition', url: 'https://www.rust-lang.org/' },
  { name: 'Apache OpenDAL', version: '0.51', url: 'https://opendal.apache.org/' },
  { name: 'Vue 3', version: '3.5.22', url: 'https://vuejs.org/' },
  { name: 'Nuxt UI', version: '4.0.1', url: 'https://ui.nuxt.com/' },
  { name: 'Tailwind CSS', version: '4.1.14', url: 'https://tailwindcss.com/' }
]

const currentVersion = ref('0.0.0')

onMounted(async () => {
  try {
    currentVersion.value = await getVersion()
  } catch {
    // not running inside Tauri; keep static version
  }
})

const latest = ref<{ version: string, url: string, notes: string } | null>(null)
const isChecking = ref(false)
const checkError = ref('')
const lastChecked = ref<Date | null>(null)

// Compare dotted numeric versions; returns true when a > b.
const isNewer = (a: string, b: string) => {
  const pa = a.split('.').map(n => parseInt(n) || 0)
  const pb = b.split('.').map(n => parseInt(n) || 0)
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    if ((pa[i] ?? 0) !== (pb[i] ?? 0)) return (pa[i] ?? 0) > (pb[i] ?? 0)
  }
  return false
}

const updateAvailable = () => !!latest.value && isNewer(latest.value.version, currentVersion.value)

const checkForUpdates = async () => {
  isChecking.value = true
  checkError.value = ''
  try {
    const res = await fetch('https://api.github.com/repos/nyt92/vnsync/releases/latest')
    if (!res.ok) throw new Error(`GitHub API returned ${res.status}. Please try again later or disable your firewall/proxy.`)
    const data = await res.json()
    latest.value = {
      version: String(data.tag_name).replace(/^v/, ''),
      url: data.html_url,
      notes: data.body ?? ''
    }
    lastChecked.value = new Date()
  } catch (e) {
    checkError.value = e instanceof Error ? e.message : String(e)
  } finally {
    isChecking.value = false
  }
}

const appInfo = {
  name: 'VNSync',
  version: currentVersion.value,
  description: 'Cross-Platform Visual Novel & RPG Save Sync Manager',
  developer: 'NYT92',
  website: 'https://github.com/nyt92/vnsync',
  license: 'GPL-3.0'
}

</script>


<template>
  <UDashboardPanel id="about-panel">
    <template #header>
      <UDashboardNavbar title="About VNSync">
        <template #leading>
          <UDashboardSidebarCollapse />
        </template>

        <template #right>
          <UButton
            label="GitHub Repository"
            icon="i-heroicons-code-bracket"
            color="neutral"
            variant="subtle"
            size="lg"
            :to="appInfo.website"
            target="_blank"
          />
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="max-w-4xl mx-auto space-y-6">
        <UCard variant="subtle">
          <div class="flex flex-col lg:flex-row items-center gap-5">
            <div
              class="size-16 rounded-full bg-primary/15 text-primary flex items-center justify-center shrink-0"
            >
              <AppLogo class="size-10" />
            </div>
            <div class="space-y-1.5 text-center lg:text-left flex-1">
              <div class="flex items-center justify-center lg:justify-start gap-2">
                <h1 class="text-2xl font-bold text-highlighted">
                  {{ appInfo.name }}
                </h1>
                <UBadge :label="`v${currentVersion}`" color="primary" variant="subtle" size="sm" />
                <UBadge :label="appInfo.license" color="success" variant="subtle" size="sm" />
              </div>
              <p class="text-sm text-muted">
                {{ appInfo.description }}
              </p>
              <p class="text-xs text-muted">
                Created by <strong class="text-highlighted">{{ appInfo.developer }}</strong>
              </p>
            </div>
          </div>
        </UCard>

        <UCard variant="outline">
          <template #header>
            <h2 class="text-sm font-semibold text-highlighted">
              Core Capabilities
            </h2>
          </template>

          <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
            <UCard
              v-for="(feature, index) in features"
              :key="index"
              variant="subtle"
            >
              <div class="space-y-1.5">
                <div class="flex items-center gap-2 font-semibold text-sm text-highlighted">
                  <UIcon :name="feature.icon" class="size-4 text-primary shrink-0" />
                  <span>{{ feature.title }}</span>
                </div>
                <p class="text-xs text-muted">
                  {{ feature.description }}
                </p>
              </div>
            </UCard>
          </div>
        </UCard>

        <UCard variant="outline">
          <template #header>
            <div class="flex items-center justify-between gap-2">
              <h2 class="text-sm font-semibold text-highlighted">
                Updates
              </h2>
              <UButton
                label="Check for Updates"
                icon="i-heroicons-arrow-path"
                color="primary"
                variant="subtle"
                :loading="isChecking"
                @click="checkForUpdates"
              />
            </div>
          </template>

          <div class="space-y-3 text-sm">
            <div class="flex items-center gap-2">
              <span class="text-muted">Installed version:</span>
              <UBadge :label="`v${currentVersion}`" color="neutral" variant="subtle" />
              <template v-if="latest">
                <span class="text-muted">Latest:</span>
                <UBadge
                  :label="`v${latest.version}`"
                  :color="updateAvailable() ? 'warning' : 'success'"
                  variant="subtle"
                />
              </template>
            </div>

            <UAlert
              v-if="checkError"
              color="error"
              variant="subtle"
              icon="i-heroicons-exclamation-triangle"
              title="Update check failed"
              :description="checkError"
            />
            <UAlert
              v-else-if="latest && updateAvailable()"
              color="warning"
              variant="subtle"
              icon="i-heroicons-arrow-down-circle"
              :title="`VNSync v${latest.version} is available`"
              :actions="[{ label: 'Download', to: latest.url, target: '_blank', color: 'warning', variant: 'solid' }]"
            />
            <UAlert
              v-else-if="latest"
              color="success"
              variant="subtle"
              icon="i-heroicons-check-circle"
              title="You are on the latest version"
            />

            <pre
              v-if="latest && updateAvailable() && latest.notes"
              class="text-xs text-muted whitespace-pre-wrap max-h-48 overflow-auto rounded-md bg-elevated/60 p-3"
            >{{ latest.notes }}</pre>

            <p v-if="lastChecked" class="text-xs text-muted">
              Last checked {{ lastChecked.toLocaleString() }}
            </p>
          </div>
        </UCard>
      </div>
    </template>
  </UDashboardPanel>
</template>

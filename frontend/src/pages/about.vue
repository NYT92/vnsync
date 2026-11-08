<script setup lang="ts">
import AppLogo from '@/components/AppLogo.vue'

const appInfo = {
  name: 'VNSync',
  version: '0.1.0',
  description: 'Cross-Platform Visual Novel & RPG Save Sync Manager',
  developer: 'NYT92',
  website: 'https://github.com/nyt92/vnsync',
  license: 'GPL-3.0'
}

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
                <UBadge :label="`v${appInfo.version}`" color="primary" variant="subtle" size="lg" />
                <UBadge :label="appInfo.license" color="success" variant="subtle" size="lg" />
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
            <h3 class="text-sm font-semibold text-highlighted">
              Technology Stack
            </h3>
          </template>

          <div class="grid grid-cols-1 lg:grid-cols-3 gap-3">
            <UCard
              v-for="tech in technologies"
              :key="tech.name"
              variant="subtle"
            >
              <div class="flex items-center justify-between">
                <span class="font-medium text-xs text-highlighted">{{ tech.name }}</span>
                <UBadge :label="tech.version" color="neutral" variant="outline" size="lg" />
              </div>
            </UCard>
          </div>
        </UCard>
      </div>
    </template>
  </UDashboardPanel>
</template>

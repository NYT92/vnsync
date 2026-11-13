<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { ask } from '@tauri-apps/plugin-dialog'
import {
  VnSyncApi,
  formatBytes,
  type GameStorageUsage,
  type RemoteGameHead
} from '@/services/vnsyncApi'

const toast = useToast()

const usage = ref<GameStorageUsage[]>([])
// gameId -> cloud history, or an error message when the cloud could not be read
const cloud = ref<Record<string, RemoteGameHead[] | string>>({})
const keep = ref(20)
const busyId = ref<string | null>(null)

const totalLocal = computed(() => usage.value.reduce((n, u) => n + u.localBytes, 0))

const cloudSummary = (gameId: string) => {
  const c = cloud.value[gameId]
  if (c === undefined) return 'Loading…'
  if (typeof c === 'string') return 'Unavailable'
  if (c.length === 0) return 'None'
  return `${c.length} saves (${formatBytes(c.reduce((n, h) => n + h.totalBytes, 0))})`
}

const loadCloud = async () => {
  cloud.value = {}
  await Promise.all(
    usage.value.map(async u => {
      try {
        cloud.value[u.gameId] = await VnSyncApi.listCloudSnapshots(u.gameId)
      } catch (err) {
        cloud.value[u.gameId] = String(err)
      }
    })
  )
}

const load = async () => {
  usage.value = await VnSyncApi.getStorageUsage()
  loadCloud()
}

onMounted(async () => {
  try {
    keep.value = (await VnSyncApi.getAppSettings()).maxLocalSnapshots || 20
  } catch {
    // keep default
  }
  await load()
})

const showError = (title: string, err: unknown) =>
  toast.add({ title, description: String(err), color: 'error', icon: 'i-heroicons-exclamation-circle' })

const handlePrune = async (game: GameStorageUsage | null) => {
  const target = game ? game.title : 'every game'
  const ok = await ask(
    `Delete local snapshots of ${target} except the newest ${keep.value}, pinned ones and the current HEAD? Cloud saves are not affected.`,
    { title: 'Clean Up Local Snapshots', kind: 'warning' }
  )
  if (!ok) return
  busyId.value = game?.gameId ?? '*'
  try {
    const out = await VnSyncApi.pruneLocalSnapshots(game?.gameId ?? null, keep.value)
    toast.add({
      title: 'Local Snapshots Cleaned',
      description: `Deleted ${out.deleted} snapshots, freed ${formatBytes(out.freedBytes)}.`,
      color: 'success',
      icon: 'i-heroicons-trash'
    })
    await load()
  } catch (err) {
    showError('Clean Up Failed', err)
  } finally {
    busyId.value = null
  }
}

const handleDeleteCloud = async (game: GameStorageUsage) => {
  const ok = await ask(
    `Permanently delete all cloud saves of ${game.title}? Other PCs can no longer download them. Saves on this PC are not affected.`,
    { title: 'Delete Cloud Saves', kind: 'warning' }
  )
  if (!ok) return
  busyId.value = game.gameId
  try {
    await VnSyncApi.deleteCloudGameData(game.gameId)
    toast.add({
      title: 'Cloud Saves Deleted',
      description: game.title,
      color: 'neutral',
      icon: 'i-heroicons-cloud'
    })
    await load()
  } catch (err) {
    showError('Delete Failed', err)
  } finally {
    busyId.value = null
  }
}
</script>

<template>
  <UDashboardPanel id="storage-panel">
    <template #header>
      <UDashboardNavbar title="Save Storage">
        <template #right>
          <UButton
            label="Refresh"
            icon="i-heroicons-arrow-path"
            color="neutral"
            variant="subtle"
            size="lg"
            @click="load"
          />
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="max-w-5xl mx-auto space-y-6">
        <UCard variant="subtle">
          <div class="flex flex-col lg:flex-row lg:items-end justify-between gap-4">
            <div>
              <h3 class="text-sm font-semibold text-highlighted">
                Local snapshot vault: {{ formatBytes(totalLocal) }}
              </h3>
              <p class="text-xs text-muted">
                Clean up keeps the newest snapshots, pinned ones and the current HEAD of each game.
              </p>
            </div>
            <div class="flex items-end gap-2">
              <UFormField label="Keep newest">
                <UInputNumber v-model="keep" :min="1" :max="500" size="md" class="w-32" />
              </UFormField>
              <UButton
                label="Clean Up All Games"
                icon="i-heroicons-trash"
                color="warning"
                variant="subtle"
                size="md"
                :loading="busyId === '*'"
                @click="handlePrune(null)"
              />
            </div>
          </div>
        </UCard>

        <UCard variant="outline">
          <UEmpty
            v-if="usage.length === 0"
            icon="i-heroicons-archive-box"
            title="No Games Yet"
            description="Add a game to start tracking its saves."
          />
          <div v-else class="divide-y divide-default">
            <div
              v-for="u in usage"
              :key="u.gameId"
              class="py-3 first:pt-0 last:pb-0 flex flex-col lg:flex-row lg:items-center justify-between gap-3 px-2"
            >
              <div class="space-y-1 min-w-0">
                <RouterLink
                  :to="`/game/${u.gameId}`"
                  class="text-sm font-semibold text-highlighted hover:text-primary truncate block"
                >
                  {{ u.title }}
                </RouterLink>
                <div class="flex flex-wrap items-center gap-3 text-xs text-muted">
                  <span>
                    This PC: {{ u.snapshotCount }} snapshots ({{ formatBytes(u.localBytes) }})<template
                      v-if="u.pinnedCount"
                    >, {{ u.pinnedCount }} pinned</template>
                  </span>
                  <span>•</span>
                  <span :title="typeof cloud[u.gameId] === 'string' ? (cloud[u.gameId] as string) : ''">
                    Cloud: {{ cloudSummary(u.gameId) }}
                  </span>
                </div>
              </div>

              <div class="flex items-center gap-1.5 shrink-0">
                <UButton
                  label="Clean Up"
                  icon="i-heroicons-trash"
                  color="neutral"
                  variant="subtle"
                  size="md"
                  :loading="busyId === u.gameId"
                  :disabled="u.snapshotCount <= keep"
                  @click="handlePrune(u)"
                />
                <UButton
                  label="Delete Cloud Saves"
                  icon="i-heroicons-cloud"
                  color="error"
                  variant="ghost"
                  size="md"
                  :disabled="!Array.isArray(cloud[u.gameId]) || (cloud[u.gameId] as RemoteGameHead[]).length === 0"
                  @click="handleDeleteCloud(u)"
                />
              </div>
            </div>
          </div>
        </UCard>
      </div>
    </template>
  </UDashboardPanel>
</template>

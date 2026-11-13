<script setup lang="ts">
import { ref, onMounted, onUnmounted } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open, save, ask } from '@tauri-apps/plugin-dialog'
import type { TabsItem, ContextMenuItem } from '@nuxt/ui'
import {
  VnSyncApi,
  formatPlaytime,
  formatBytes,
  type Game,
  type SnapshotRecord,
  type SyncOutcome,
  type RemoteGameHead
} from '@/services/vnsyncApi'
import EditGameSlideover from '@/components/EditGameSlideover.vue'

const route = useRoute()
const router = useRouter()
const toast = useToast()
const gameId = route.params.id as string

const game = ref<Game | null>(null)
const snapshots = ref<SnapshotRecord[]>([])
const resolvedPaths = ref<string[]>([])
const noteInput = ref('')
const isBusy = ref(false)
const isRunning = ref(false)
const syncOutcome = ref<SyncOutcome | null>(null)
const isEditOpen = ref(false)
const cloudSnapshots = ref<RemoteGameHead[]>([])
const cloudError = ref('')
const isCloudLoading = ref(false)

let unlistenStarted: UnlistenFn | null = null
let unlistenExited: UnlistenFn | null = null

const tabItems: TabsItem[] = [
  {
    label: 'Save Vault & History',
    icon: 'i-heroicons-archive-box',
    slot: 'vault'
  },
  {
    label: 'Cloud Saves',
    icon: 'i-heroicons-cloud',
    slot: 'cloud'
  },
  {
    label: 'Path Mapping & Proton',
    icon: 'i-heroicons-folder-open',
    slot: 'paths'
  }
]

const loadAll = async () => {
  const g = await VnSyncApi.getGame(gameId)
  if (!g) {
    router.push('/')
    return
  }
  game.value = g
  const [snaps, activeIds] = await Promise.all([
    VnSyncApi.listSnapshots(gameId),
    VnSyncApi.getRunningGames()
  ])
  snapshots.value = snaps
  isRunning.value = activeIds.includes(gameId)
  resolvedPaths.value = await VnSyncApi.previewResolvedPaths(
    g.savePaths,
    g.exePath,
    g.protonPrefix
  )
}

const loadCloud = async () => {
  isCloudLoading.value = true
  cloudError.value = ''
  try {
    cloudSnapshots.value = await VnSyncApi.listCloudSnapshots(gameId)
  } catch (err: any) {
    cloudSnapshots.value = []
    cloudError.value = String(err)
  } finally {
    isCloudLoading.value = false
  }
}

onMounted(async () => {
  await loadAll()
  loadCloud()
  try {
    unlistenStarted = await listen<string>('game:started', event => {
      if (event.payload === gameId) isRunning.value = true
    })
    unlistenExited = await listen<string>('game:exited', async event => {
      if (event.payload === gameId) {
        isRunning.value = false
        await loadAll()
      }
    })
  } catch {
    // non-tauri preview fallback
  }
})

onUnmounted(() => {
  unlistenStarted?.()
  unlistenExited?.()
})

const handleCreateCheckpoint = async () => {
  if (!game.value) return
  isBusy.value = true
  try {
    const snap = await VnSyncApi.createManualSnapshot(game.value.id, noteInput.value)
    noteInput.value = ''
    toast.add({
      title: 'Snapshot Created',
      description: `Packed ${snap.fileCount} save files (${formatBytes(snap.totalBytes)})`,
      color: 'success',
      icon: 'i-heroicons-archive-box-arrow-down'
    })
    await loadAll()
  } catch (err: any) {
    toast.add({
      title: 'Snapshot Failed',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  } finally {
    isBusy.value = false
  }
}

const handleRestore = async (snap: SnapshotRecord) => {
  if (!game.value) return
  isBusy.value = true
  try {
    const count = await VnSyncApi.restoreSnapshot(game.value.id, snap.id)
    toast.add({
      title: 'Save State Restored',
      description: `Restored ${count} files from ${snap.note || 'checkpoint'}.`,
      color: 'success',
      icon: 'i-heroicons-arrow-uturn-left'
    })
    await loadAll()
  } catch (err: any) {
    toast.add({
      title: 'Restore Failed',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  } finally {
    isBusy.value = false
  }
}

const showError = (title: string, err: unknown) =>
  toast.add({
    title,
    description: String(err),
    color: 'error',
    icon: 'i-heroicons-exclamation-circle'
  })

const handleCloudRestore = async (head: RemoteGameHead) => {
  if (!game.value) return
  const when = new Date(head.updatedAt).toLocaleString()
  const ok = await ask(
    `Replace this PC's current saves with the cloud save from ${head.deviceName} (${when})? A safety backup is created first.`,
    { title: 'Restore Cloud Save', kind: 'warning' }
  )
  if (!ok) return
  isBusy.value = true
  try {
    const count = await VnSyncApi.restoreCloudSnapshot(game.value.id, head.headSnapshotId)
    toast.add({
      title: 'Cloud Save Restored',
      description: `Restored ${count} files from ${head.deviceName}. Sync to share it with other PCs.`,
      color: 'success',
      icon: 'i-heroicons-cloud-arrow-down'
    })
    await loadAll()
  } catch (err) {
    showError('Restore Failed', err)
  } finally {
    isBusy.value = false
  }
}

const handleTogglePin = async (snap: SnapshotRecord) => {
  try {
    await VnSyncApi.setSnapshotPinned(snap.id, !snap.pinned)
    await loadAll()
  } catch (err) {
    showError('Pin Failed', err)
  }
}

const handleExportSnapshot = async (snap: SnapshotRecord) => {
  if (!game.value) return
  try {
    const stamp = snap.createdAt.slice(0, 19).replace(/[T:]/g, '-')
    const dest = await save({
      title: 'Export Save Snapshot',
      defaultPath: `${game.value.title.replace(/[\\/:*?"<>|]/g, '_')}-${stamp}.tar.zst`,
      filters: [{ name: 'Snapshot Archive', extensions: ['zst'] }]
    })
    if (!dest) return
    await VnSyncApi.exportSnapshot(snap.id, dest)
    toast.add({
      title: 'Snapshot Exported',
      description: dest,
      color: 'success',
      icon: 'i-heroicons-arrow-down-tray'
    })
  } catch (err) {
    showError('Export Failed', err)
  }
}

const handleDeleteSnapshot = async (snap: SnapshotRecord) => {
  const ok = await ask(
    `Delete "${snap.note || 'Save Checkpoint'}" from this PC? Cloud copies are not affected.`,
    { title: 'Delete Snapshot', kind: 'warning' }
  )
  if (!ok) return
  try {
    await VnSyncApi.deleteSnapshot(snap.id)
    await loadAll()
  } catch (err) {
    showError('Delete Failed', err)
  }
}

const handleSync = async (forceDirection?: 'push' | 'pull') => {
  if (!game.value) return
  isBusy.value = true
  try {
    const out = await VnSyncApi.syncGameNow(game.value.id, forceDirection)
    syncOutcome.value = out
    toast.add({
      title: out.action === 'conflict' ? 'Sync Conflict Detected' : 'Sync Complete',
      description: out.message,
      color: out.action === 'conflict' ? 'error' : 'success',
      icon:
        out.action === 'conflict'
          ? 'i-heroicons-exclamation-triangle'
          : 'i-heroicons-cloud-arrow-up'
    })
    await loadAll()
    loadCloud()
  } catch (err: any) {
    toast.add({
      title: 'Sync Error',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  } finally {
    isBusy.value = false
  }
}

const handleLaunch = async () => {
  if (!game.value || isRunning.value) return
  isRunning.value = true
  try {
    const msg = await VnSyncApi.launchGame(game.value.id)
    toast.add({
      title: 'Launched Game',
      description: msg,
      color: 'success',
      icon: 'i-heroicons-play'
    })
  } catch (err: any) {
    isRunning.value = false
    toast.add({
      title: 'Launch Error',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  }
}

const handleLocateGame = async (pickFolder = true) => {
  if (!game.value) return
  try {
    const selected = await open({
      directory: pickFolder,
      multiple: false,
      title: pickFolder
        ? `Select Game Folder for ${game.value.title}`
        : `Select Executable for ${game.value.title}`
    })
    if (typeof selected === 'string' && selected) {
      await VnSyncApi.relocateGamePath(game.value.id, selected)
      toast.add({
        title: 'Game Path Updated',
        description: 'Executable and portable save paths resolved for this user.',
        color: 'success',
        icon: 'i-heroicons-folder-open'
      })
      await loadAll()
    }
  } catch (err: any) {
    toast.add({
      title: 'Could not locate game',
      description: String(err),
      color: 'error'
    })
  }
}

const handleDeleteGame = async () => {
  if (!game.value) return
  await VnSyncApi.removeGame(game.value.id)
  toast.add({
    title: 'Removed from Library',
    description: `${game.value.title} removed. Local game saves were untouched.`,
    color: 'neutral'
  })
  router.push('/')
}

const getSnapshotContextMenu = (snap: SnapshotRecord): ContextMenuItem[][] => [
  [
    {
      label: snap.note || 'Save Snapshot',
      type: 'label'
    },
    {
      label: 'Restore This Snapshot',
      icon: 'i-heroicons-arrow-uturn-left',
      onSelect: () => handleRestore(snap)
    },
    {
      label: snap.pinned ? 'Unpin' : 'Pin (keep when pruning)',
      icon: 'i-heroicons-bookmark',
      onSelect: () => handleTogglePin(snap)
    },
    {
      label: 'Export .tar.zst',
      icon: 'i-heroicons-arrow-down-tray',
      onSelect: () => handleExportSnapshot(snap)
    },
    {
      label: 'Copy BLAKE3 Hash',
      icon: 'i-heroicons-clipboard-document',
      onSelect: () => {
        navigator.clipboard.writeText(snap.treeHash)
        toast.add({
          title: 'Copied BLAKE3 Hash',
          description: snap.treeHash,
          color: 'neutral'
        })
      }
    }
  ],
  [
    {
      label: 'Delete Snapshot',
      icon: 'i-heroicons-trash',
      color: 'error',
      disabled: game.value?.headSnapshotId === snap.id,
      onSelect: () => handleDeleteSnapshot(snap)
    }
  ]
]
</script>

<template>
  <UDashboardPanel v-if="game" id="game-detail-panel">
    <template #header>
      <UDashboardNavbar :title="game.title">
        <template #leading>
          <UButton
            icon="i-heroicons-arrow-left"
            color="neutral"
            variant="ghost"
            size="lg"
            to="/"
          />
        </template>

        <template #right>
          <UButton
            label="Edit Game"
            icon="i-heroicons-pencil-square"
            color="neutral"
            variant="outline"
            size="lg"
            @click="isEditOpen = true"
          />
          <UButton
            label="Sync Saves"
            icon="i-heroicons-cloud-arrow-up"
            color="neutral"
            variant="subtle"
            size="lg"
            :loading="isBusy"
            @click="handleSync()"
          />
          <UButton
            v-if="game.exePath && game.exeExists !== false"
            :label="isRunning ? 'Running...' : 'Play Game'"
            icon="i-heroicons-play-solid"
            :color="isRunning ? 'success' : 'primary'"
            :variant="isRunning ? 'subtle' : 'solid'"
            :loading="isRunning"
            size="lg"
            :disabled="isRunning"
            @click="handleLaunch"
          />
          <UButton
            v-else
            label="Locate Game Folder"
            icon="i-heroicons-folder-open"
            color="warning"
            variant="solid"
            size="lg"
            @click="handleLocateGame(true)"
          />
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="w-full max-w-5xl mx-auto space-y-6">
        <!-- Hero Banner Card -->
        <UCard variant="subtle">
          <div class="flex flex-col lg:flex-row gap-6 items-start">
            <img
              :src="
                game.thumbnail ||
                'https://placehold.co/300x420/1e293b/e2e8f0?text=' +
                  encodeURIComponent(game.title)
              "
              :alt="game.title"
              class="w-40 h-56 object-cover rounded-lg shadow-md shrink-0"
            />

            <div class="flex-1 min-w-0 space-y-3">
              <div class="flex items-center gap-2 flex-wrap">
                <UBadge :label="game.engine" color="primary" variant="subtle" size="md" />
                <UBadge
                  :label="game.syncStatus"
                  :color="
                    game.syncStatus === 'Synced'
                      ? 'success'
                      : game.syncStatus === 'Conflict'
                        ? 'error'
                        : 'neutral'
                  "
                  size="md"
                />
                <UBadge :label="game.status" color="neutral" variant="outline" size="md" />
                <UBadge
                  v-if="game.vndbId"
                  :label="`VNDB ${game.vndbId}`"
                  color="neutral"
                  variant="subtle"
                  size="md"
                />
              </div>

              <div>
                <h1 class="text-3xl font-bold text-highlighted">
                  {{ game.title }}
                </h1>
                <p class="text-sm text-muted">
                  {{ game.originalTitle ? `${game.originalTitle} • ` : '' }}{{ game.developer || 'Unknown Studio' }}
                </p>
              </div>

              <p class="text-sm text-muted line-clamp-3">
                {{ game.description || 'No description provided.' }}
              </p>

              <div class="flex flex-wrap items-center gap-4 pt-1 text-sm text-muted">
                <span class="flex items-center gap-1">
                  <UIcon name="i-heroicons-clock" class="size-5 text-primary" />
                  Playtime: <strong class="text-highlighted">{{ formatPlaytime(game.playtimeSeconds) }}</strong>
                </span>
                <span class="flex items-center gap-1">
                  <UIcon name="i-heroicons-archive-box" class="size-5 text-primary" />
                  Snapshots: <strong class="text-highlighted">{{ snapshots.length }}</strong>
                </span>
                <span v-if="game.lastPlayedAt">
                  Last Played: <strong class="text-highlighted">{{ new Date(game.lastPlayedAt).toLocaleString() }}</strong>
                </span>
              </div>
            </div>

            <div class="flex items-center gap-1">
              <UTooltip text="Edit Metadata & Paths">
                <UButton
                  icon="i-heroicons-pencil-square"
                  color="neutral"
                  variant="ghost"
                  size="md"
                  @click="isEditOpen = true"
                />
              </UTooltip>

              <UModal
                title="Remove Game from VNSync?"
                description="This removes the game entry and local snapshot vault history from VNSync. Your actual game save files on disk will not be deleted."
              >
                <UButton
                  icon="i-heroicons-trash"
                  color="error"
                  variant="ghost"
                  size="md"
                />

                <template #footer="{ close }">
                  <div class="flex justify-end gap-2 w-full">
                    <UButton label="Cancel" color="neutral" variant="ghost" size="md" @click="close" />
                    <UButton
                      label="Remove Game"
                      color="error"
                      size="md"
                      @click="
                        () => {
                          close()
                          handleDeleteGame()
                        }
                      "
                    />
                  </div>
                </template>
              </UModal>
            </div>
          </div>
        </UCard>

        <!-- Conflict Resolution Alert & Side-by-Side Comparison -->
        <UCard
          v-if="game.syncStatus === 'Conflict' || syncOutcome?.action === 'conflict'"
          variant="outline"
          class="ring-2 ring-error"
        >
          <template #header>
            <div class="flex items-center gap-2 text-error">
              <UIcon name="i-heroicons-exclamation-triangle" class="size-5" />
              <span class="font-bold text-sm">Save Lineage Conflict Detected</span>
            </div>
          </template>

          <div class="space-y-4">
            <p class="text-xs text-muted">
              Both this device and remote cloud storage modified saves independently since the last shared snapshot. Choose which state should become active:
            </p>

            <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
              <UCard variant="subtle">
                <div class="space-y-2">
                  <div class="flex items-center justify-between">
                    <span class="text-xs font-bold text-highlighted">Local Device State</span>
                    <UBadge label="Local" color="warning" variant="subtle" size="sm" />
                  </div>
                  <div v-if="syncOutcome?.localSnapshot" class="text-xs text-muted space-y-1">
                    <div>Device: {{ syncOutcome.localSnapshot.deviceName }}</div>
                    <div>Saved: {{ new Date(syncOutcome.localSnapshot.createdAt).toLocaleString() }}</div>
                    <div>Size: {{ syncOutcome.localSnapshot.fileCount }} files ({{ formatBytes(syncOutcome.localSnapshot.totalBytes) }})</div>
                  </div>
                  <UButton
                    label="Keep Local & Push to Cloud"
                    icon="i-heroicons-cloud-arrow-up"
                    color="warning"
                    size="lg"
                    block
                    :loading="isBusy"
                    @click="handleSync('push')"
                  />
                </div>
              </UCard>

              <UCard variant="subtle">
                <div class="space-y-2">
                  <div class="flex items-center justify-between">
                    <span class="text-xs font-bold text-highlighted">Remote Cloud State</span>
                    <UBadge label="Cloud" color="primary" variant="subtle" size="sm" />
                  </div>
                  <div v-if="syncOutcome?.remoteHead" class="text-xs text-muted space-y-1">
                    <div>Device: {{ syncOutcome.remoteHead.deviceName }}</div>
                    <div>Updated: {{ new Date(syncOutcome.remoteHead.updatedAt).toLocaleString() }}</div>
                    <div>Size: {{ syncOutcome.remoteHead.fileCount }} files ({{ formatBytes(syncOutcome.remoteHead.totalBytes) }})</div>
                  </div>
                  <UButton
                    label="Pull Cloud & Restore Locally"
                    icon="i-heroicons-cloud-arrow-down"
                    color="primary"
                    size="lg"
                    block
                    :loading="isBusy"
                    @click="handleSync('pull')"
                  />
                </div>
              </UCard>
            </div>
          </div>
        </UCard>

        <!-- Main Tabs: Save Vault vs Path Mapping -->
        <UTabs :items="tabItems" class="w-full">
          <template #vault>
            <UCard variant="outline" class="mt-3">
              <template #header>
                <div class="flex flex-col lg:flex-row lg:items-center justify-between gap-3">
                  <div>
                    <h3 class="text-sm font-semibold text-highlighted">
                      Snapshot Vault (.tar.zst + BLAKE3)
                    </h3>
                    <p class="text-xs text-muted">
                      Create manual checkpoints before branching choices or right-click any snapshot to restore.
                    </p>
                  </div>

                  <UFieldGroup size="md">
                    <UInput
                      v-model="noteInput"
                      placeholder="Checkpoint label (e.g. Choice A)"
                      size="md"
                      class="w-56"
                      @keyup.enter="handleCreateCheckpoint"
                    />
                    <UButton
                      label="Snapshot"
                      icon="i-heroicons-archive-box-arrow-down"
                      color="primary"
                      size="md"
                      :loading="isBusy"
                      @click="handleCreateCheckpoint"
                    />
                  </UFieldGroup>
                </div>
              </template>

              <UEmpty
                v-if="snapshots.length === 0"
                icon="i-heroicons-archive-box"
                title="No Save Snapshots Yet"
                description="Click 'Snapshot' or 'Sync Saves' to pack your game's save directory into a verified checkpoint."
              />

              <div v-else class="divide-y divide-default">
                <UContextMenu
                  v-for="snap in snapshots"
                  :key="snap.id"
                  :items="getSnapshotContextMenu(snap)"
                >
                  <div
                    class="py-3 first:pt-0 last:pb-0 flex flex-col lg:flex-row lg:items-center justify-between gap-3 hover:bg-elevated/40 px-2 rounded transition-colors"
                    @contextmenu.stop
                  >
                    <div class="space-y-1 min-w-0">
                      <div class="flex items-center gap-2">
                        <span class="text-sm font-semibold text-highlighted truncate">
                          {{ snap.note || 'Save Checkpoint' }}
                        </span>
                        <UBadge
                          v-if="game.headSnapshotId === snap.id"
                          label="HEAD"
                          color="success"
                          variant="subtle"
                          size="sm"
                        />
                        <UBadge
                          v-if="snap.pinned"
                          label="Pinned"
                          icon="i-heroicons-bookmark"
                          color="primary"
                          variant="subtle"
                          size="sm"
                        />
                        <UBadge
                          :label="snap.deviceName"
                          color="neutral"
                          variant="outline"
                          size="sm"
                        />
                      </div>
                      <div class="flex flex-wrap items-center gap-3 text-xs text-muted">
                        <span>{{ new Date(snap.createdAt).toLocaleString() }}</span>
                        <span>•</span>
                        <span>{{ snap.fileCount }} files ({{ formatBytes(snap.totalBytes) }})</span>
                        <span>•</span>
                        <span class="font-mono text-[11px]">{{ snap.treeHash.slice(0, 22) }}</span>
                      </div>
                    </div>

                    <div class="flex items-center gap-1.5 shrink-0">
                      <UButton
                        label="Restore"
                        icon="i-heroicons-arrow-uturn-left"
                        color="neutral"
                        variant="subtle"
                        size="md"
                        :loading="isBusy"
                        @click="handleRestore(snap)"
                      />
                      <UDropdownMenu :items="getSnapshotContextMenu(snap)">
                        <UButton
                          icon="i-heroicons-ellipsis-vertical"
                          color="neutral"
                          variant="ghost"
                          size="md"
                          aria-label="Snapshot actions"
                        />
                      </UDropdownMenu>
                    </div>
                  </div>
                </UContextMenu>
              </div>
            </UCard>
          </template>

          <template #cloud>
            <UCard variant="outline" class="mt-3">
              <template #header>
                <div class="flex items-center justify-between gap-3">
                  <div>
                    <h3 class="text-sm font-semibold text-highlighted">
                      Cloud Save History
                    </h3>
                    <p class="text-xs text-muted">
                      Saves uploaded by every PC. Restore one to download just that save onto this PC.
                    </p>
                  </div>
                  <UButton
                    label="Refresh"
                    icon="i-heroicons-arrow-path"
                    color="neutral"
                    variant="subtle"
                    size="md"
                    :loading="isCloudLoading"
                    @click="loadCloud"
                  />
                </div>
              </template>

              <UAlert
                v-if="cloudError"
                color="error"
                variant="subtle"
                icon="i-heroicons-exclamation-circle"
                title="Could not load cloud saves"
                :description="cloudError"
              />
              <UEmpty
                v-else-if="cloudSnapshots.length === 0 && !isCloudLoading"
                icon="i-heroicons-cloud"
                title="No Cloud Saves Yet"
                description="Click 'Sync Saves' to upload this game's saves."
              />
              <div v-else class="divide-y divide-default">
                <div
                  v-for="(head, idx) in cloudSnapshots"
                  :key="head.headSnapshotId"
                  class="py-3 first:pt-0 last:pb-0 flex flex-col lg:flex-row lg:items-center justify-between gap-3 px-2"
                >
                  <div class="space-y-1 min-w-0">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-semibold text-highlighted truncate">
                        {{ new Date(head.updatedAt).toLocaleString() }}
                      </span>
                      <UBadge v-if="idx === 0" label="Latest" color="success" variant="subtle" size="sm" />
                      <UBadge
                        v-if="game.headSnapshotId === head.headSnapshotId"
                        label="On this PC"
                        color="primary"
                        variant="subtle"
                        size="sm"
                      />
                      <UBadge :label="head.deviceName" color="neutral" variant="outline" size="sm" />
                    </div>
                    <div class="flex flex-wrap items-center gap-3 text-xs text-muted">
                      <span>{{ head.fileCount }} files ({{ formatBytes(head.totalBytes) }})</span>
                      <span>•</span>
                      <span class="font-mono text-[11px]">{{ head.treeHash.slice(0, 22) }}</span>
                    </div>
                  </div>

                  <UButton
                    label="Download & Restore"
                    icon="i-heroicons-cloud-arrow-down"
                    color="neutral"
                    variant="subtle"
                    size="md"
                    :loading="isBusy"
                    @click="handleCloudRestore(head)"
                  />
                </div>
              </div>
            </UCard>
          </template>

          <template #paths>
            <UCard variant="outline" class="mt-3">
              <template #header>
                <div class="flex items-center justify-between">
                  <div>
                    <h3 class="text-sm font-semibold text-highlighted">
                      Cross-Platform Save Path Resolution
                    </h3>
                    <p class="text-xs text-muted">
                      Portable tokens automatically map to native OS directories or Steam Deck Proton prefixes.
                    </p>
                  </div>
                  <UButton
                    label="Edit Paths"
                    icon="i-heroicons-pencil-square"
                    color="neutral"
                    variant="subtle"
                    size="md"
                    @click="isEditOpen = true"
                  />
                </div>
              </template>

              <div class="space-y-3">
                <div
                  v-for="(pattern, idx) in game.savePaths"
                  :key="idx"
                  class="p-3 rounded-lg bg-elevated/60 border border-default space-y-1"
                >
                  <div class="flex items-center justify-between">
                    <span class="text-xs font-mono font-bold text-primary">{{ pattern }}</span>
                    <UBadge label="Canonical Rule" color="primary" variant="subtle" size="sm" />
                  </div>
                  <div class="text-xs font-mono text-muted truncate">
                    Resolved Host Path: {{ resolvedPaths[idx] }}
                  </div>
                </div>

                <div class="p-3 rounded-lg bg-elevated/30 border border-default text-xs space-y-2">
                  <div class="flex flex-col lg:flex-row lg:items-center justify-between gap-2">
                    <div>
                      <div class="font-semibold text-highlighted flex items-center gap-2">
                        <span>Executable Path</span>
                        <UBadge
                          :label="game.exePath && game.exeExists !== false ? 'Found' : 'Missing on this user/PC'"
                          :color="game.exePath && game.exeExists !== false ? 'success' : 'warning'"
                          variant="subtle"
                          size="sm"
                        />
                      </div>
                      <div class="font-mono text-muted mt-0.5">{{ game.exePath || 'Not configured' }}</div>
                    </div>
                    <div class="flex items-center gap-1.5 shrink-0">
                      <UButton
                        label="Select Folder"
                        icon="i-heroicons-folder-open"
                        color="neutral"
                        variant="subtle"
                        size="md"
                        @click="handleLocateGame(true)"
                      />
                      <UButton
                        label="Select .exe"
                        icon="i-heroicons-command-line"
                        color="neutral"
                        variant="outline"
                        size="md"
                        @click="handleLocateGame(false)"
                      />
                    </div>
                  </div>
                  <template v-if="game.protonPrefix">
                    <div class="font-semibold text-highlighted pt-2">Wine / Proton Prefix</div>
                    <div class="font-mono text-muted">{{ game.protonPrefix }}</div>
                  </template>
                </div>
              </div>
            </UCard>
          </template>
        </UTabs>
      </div>

      <EditGameSlideover
        v-model:open="isEditOpen"
        :game="game"
        @saved="loadAll"
      />
    </template>
  </UDashboardPanel>
</template>

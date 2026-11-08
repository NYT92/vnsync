<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useRouter } from 'vue-router'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { open, save } from '@tauri-apps/plugin-dialog'
import type { ContextMenuItem, DropdownMenuItem } from '@nuxt/ui'
import { VnSyncApi, formatPlaytime, type Game } from '@/services/vnsyncApi'
import EditGameSlideover from '@/components/EditGameSlideover.vue'

const router = useRouter()
const toast = useToast()

const games = ref<Game[]>([])
const runningIds = ref<Set<string>>(new Set())
const loading = ref(true)
const syncingId = ref<string | null>(null)
const syncingAll = ref(false)
const searchQuery = ref('')
const activeFilter = ref<'All' | 'Playing' | 'Completed' | 'Conflict'>('All')

const editingGame = ref<Game | null>(null)
const isEditOpen = ref(false)

let unlistenStarted: UnlistenFn | null = null
let unlistenExited: UnlistenFn | null = null

const isGameRunning = (id: string) => runningIds.value.has(id)
const isExeReady = (game: Game) => Boolean(game.exePath && game.exeExists !== false)

const filterOptions: Array<'All' | 'Playing' | 'Completed' | 'Conflict'> = [
  'All',
  'Playing',
  'Completed',
  'Conflict'
]

const statuses = ['Not Started', 'Playing', 'Completed', 'On Hold', 'Dropped']

const loadGames = async () => {
  loading.value = true
  try {
    const [loadedGames, activeIds] = await Promise.all([
      VnSyncApi.getGames(),
      VnSyncApi.getRunningGames()
    ])
    games.value = loadedGames
    runningIds.value = new Set(activeIds)
  } catch (err) {
    toast.add({
      title: 'Failed to load library',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  } finally {
    loading.value = false
  }
}

onMounted(async () => {
  await loadGames()
  try {
    unlistenStarted = await listen<string>('game:started', event => {
      const next = new Set(runningIds.value)
      next.add(event.payload)
      runningIds.value = next
    })
    unlistenExited = await listen<string>('game:exited', async event => {
      const next = new Set(runningIds.value)
      next.delete(event.payload)
      runningIds.value = next
      await loadGames()
    })
  } catch {
    // non-tauri preview fallback
  }
})

onUnmounted(() => {
  unlistenStarted?.()
  unlistenExited?.()
})

const filteredGames = computed(() => {
  return games.value.filter(g => {
    const matchesSearch =
      !searchQuery.value.trim() ||
      g.title.toLowerCase().includes(searchQuery.value.toLowerCase()) ||
      g.developer.toLowerCase().includes(searchQuery.value.toLowerCase()) ||
      g.engine.toLowerCase().includes(searchQuery.value.toLowerCase())

    if (!matchesSearch) return false
    if (activeFilter.value === 'All') return true
    if (activeFilter.value === 'Conflict') return g.syncStatus === 'Conflict'
    return g.status === activeFilter.value
  })
})

const totalPlaytimeLabel = computed(() => {
  const totalSecs = games.value.reduce((acc, g) => acc + (g.playtimeSeconds || 0), 0)
  return formatPlaytime(totalSecs)
})

const stats = computed(() => [
  {
    label: 'Library Size',
    value: games.value.length,
    icon: 'i-heroicons-rectangle-stack'
  },
  {
    label: 'Active Playthroughs',
    value: games.value.filter(g => g.status === 'Playing').length,
    icon: 'i-heroicons-play-circle'
  },
  {
    label: 'Cloud Synced',
    value: games.value.filter(g => g.syncStatus === 'Synced').length,
    icon: 'i-heroicons-cloud-arrow-up'
  },
  {
    label: 'Total Playtime',
    value: totalPlaytimeLabel.value,
    icon: 'i-heroicons-clock'
  }
])

const openEditSlideover = (game: Game, event?: Event) => {
  event?.stopPropagation()
  editingGame.value = game
  isEditOpen.value = true
}

const handleQuickSync = async (
  game: Game,
  event?: Event,
  forceDirection?: 'push' | 'pull'
) => {
  event?.stopPropagation()
  syncingId.value = game.id
  try {
    const res = await VnSyncApi.syncGameNow(game.id, forceDirection)
    toast.add({
      title: game.title,
      description: res.message,
      color: res.action === 'conflict' ? 'error' : 'success',
      icon:
        res.action === 'conflict'
          ? 'i-heroicons-exclamation-triangle'
          : 'i-heroicons-check-circle'
    })
    await loadGames()
    if (res.action === 'conflict') {
      router.push(`/game/${game.id}`)
    }
  } catch (err: any) {
    toast.add({
      title: `Sync failed for ${game.title}`,
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  } finally {
    syncingId.value = null
  }
}

const handleQuickSnapshot = async (game: Game) => {
  try {
    const snap = await VnSyncApi.createManualSnapshot(game.id, 'Quick checkpoint')
    toast.add({
      title: 'Checkpoint Created',
      description: `${game.title}: Packed ${snap.fileCount} save files.`,
      color: 'success',
      icon: 'i-heroicons-archive-box-arrow-down'
    })
    await loadGames()
  } catch (err: any) {
    toast.add({
      title: 'Snapshot Failed',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  }
}

const handleChangeStatus = async (game: Game, newStatus: string) => {
  try {
    const updated: Game = {
      ...game,
      status: newStatus,
      progress: newStatus === 'Completed' ? 100 : game.progress
    }
    await VnSyncApi.saveGame(updated)
    toast.add({
      title: 'Status Updated',
      description: `${game.title} marked as ${newStatus}.`,
      color: 'primary',
      icon: 'i-heroicons-tag'
    })
    await loadGames()
  } catch (err: any) {
    toast.add({
      title: 'Failed updating status',
      description: String(err),
      color: 'error'
    })
  }
}

const handleDeleteGame = async (game: Game) => {
  try {
    await VnSyncApi.removeGame(game.id)
    toast.add({
      title: 'Game Removed',
      description: `${game.title} removed from VNSync library.`,
      color: 'neutral',
      icon: 'i-heroicons-trash'
    })
    await loadGames()
  } catch (err: any) {
    toast.add({
      title: 'Failed removing game',
      description: String(err),
      color: 'error'
    })
  }
}

const handleLocateGame = async (game: Game, pickFolder = true, event?: Event) => {
  event?.stopPropagation()
  try {
    const selected = await open({
      directory: pickFolder,
      multiple: false,
      title: pickFolder
        ? `Select Game Folder for ${game.title}`
        : `Select Executable for ${game.title}`
    })
    if (typeof selected === 'string' && selected) {
      const updated = await VnSyncApi.relocateGamePath(game.id, selected)
      toast.add({
        title: 'Game Location Updated',
        description: `${updated.title} linked to ${updated.exePath}`,
        color: 'success',
        icon: 'i-heroicons-folder-open'
      })
      await loadGames()
    }
  } catch (err: any) {
    toast.add({
      title: 'Could not locate game executable',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  }
}

const handleSyncLibrary = async (direction?: 'push' | 'pull') => {
  syncingAll.value = true
  try {
    const res = await VnSyncApi.syncLibraryNow(direction)
    toast.add({
      title: 'Library Catalog Synced',
      description: res.message,
      color: res.missingExeCount > 0 ? 'warning' : 'success',
      icon: 'i-heroicons-rectangle-stack'
    })
    await loadGames()
  } catch (err: any) {
    toast.add({
      title: 'Library Sync Failed',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  } finally {
    syncingAll.value = false
  }
}

const handleExportLibrary = async () => {
  try {
    const dest = await save({
      title: 'Export Game Library Backup',
      defaultPath: 'vnsync-library-backup.json',
      filters: [{ name: 'JSON Backup', extensions: ['json'] }]
    })
    if (dest) {
      const msg = await VnSyncApi.exportLibraryBackup(dest)
      toast.add({
        title: 'Library Backup Exported',
        description: msg,
        color: 'success',
        icon: 'i-heroicons-arrow-down-tray'
      })
    }
  } catch (err: any) {
    toast.add({
      title: 'Export Failed',
      description: String(err),
      color: 'error'
    })
  }
}

const handleImportLibrary = async () => {
  try {
    const selected = await open({
      multiple: false,
      title: 'Import Game Library Backup',
      filters: [{ name: 'JSON Backup', extensions: ['json'] }]
    })
    if (typeof selected === 'string' && selected) {
      const res = await VnSyncApi.importLibraryBackup(selected)
      toast.add({
        title: 'Library Backup Imported',
        description: res.message,
        color: res.missingExeCount > 0 ? 'warning' : 'success',
        icon: 'i-heroicons-arrow-up-tray'
      })
      await loadGames()
    }
  } catch (err: any) {
    toast.add({
      title: 'Import Failed',
      description: String(err),
      color: 'error'
    })
  }
}

const handleHealPaths = async () => {
  try {
    const res = await VnSyncApi.healGamePaths()
    toast.add({
      title: 'User Paths Checked',
      description: res.message,
      color: res.remappedPaths > 0 ? 'success' : 'neutral',
      icon: 'i-heroicons-wrench-screwdriver'
    })
    await loadGames()
  } catch (err: any) {
    toast.add({
      title: 'Path Check Failed',
      description: String(err),
      color: 'error'
    })
  }
}

const libraryMenuItems = computed<DropdownMenuItem[][]>(() => [
  [
    {
      label: 'Sync Library Catalog (Cloud)',
      icon: 'i-heroicons-arrows-right-left',
      onSelect: () => handleSyncLibrary()
    },
    {
      label: 'Backup Library to Cloud',
      icon: 'i-heroicons-cloud-arrow-up',
      onSelect: () => handleSyncLibrary('push')
    },
    {
      label: 'Restore Library from Cloud',
      icon: 'i-heroicons-cloud-arrow-down',
      onSelect: () => handleSyncLibrary('pull')
    }
  ],
  [
    {
      label: 'Export Library Backup (.json)...',
      icon: 'i-heroicons-arrow-down-tray',
      onSelect: () => handleExportLibrary()
    },
    {
      label: 'Import Library Backup (.json)...',
      icon: 'i-heroicons-arrow-up-tray',
      onSelect: () => handleImportLibrary()
    }
  ],
  [
    {
      label: 'Auto-Fix Wrong User Paths',
      icon: 'i-heroicons-wrench-screwdriver',
      onSelect: () => handleHealPaths()
    }
  ]
])

const getGameContextMenu = (game: Game): ContextMenuItem[][] => [
  [
    {
      label: game.title,
      type: 'label'
    },
    {
      label: isGameRunning(game.id) ? 'Running...' : 'Play Game',
      icon: 'i-heroicons-play-solid',
      disabled: !isExeReady(game) || isGameRunning(game.id),
      onSelect: () => handleLaunch(game)
    },
    {
      label: 'Locate Game Folder...',
      icon: 'i-heroicons-folder-open',
      onSelect: () => handleLocateGame(game, true)
    },
    {
      label: 'Select Game Executable (.exe)...',
      icon: 'i-heroicons-command-line',
      onSelect: () => handleLocateGame(game, false)
    },
    {
      label: 'Edit Game & Metadata...',
      icon: 'i-heroicons-pencil-square',
      onSelect: () => openEditSlideover(game)
    },
    {
      label: 'Open Save Vault & History',
      icon: 'i-heroicons-archive-box',
      onSelect: () => router.push(`/game/${game.id}`)
    }
  ],
  [
    {
      label: 'Sync Saves Now',
      icon: 'i-heroicons-cloud-arrow-up',
      onSelect: () => handleQuickSync(game)
    },
    {
      label: 'Create Save Checkpoint',
      icon: 'i-heroicons-archive-box-arrow-down',
      onSelect: () => handleQuickSnapshot(game)
    },
    {
      label: 'Advanced Cloud Sync',
      icon: 'i-heroicons-arrows-right-left',
      children: [
        {
          label: 'Force Push Local to Cloud',
          icon: 'i-heroicons-arrow-up-tray',
          onSelect: () => handleQuickSync(game, undefined, 'push')
        },
        {
          label: 'Force Pull Cloud to Local',
          icon: 'i-heroicons-arrow-down-tray',
          onSelect: () => handleQuickSync(game, undefined, 'pull')
        }
      ]
    }
  ],
  [
    {
      label: 'Set Play Status',
      icon: 'i-heroicons-tag',
      children: statuses.map(st => ({
        label: st,
        type: 'checkbox' as const,
        checked: game.status === st,
        onSelect: () => handleChangeStatus(game, st)
      }))
    }
  ],
  [
    {
      label: 'Remove from Library',
      icon: 'i-heroicons-trash',
      color: 'error',
      onSelect: () => handleDeleteGame(game)
    }
  ]
]

const handleSyncAll = async () => {
  syncingAll.value = true
  try {
    await VnSyncApi.syncLibraryNow().catch(() => null)
    await loadGames()
  } catch {
    // ignore catalog sync error if no cloud configured
  }
  let syncedCount = 0
  for (const g of games.value) {
    try {
      await VnSyncApi.syncGameNow(g.id)
      syncedCount++
    } catch {
      // individual errors ignored in batch
    }
  }
  await loadGames()
  syncingAll.value = false
  toast.add({
    title: 'Library & Save Sync Finished',
    description: `Synced library catalog and ${syncedCount} of ${games.value.length} games.`,
    color: 'primary',
    icon: 'i-heroicons-cloud-arrow-up'
  })
}

const handleLaunch = async (game: Game, event?: Event) => {
  event?.stopPropagation()
  if (isGameRunning(game.id)) return
  const next = new Set(runningIds.value)
  next.add(game.id)
  runningIds.value = next
  try {
    const msg = await VnSyncApi.launchGame(game.id)
    toast.add({
      title: 'Game Launched',
      description: msg,
      color: 'success',
      icon: 'i-heroicons-play'
    })
  } catch (err: any) {
    const revert = new Set(runningIds.value)
    revert.delete(game.id)
    runningIds.value = revert
    toast.add({
      title: 'Could not launch executable',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  }
}

const syncBadgeColor = (status: string) => {
  if (status === 'Synced') return 'success'
  if (status === 'Conflict') return 'error'
  return 'neutral'
}
</script>

<template>
  <UDashboardPanel id="library-panel">
    <template #header>
      <UDashboardNavbar title="Game Library" :badge="games.length">
        <template #leading>
          <UDashboardSidebarCollapse />
        </template>

        <template #right>
          <UDropdownMenu :items="libraryMenuItems">
            <UButton
              label="Library Backup"
              icon="i-heroicons-rectangle-stack"
              trailing-icon="i-heroicons-chevron-down"
              color="neutral"
              variant="outline"
              size="lg"
            />
          </UDropdownMenu>
          <UButton
            label="Sync All"
            icon="i-heroicons-arrow-path"
            color="neutral"
            variant="subtle"
            size="lg"
            :loading="syncingAll"
            @click="handleSyncAll"
          />
          <UButton
            label="Add Game"
            icon="i-heroicons-plus"
            color="primary"
            size="lg"
            to="/add-game"
          />
        </template>
      </UDashboardNavbar>

      <UDashboardToolbar>
        <template #left>
          <UInput
            v-model="searchQuery"
            icon="i-heroicons-magnifying-glass"
            placeholder="Filter by title, developer, or engine..."
            size="lg"
            class="w-64 lg:w-80"
          />
        </template>

        <template #right>
          <UFieldGroup size="lg">
            <UButton
              v-for="filter in filterOptions"
              :key="filter"
              :label="filter"
              size="lg"
              :color="activeFilter === filter ? 'primary' : 'neutral'"
              :variant="activeFilter === filter ? 'subtle' : 'ghost'"
              @click="activeFilter = filter"
            />
          </UFieldGroup>
        </template>
      </UDashboardToolbar>
    </template>

    <template #body>
      <div class="space-y-6">
        <!-- Compact Stat Strip -->
        <div class="grid grid-cols-1 lg:grid-cols-4 gap-3">
          <UCard
            v-for="stat in stats"
            :key="stat.label"
            variant="subtle"
          >
            <div class="flex items-center justify-between gap-2">
              <div class="min-w-0">
                <p class="text-xs font-medium text-muted truncate">
                  {{ stat.label }}
                </p>
                <p class="text-xl font-bold text-highlighted mt-0.5">
                  {{ stat.value }}
                </p>
              </div>
              <UAvatar :icon="stat.icon" size="md" class="bg-primary/10 text-primary shrink-0" />
            </div>
          </UCard>
        </div>

        <!-- Empty State -->
        <UCard v-if="filteredGames.length === 0 && !loading" variant="subtle" class="py-8">
          <UEmpty
            icon="i-tabler-device-gamepad"
            :title="games.length === 0 ? 'Your VNSync Library is Empty' : 'No Matching Games'"
            :description="
              games.length === 0
                ? 'Add a Visual Novel or RPG executable to auto-detect save folders (Ren\'Py, RPG Maker, Kirikiri, Unity), fetch VNDB cover art, and sync saves across Windows & Steam Deck.'
                : 'Try clearing your search filter or status tab.'
            "
            :actions="
              games.length === 0
                ? [
                    {
                      label: 'Add First Game',
                      icon: 'i-heroicons-plus-circle',
                      color: 'primary',
                      to: '/add-game'
                    },
                    {
                      label: 'Restore Library from Cloud',
                      icon: 'i-heroicons-cloud-arrow-down',
                      color: 'neutral',
                      variant: 'subtle',
                      onClick: () => handleSyncLibrary('pull')
                    },
                    {
                      label: 'Import Library JSON',
                      icon: 'i-heroicons-arrow-up-tray',
                      color: 'neutral',
                      variant: 'outline',
                      onClick: () => handleImportLibrary()
                    }
                  ]
                : [
                    {
                      label: 'Reset Filters',
                      icon: 'i-heroicons-x-mark',
                      color: 'neutral',
                      variant: 'subtle',
                      onClick: () => {
                        searchQuery = ''
                        activeFilter = 'All'
                      }
                    }
                  ]
            "
          />
        </UCard>

        <!-- Poster Library Grid with Right-Click Context Menu -->
        <div
          v-else
          class="grid grid-cols-1 lg:grid-cols-3 xl:grid-cols-4 gap-4"
        >
          <UContextMenu
            v-for="game in filteredGames"
            :key="game.id"
            :items="getGameContextMenu(game)"
            :ui="{ content: 'w-60' }"
          >
            <UCard
              variant="outline"
              class="group cursor-pointer hover:ring-2 hover:ring-primary/50 transition-all"
              @click="router.push(`/game/${game.id}`)"
              @contextmenu.stop
            >
              <div class="relative -mx-4 -mt-4 mb-3 overflow-hidden rounded-t-lg bg-elevated h-48">
                <img
                  :src="
                    game.thumbnail ||
                    'https://placehold.co/400x600/1e293b/e2e8f0?text=' +
                      encodeURIComponent(game.title)
                  "
                  :alt="game.title"
                  class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
                />
                <div class="absolute inset-0 bg-gradient-to-t from-black/75 via-black/15 to-transparent" />

                <div class="absolute top-2.5 left-2.5 flex items-center gap-1.5">
                  <UBadge :label="game.engine || 'Generic'" color="neutral" variant="solid" size="lg" />
                </div>

                <div class="absolute top-2.5 right-2.5">
                  <UBadge
                    :label="game.syncStatus || 'Local Only'"
                    :color="syncBadgeColor(game.syncStatus)"
                    variant="solid"
                    size="lg"
                  />
                </div>

                <div class="absolute bottom-2.5 left-3 right-3">
                  <h3 class="text-sm font-bold text-white truncate">
                    {{ game.title }}
                  </h3>
                  <p class="text-[11px] text-gray-300 truncate">
                    {{ game.developer || 'Unknown Studio' }}
                  </p>
                </div>
              </div>

              <div class="space-y-3">
                <div class="flex items-center justify-between text-xs text-muted">
                  <span class="flex items-center gap-1">
                    <UIcon name="i-heroicons-clock" class="size-3.5" />
                    {{ formatPlaytime(game.playtimeSeconds) }}
                  </span>
                  <UBadge :label="game.status" color="neutral" variant="subtle" size="lg" />
                </div>

                <UProgress :model-value="game.progress" size="xs" color="primary" />

                <div class="flex items-center gap-1.5 pt-1">
                  <UButton
                    v-if="isExeReady(game)"
                    :label="isGameRunning(game.id) ? 'Running...' : 'Play'"
                    :icon="isGameRunning(game.id) ? 'i-heroicons-Bolt' : 'i-heroicons-play-solid'"
                    :color="isGameRunning(game.id) ? 'success' : 'primary'"
                    :variant="isGameRunning(game.id) ? 'subtle' : 'solid'"
                    :loading="isGameRunning(game.id)"
                    size="lg"
                    class="flex-1 justify-center"
                    :disabled="isGameRunning(game.id)"
                    @click="handleLaunch(game, $event)"
                  />
                  <UButton
                    v-else
                    label="Locate Folder"
                    icon="i-heroicons-folder-open"
                    color="warning"
                    variant="subtle"
                    size="lg"
                    class="flex-1 justify-center"
                    @click="handleLocateGame(game, true, $event)"
                  />
                  <UTooltip text="Sync saves now">
                    <UButton
                      icon="i-heroicons-cloud-arrow-up"
                      color="neutral"
                      variant="subtle"
                      size="lg"
                      :loading="syncingId === game.id"
                      @click="handleQuickSync(game, $event)"
                    />
                  </UTooltip>
                  <UTooltip text="Edit game & metadata">
                    <UButton
                      icon="i-heroicons-pencil-square"
                      color="neutral"
                      variant="ghost"
                      size="lg"
                      @click="openEditSlideover(game, $event)"
                    />
                  </UTooltip>
                </div>
              </div>
            </UCard>
          </UContextMenu>
        </div>
      </div>

      <!-- Reusable Edit Game & Metadata Slideover -->
      <EditGameSlideover
        v-model:open="isEditOpen"
        :game="editingGame"
        @saved="loadGames"
      />
    </template>
  </UDashboardPanel>
</template>

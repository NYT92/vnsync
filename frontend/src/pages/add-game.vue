<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { open } from '@tauri-apps/plugin-dialog'
import {
  VnSyncApi,
  type VndbResult,
  type Game,
  type SaveLocationEntry
} from '@/services/vnsyncApi'

const router = useRouter()
const toast = useToast()

const vndbQuery = ref('')
const vndbResults = ref<VndbResult[]>([])
const isSearchingVndb = ref(false)

const form = ref<Game>({
  id: '',
  title: '',
  originalTitle: '',
  vndbId: '',
  engine: 'Custom / Generic',
  developer: '',
  publisher: '',
  releaseDate: '',
  genre: 'Visual Novel',
  rating: '',
  description: '',
  status: 'Not Started',
  progress: 0,
  thumbnail: '',
  exePath: '',
  protonPrefix: '',
  savePaths: ['<game_dir>/save'],
  playtimeSeconds: 0,
  lastPlayedAt: null,
  headSnapshotId: null,
  syncStatus: 'Local Only'
})

const resolvedPreview = ref<string[]>([])
const detectedFileCount = ref<number>(0)
const newSavePath = ref('')
const isSubmitting = ref(false)
const savePresets = ref<SaveLocationEntry[]>([])
const saveDbSource = ref<'default' | 'custom'>('default')
const selectedPresetId = ref<string>('')

const engines = [
  'Ren\'Py',
  'TyranoBuilder',
  'Nitroplus / NPK',
  'Nitroplus / NPA',
  'RPG Maker MV',
  'RPG Maker MZ',
  'RPG Maker VX / XP',
  'Kirikiri / KAG',
  'Unity',
  'BGI / Ethornell',
  'SiglusEngine',
  'MAGES. Engine',
  'Custom / Generic'
]

const statuses = ['Not Started', 'Playing', 'Completed', 'On Hold', 'Dropped']

const presetSelectItems = computed(() =>
  savePresets.value.map(p => ({
    label: p.developer ? `${p.title} (${p.developer})` : p.title,
    value: p.id
  }))
)

const loadPresetDatabase = async () => {
  try {
    const status = await VnSyncApi.getSaveLocationDb()
    savePresets.value = status.entries
    saveDbSource.value = status.source
  } catch (err) {
    console.error('Failed loading save location DB:', err)
  }
}

onMounted(loadPresetDatabase)

const applySavePreset = async (presetId: string) => {
  const preset = savePresets.value.find(p => p.id === presetId)
  if (!preset) return
  if (preset.engine) form.value.engine = preset.engine
  if (!form.value.title && !preset.title.includes('Template')) {
    form.value.title = preset.title
  }
  if (!form.value.developer && preset.developer && preset.developer !== 'Generic') {
    form.value.developer = preset.developer
  }
  const titleToken = form.value.title.trim() || preset.title
  form.value.savePaths = preset.savePaths.map(p => p.replace('<game_title>', titleToken))
  await refreshPreview()
  toast.add({
    title: 'Save Location Applied',
    description: preset.notes || `Loaded ${preset.savePaths.length} save path rules for ${preset.title}.`,
    color: 'primary',
    icon: 'i-heroicons-folder-open'
  })
}

const handleLoadCustomJson = async () => {
  try {
    const selected = await open({
      multiple: false,
      title: 'Select Custom Save Locations JSON',
      filters: [{ name: 'JSON Database', extensions: ['json'] }]
    })
    if (typeof selected === 'string' && selected) {
      const status = await VnSyncApi.loadCustomSaveLocations(selected)
      savePresets.value = status.entries
      saveDbSource.value = status.source
      toast.add({
        title: 'Custom Save Database Loaded',
        description: `Loaded ${status.entries.length} save location presets from custom JSON.`,
        color: 'success',
        icon: 'i-heroicons-check-circle'
      })
    }
  } catch (err: any) {
    toast.add({
      title: 'Invalid Save Locations JSON',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  }
}

const handleResetDefaultJson = async () => {
  try {
    const status = await VnSyncApi.resetSaveLocations()
    savePresets.value = status.entries
    saveDbSource.value = status.source
    selectedPresetId.value = ''
    toast.add({
      title: 'Restored Default Save Database',
      description: `Using built-in database (${status.entries.length} presets).`,
      color: 'neutral',
      icon: 'i-heroicons-arrow-path'
    })
  } catch (err: any) {
    console.error('Reset save locations failed:', err)
  }
}

const refreshPreview = async () => {
  resolvedPreview.value = await VnSyncApi.previewResolvedPaths(
    form.value.savePaths,
    form.value.exePath,
    form.value.protonPrefix
  )
}

const handleSearchVndb = async () => {
  if (!vndbQuery.value.trim()) return
  isSearchingVndb.value = true
  try {
    vndbResults.value = await VnSyncApi.searchVndb(vndbQuery.value)
    if (vndbResults.value.length === 0) {
      toast.add({
        title: 'No VNDB results',
        description: `No visual novels matched "${vndbQuery.value}".`,
        color: 'neutral',
        icon: 'i-heroicons-magnifying-glass'
      })
    }
  } catch (err: any) {
    toast.add({
      title: 'VNDB Search Error',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  } finally {
    isSearchingVndb.value = false
  }
}

const applyVndbEntry = async (entry: VndbResult) => {
  form.value.title = entry.title
  form.value.originalTitle = entry.alttitle
  form.value.vndbId = entry.id
  form.value.developer = entry.developer
  form.value.releaseDate = entry.released
  form.value.description = entry.description
  if (entry.imageUrl) {
    form.value.thumbnail = entry.imageUrl
  }
  vndbResults.value = []

  // Auto-match against Save Location Database by VNDB ID, title, or Nitroplus/Nitro+CHiRAL studio
  const matchedPreset =
    savePresets.value.find(p => p.vndbId && p.vndbId === entry.id) ||
    savePresets.value.find(p => p.title.toLowerCase() === entry.title.toLowerCase()) ||
    (entry.developer?.toLowerCase().includes('chiral')
      ? savePresets.value.find(p => p.id === 'nitro-chiral-generic')
      : entry.developer?.toLowerCase().includes('nitro')
        ? savePresets.value.find(p => p.id === 'nitroplus-generic')
        : undefined)

  if (matchedPreset) {
    selectedPresetId.value = matchedPreset.id
    await applySavePreset(matchedPreset.id)
  }

  toast.add({
    title: 'Metadata Applied',
    description: `Populated metadata from ${entry.title} (${entry.id})`,
    color: 'success',
    icon: 'i-heroicons-sparkles'
  })
}

const browseExecutable = async () => {
  try {
    const selected = await open({
      multiple: false,
      title: 'Select Game Executable'
    })
    if (typeof selected === 'string' && selected) {
      form.value.exePath = selected
      await runEngineAutoDetect()
    }
  } catch (err) {
    console.error('File dialog error:', err)
  }
}

const runEngineAutoDetect = async () => {
  if (!form.value.exePath.trim()) return
  try {
    const det = await VnSyncApi.detectGameEngine(
      form.value.exePath,
      form.value.protonPrefix
    )
    form.value.engine = det.engine
    if (!form.value.title) {
      form.value.title = det.suggestedTitle
    }
    form.value.savePaths = det.canonicalSavePaths
    resolvedPreview.value = det.resolvedSavePaths
    detectedFileCount.value = det.existingSaveFiles
    toast.add({
      title: `Detected ${det.engine}`,
      description: `Found ${det.existingSaveFiles} existing save files in resolved path.`,
      color: 'primary',
      icon: 'i-heroicons-cpu-chip'
    })
  } catch (err) {
    console.error('Auto-detect failed:', err)
  }
}

const browseSaveFolder = async () => {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Select Save Data Directory'
    })
    if (typeof selected === 'string' && selected) {
      form.value.savePaths.push(selected)
      await refreshPreview()
    }
  } catch (err) {
    console.error('Folder dialog error:', err)
  }
}

const browseSaveFile = async () => {
  try {
    const selected = await open({
      directory: false,
      multiple: true,
      title: 'Select Save File(s)'
    })
    if (Array.isArray(selected)) {
      form.value.savePaths.push(...selected)
      await refreshPreview()
    } else if (typeof selected === 'string' && selected) {
      form.value.savePaths.push(selected)
      await refreshPreview()
    }
  } catch (err) {
    console.error('File dialog error:', err)
  }
}

const addCustomSavePattern = async () => {
  if (!newSavePath.value.trim()) return
  form.value.savePaths.push(newSavePath.value.trim())
  newSavePath.value = ''
  await refreshPreview()
}

const removeSavePath = async (idx: number) => {
  form.value.savePaths.splice(idx, 1)
  await refreshPreview()
}

const handleSubmit = async () => {
  if (!form.value.title.trim()) return
  isSubmitting.value = true
  try {
    const saved = await VnSyncApi.saveGame(form.value)
    toast.add({
      title: 'Game Added',
      description: `${saved.title} added to your library.`,
      color: 'success',
      icon: 'i-heroicons-check-circle'
    })
    router.push(`/game/${saved.id}`)
  } catch (err) {
    toast.add({
      title: 'Failed to save game',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  } finally {
    isSubmitting.value = false
  }
}
</script>

<template>
  <UDashboardPanel id="add-game-panel">
    <template #header>
      <UDashboardNavbar title="Add Visual Novel or RPG">
        <template #leading>
          <UDashboardSidebarCollapse />
        </template>

        <template #right>
          <UButton
            label="Cancel"
            color="neutral"
            variant="ghost"
            size="lg"
            to="/"
          />
          <UButton
            label="Save to Library"
            icon="i-heroicons-check"
            color="primary"
            size="lg"
            :loading="isSubmitting"
            :disabled="!form.title.trim()"
            @click="handleSubmit"
          />
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="grid grid-cols-1 lg:grid-cols-12 gap-6 max-w-6xl mx-auto">
        <!-- Left Column: VNDB Lookup & Executable Detector -->
        <div class="lg:col-span-7 space-y-6">
          <UCard variant="outline">
            <template #header>
              <div class="flex items-center justify-between">
                <div class="flex items-center gap-2">
                  <UIcon name="i-heroicons-sparkles" class="size-5 text-primary" />
                  <h2 class="text-sm font-semibold text-highlighted">
                    Auto-Fill from VNDB
                  </h2>
                </div>
                <UBadge label="VNDB API v2" color="primary" variant="subtle" size="lg" />
              </div>
            </template>

            <div class="space-y-4">
              <UFormField
                label="Search Visual Novel Title"
                description="Fetch official cover art, developer, release date, and synopsis."
              >
                <UFieldGroup class="w-full" size="lg">
                  <UInput
                    v-model="vndbQuery"
                    icon="i-heroicons-magnifying-glass"
                    placeholder="e.g. Steins;Gate, Fate/stay night, Clannad..."
                    size="lg"
                    class="flex-1"
                    @keyup.enter="handleSearchVndb"
                  />
                  <UButton
                    label="Search"
                    color="primary"
                    variant="subtle"
                    size="lg"
                    :loading="isSearchingVndb"
                    @click="handleSearchVndb"
                  />
                </UFieldGroup>
              </UFormField>

              <div
                v-if="vndbResults.length > 0"
                class="grid grid-cols-1 lg:grid-cols-2 gap-2.5 max-h-64 overflow-y-auto pr-1"
              >
                <UCard
                  v-for="item in vndbResults"
                  :key="item.id"
                  variant="subtle"
                  class="cursor-pointer hover:ring-2 hover:ring-primary/50 transition-all"
                  @click="applyVndbEntry(item)"
                >
                  <div class="flex gap-3 items-center">
                    <img
                      v-if="item.imageUrl"
                      :src="item.imageUrl"
                      :alt="item.title"
                      class="w-12 h-16 object-cover rounded shrink-0"
                    />
                    <div class="min-w-0 flex-1">
                      <p class="text-xs font-bold text-highlighted truncate">
                        {{ item.title }}
                      </p>
                      <p class="text-[11px] text-muted truncate">
                        {{ item.developer || item.alttitle }}
                      </p>
                      <div class="flex items-center gap-1.5 mt-1">
                        <UBadge :label="item.id" size="lg" color="neutral" variant="outline" />
                        <span class="text-[10px] text-muted">{{ item.released }}</span>
                      </div>
                    </div>
                  </div>
                </UCard>
              </div>
            </div>
          </UCard>

          <UCard variant="outline">
            <template #header>
              <div class="flex items-center justify-between">
                <div class="flex items-center gap-2">
                  <UIcon name="i-heroicons-cpu-chip" class="size-5 text-primary" />
                  <h2 class="text-sm font-semibold text-highlighted">
                    Executable & Cross-Platform Save Paths
                  </h2>
                </div>
                <UBadge
                  :label="`${form.engine} • ${detectedFileCount} files`"
                  color="success"
                  variant="subtle"
                  size="lg"
                />
              </div>
            </template>

            <div class="space-y-4">
              <UFormField
                label="Game Executable"
                description="Select the game's .exe or binary to automatically fingerprint its engine and save folder."
              >
                <UFieldGroup class="w-full" size="lg">
                  <UInput
                    v-model="form.exePath"
                    icon="i-heroicons-command-line"
                    placeholder="C:\Games\MyVN\Game.exe"
                    size="lg"
                    class="flex-1"
                    @change="runEngineAutoDetect"
                  />
                  <UButton
                    label="Browse"
                    icon="i-heroicons-folder-open"
                    color="neutral"
                    variant="subtle"
                    size="lg"
                    @click="browseExecutable"
                  />
                  <UButton
                    label="Detect"
                    icon="i-heroicons-bolt"
                    color="primary"
                    variant="soft"
                    size="lg"
                    @click="runEngineAutoDetect"
                  />
                </UFieldGroup>
              </UFormField>

              <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
                <UFormField label="Detected Engine Profile">
                  <USelect v-model="form.engine" :items="engines" size="lg" class="w-full" />
                </UFormField>

                <UFormField
                  label="Wine / Proton Prefix"
                  hint="Linux / Steam Deck"
                >
                  <UInput
                    v-model="form.protonPrefix"
                    placeholder="/home/deck/.../compatdata/12345"
                    size="lg"
                    class="w-full"
                    @change="refreshPreview"
                  />
                </UFormField>
              </div>

              <USeparator label="Portable Save Path Rules" />

              <div class="p-3 rounded-lg bg-elevated/40 border border-default space-y-2.5">
                <div class="flex flex-wrap items-center justify-between gap-2">
                  <div class="flex items-center gap-2">
                    <span class="text-xs font-semibold text-highlighted">
                      Save Location Database
                    </span>
                    <UBadge
                      :label="saveDbSource === 'custom' ? `Custom JSON (${savePresets.length})` : `Default JSON (${savePresets.length})`"
                      :color="saveDbSource === 'custom' ? 'warning' : 'primary'"
                      variant="subtle"
                      size="lg"
                    />
                  </div>
                  <div class="flex items-center gap-1.5">
                    <UButton
                      label="Load Custom JSON"
                      icon="i-heroicons-arrow-up-tray"
                      color="neutral"
                      variant="subtle"
                      size="lg"
                      @click="handleLoadCustomJson"
                    />
                    <UButton
                      v-if="saveDbSource === 'custom'"
                      label="Use Default"
                      icon="i-heroicons-arrow-path"
                      color="neutral"
                      variant="ghost"
                      size="lg"
                      @click="handleResetDefaultJson"
                    />
                  </div>
                </div>

                <USelect
                  v-model="selectedPresetId"
                  :items="presetSelectItems"
                  placeholder="Select game / engine preset (e.g. Nitro+CHiRAL, Nitroplus, STEINS;GATE, Key...)"
                  class="w-full"
                  size="lg"
                  @update:model-value="applySavePreset"
                />
              </div>

              <div class="space-y-2">
                <div
                  v-for="(pattern, idx) in form.savePaths"
                  :key="idx"
                  class="flex items-center justify-between gap-2 px-3 py-2 rounded-md bg-elevated/60 border border-default"
                >
                  <div class="min-w-0 flex-1">
                    <p class="text-xs font-mono font-semibold text-primary truncate">
                      {{ pattern }}
                    </p>
                    <p v-if="resolvedPreview[idx]" class="text-[11px] font-mono text-muted truncate">
                      {{ resolvedPreview[idx] }}
                    </p>
                  </div>
                  <UButton
                    icon="i-heroicons-trash"
                    color="error"
                    variant="ghost"
                    size="lg"
                    @click="removeSavePath(idx)"
                  />
                </div>
              </div>

              <UFieldGroup class="w-full" size="lg">
                <UInput
                  v-model="newSavePath"
                  placeholder="e.g. <game_dir>/*.sav, <win_appdata>/RenPy/GameID, or <game_dir>/savedata"
                  class="flex-1"
                  size="lg"
                  @keyup.enter="addCustomSavePattern"
                />
                <UButton
                  label="Add Rule"
                  color="neutral"
                  variant="subtle"
                  size="lg"
                  @click="addCustomSavePattern"
                />
                <UButton
                  label="Pick File"
                  icon="i-heroicons-document-plus"
                  color="neutral"
                  variant="outline"
                  size="lg"
                  @click="browseSaveFile"
                />
                <UButton
                  label="Pick Folder"
                  icon="i-heroicons-folder-plus"
                  color="neutral"
                  variant="outline"
                  size="lg"
                  @click="browseSaveFolder"
                />
              </UFieldGroup>
            </div>
          </UCard>
        </div>

        <!-- Right Column: Game Metadata Form & Live Card Preview -->
        <div class="lg:col-span-5 space-y-6">
          <UCard variant="outline">
            <template #header>
              <div class="flex items-center gap-2">
                <UIcon name="i-heroicons-document-text" class="size-5 text-primary" />
                <h2 class="text-sm font-semibold text-highlighted">
                  Game Details
                </h2>
              </div>
            </template>

            <div class="space-y-4">
              <div v-if="form.thumbnail" class="flex gap-4 items-center p-2.5 rounded-lg bg-elevated/50">
                <img
                  :src="form.thumbnail"
                  :alt="form.title"
                  class="w-16 h-22 object-cover rounded shadow-sm shrink-0"
                />
                <div class="min-w-0">
                  <p class="text-sm font-bold text-highlighted truncate">
                    {{ form.title || 'Untitled Game' }}
                  </p>
                  <p class="text-xs text-muted truncate">
                    {{ form.developer || 'Unknown Developer' }}
                  </p>
                  <UBadge
                    v-if="form.vndbId"
                    :label="`VNDB ${form.vndbId}`"
                    color="primary"
                    variant="subtle"
                    size="lg"
                    class="mt-1"
                  />
                </div>
              </div>

              <UFormField label="Game Title" required>
                <UInput v-model="form.title" placeholder="Enter game title" size="lg" class="w-full" />
              </UFormField>

              <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
                <UFormField label="Developer / Studio">
                  <UInput v-model="form.developer" placeholder="e.g. Key, TYPE-MOON" size="lg" class="w-full" />
                </UFormField>

                <UFormField label="Play Status">
                  <USelect v-model="form.status" :items="statuses" size="lg" class="w-full" />
                </UFormField>
              </div>

              <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
                <UFormField label="Release Date">
                  <UInput v-model="form.releaseDate" type="date" size="lg" class="w-full" />
                </UFormField>

                <UFormField label="Original Title">
                  <UInput v-model="form.originalTitle" placeholder="Japanese / Original title" size="lg" class="w-full" />
                </UFormField>
              </div>

              <UFormField label="Cover Image URL">
                <UInput v-model="form.thumbnail" placeholder="https://..." size="lg" class="w-full" />
              </UFormField>

              <UFormField label="Synopsis / Notes">
                <UTextarea v-model="form.description" :rows="3" size="lg" class="w-full" />
              </UFormField>

              <UButton
                label="Add Game to VNSync"
                icon="i-heroicons-plus-circle"
                color="primary"
                size="lg"
                block
                :loading="isSubmitting"
                :disabled="!form.title.trim()"
                @click="handleSubmit"
              />
            </div>
          </UCard>
        </div>
      </div>
    </template>
  </UDashboardPanel>
</template>

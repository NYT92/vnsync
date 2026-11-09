<script setup lang="ts">
import { ref, computed, watch } from 'vue'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import {
  VnSyncApi,
  type Game,
  type VndbResult,
  type SaveLocationEntry
} from '@/services/vnsyncApi'

const props = defineProps<{
  game: Game | null
}>()

const isOpen = defineModel<boolean>('open', { default: false })

const emit = defineEmits<{
  saved: [game: Game]
}>()

const toast = useToast()

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
  savePaths: [],
  playtimeSeconds: 0,
  lastPlayedAt: null,
  headSnapshotId: null,
  syncStatus: 'Local Only'
})

const vndbQuery = ref('')
const vndbResults = ref<VndbResult[]>([])
const isSearchingVndb = ref(false)
const resolvedPreview = ref<string[]>([])
const newSavePath = ref('')
const isSaving = ref(false)

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

const savePresets = ref<SaveLocationEntry[]>([])
const selectedPresetId = ref('')

const presetSelectItems = computed(() =>
  savePresets.value.map(p => ({
    label: p.developer ? `${p.title} (${p.developer})` : p.title,
    value: p.id
  }))
)

const applySavePreset = async (presetId: string) => {
  const preset = savePresets.value.find(p => p.id === presetId)
  if (!preset) return
  if (preset.engine) form.value.engine = preset.engine
  const titleToken = form.value.title.trim() || preset.title
  form.value.savePaths = preset.savePaths.map(p => p.replace('<game_title>', titleToken))
  await refreshPreview()
}

const refreshPreview = async () => {
  resolvedPreview.value = await VnSyncApi.previewResolvedPaths(
    form.value.savePaths,
    form.value.exePath,
    form.value.protonPrefix
  )
}

watch(
  () => [props.game, isOpen.value] as const,
  async ([g, openState]) => {
    if (g && openState) {
      form.value = {
        ...g,
        savePaths: [...(g.savePaths || [])]
      }
      vndbQuery.value = g.title
      vndbResults.value = []
      selectedPresetId.value = ''
      const db = await VnSyncApi.getSaveLocationDb().catch(() => null)
      if (db) savePresets.value = db.entries
      await refreshPreview()
    }
  },
  { immediate: true }
)

const handleSearchVndb = async () => {
  if (!vndbQuery.value.trim()) return
  isSearchingVndb.value = true
  try {
    vndbResults.value = await VnSyncApi.searchVndb(vndbQuery.value)
  } catch (err: any) {
    toast.add({
      title: 'VNDB Search Failed',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  } finally {
    isSearchingVndb.value = false
  }
}

const applyVndbEntry = (entry: VndbResult) => {
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
  toast.add({
    title: 'VNDB Metadata Applied',
    description: `Updated fields from ${entry.title} (${entry.id})`,
    color: 'success',
    icon: 'i-heroicons-sparkles'
  })
}

const browseExecutable = async () => {
  try {
    const selected = await openDialog({
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

const browseGameFolder = async () => {
  if (!form.value.id) return
  try {
    const selected = await openDialog({
      directory: true,
      multiple: false,
      title: 'Select Game Installation Folder'
    })
    if (typeof selected === 'string' && selected) {
      const updated = await VnSyncApi.relocateGamePath(form.value.id, selected)
      form.value.exePath = updated.exePath
      form.value.savePaths = updated.savePaths
      form.value.engine = updated.engine
      await refreshPreview()
      toast.add({
        title: 'Game Folder Linked',
        description: `Located ${updated.exePath}`,
        color: 'success',
        icon: 'i-heroicons-folder-open'
      })
    }
  } catch (err: any) {
    toast.add({
      title: 'No executable found in folder',
      description: String(err),
      color: 'error'
    })
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
    form.value.savePaths = det.canonicalSavePaths
    resolvedPreview.value = det.resolvedSavePaths
    toast.add({
      title: `Detected ${det.engine}`,
      description: `Found ${det.existingSaveFiles} files in resolved save folder.`,
      color: 'primary',
      icon: 'i-heroicons-cpu-chip'
    })
  } catch (err) {
    console.error('Auto-detect failed:', err)
  }
}

const browseSaveFolder = async () => {
  try {
    const selected = await openDialog({
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
    const selected = await openDialog({
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

const handleSave = async () => {
  if (!form.value.title.trim()) return
  isSaving.value = true
  try {
    const updated = await VnSyncApi.saveGame(form.value)
    toast.add({
      title: 'Game Updated',
      description: `Saved changes to ${updated.title}.`,
      color: 'success',
      icon: 'i-heroicons-check-circle'
    })
    isOpen.value = false
    emit('saved', updated)
  } catch (err: any) {
    toast.add({
      title: 'Failed to update game',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-circle'
    })
  } finally {
    isSaving.value = false
  }
}
</script>

<template>
  <USlideover
    v-model:open="isOpen"
    title="Edit Game & Metadata"
    description="Update VNDB metadata, cover art, progress, executable path, or cross-platform save rules."
    :ui="{ content: 'top-8 max-h-[calc(100%-2rem)]', overlay: 'top-8' }"
  >
    <template #body>
      <div class="space-y-5">
        <!-- Quick VNDB Re-fetch -->
        <UCard variant="subtle">
          <div class="space-y-3">
            <UFormField label="Fetch / Refresh from VNDB">
              <UFieldGroup class="w-full" size="lg">
                <UInput
                  v-model="vndbQuery"
                  icon="i-heroicons-magnifying-glass"
                  placeholder="Search VNDB..."
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
              class="space-y-1.5 max-h-48 overflow-y-auto pr-1"
            >
              <div
                v-for="item in vndbResults"
                :key="item.id"
                class="flex items-center gap-2.5 p-2 rounded-md bg-default hover:ring-1 hover:ring-primary cursor-pointer"
                @click="applyVndbEntry(item)"
              >
                <img
                  v-if="item.imageUrl"
                  :src="item.imageUrl"
                  :alt="item.title"
                  class="w-9 h-12 object-cover rounded shrink-0"
                />
                <div class="min-w-0 flex-1">
                  <p class="text-xs font-bold text-highlighted truncate">
                    {{ item.title }}
                  </p>
                  <p class="text-[11px] text-muted truncate">
                    {{ item.developer }} • {{ item.released }} ({{ item.id }})
                  </p>
                </div>
              </div>
            </div>
          </div>
        </UCard>

        <!-- Metadata Fields -->
        <div class="space-y-4">
          <UFormField label="Title" required>
            <UInput v-model="form.title" size="lg" class="w-full" />
          </UFormField>

          <div class="grid grid-cols-1 lg:grid-cols-2 gap-3">
            <UFormField label="Original Title">
              <UInput v-model="form.originalTitle" size="lg" class="w-full" />
            </UFormField>

            <UFormField label="Developer">
              <UInput v-model="form.developer" size="lg" class="w-full" />
            </UFormField>
          </div>

          <div class="grid grid-cols-1 lg:grid-cols-2 gap-3">
            <UFormField label="Status">
              <USelect v-model="form.status" :items="statuses" size="lg" class="w-full" />
            </UFormField>

            <UFormField :label="`Completion (${form.progress}%)`">
              <UInput
                v-model.number="form.progress"
                type="number"
                min="0"
                max="100"
                size="lg"
                class="w-full"
              />
            </UFormField>
          </div>

          <div class="grid grid-cols-1 lg:grid-cols-2 gap-3">
            <UFormField label="Release Date">
              <UInput v-model="form.releaseDate" type="date" size="lg" class="w-full" />
            </UFormField>

            <UFormField label="VNDB ID">
              <UInput v-model="form.vndbId" placeholder="e.g. v17" size="lg" class="w-full" />
            </UFormField>
          </div>

          <UFormField label="Cover Image URL">
            <UInput v-model="form.thumbnail" placeholder="https://..." size="lg" class="w-full" />
          </UFormField>

          <UFormField label="Description / Notes">
            <UTextarea v-model="form.description" :rows="3" size="lg" class="w-full" />
          </UFormField>
        </div>

        <USeparator label="Executable & Save Rules" />

        <div class="space-y-4">
          <UFormField label="Game Executable">
            <UFieldGroup class="w-full" size="lg">
              <UInput v-model="form.exePath" size="lg" class="flex-1" />
              <UButton
                label="Folder"
                icon="i-heroicons-folder-open"
                color="neutral"
                variant="subtle"
                size="lg"
                @click="browseGameFolder"
              />
              <UButton
                label=".exe"
                icon="i-heroicons-command-line"
                color="neutral"
                variant="outline"
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

          <div class="grid grid-cols-1 lg:grid-cols-2 gap-3">
            <UFormField label="Engine">
              <USelect v-model="form.engine" :items="engines" size="lg" class="w-full" />
            </UFormField>

            <UFormField label="Wine / Proton Prefix">
              <UInput
                v-model="form.protonPrefix"
                placeholder="Optional Linux prefix"
                size="lg"
                class="w-full"
                @change="refreshPreview"
              />
            </UFormField>
          </div>

          <div class="space-y-2">
            <label class="text-xs font-medium text-highlighted">Save Path Rules</label>

            <USelect
              v-model="selectedPresetId"
              :items="presetSelectItems"
              placeholder="Apply preset from Save Location Database..."
              class="w-full"
              size="lg"
              @update:model-value="applySavePreset"
            />

            <div
              v-for="(pattern, idx) in form.savePaths"
              :key="idx"
              class="flex items-center justify-between gap-2 px-2.5 py-1.5 rounded bg-elevated/60 border border-default"
            >
              <div class="min-w-0 flex-1">
                <p class="text-xs font-mono font-semibold text-primary truncate">
                  {{ pattern }}
                </p>
                <p v-if="resolvedPreview[idx]" class="text-[10px] font-mono text-muted truncate">
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

            <UFieldGroup class="w-full" size="lg">
              <UInput
                v-model="newSavePath"
                placeholder="<game_dir>/*.sav, <win_appdata>/RenPy/..., or <game_dir>/save"
                size="lg"
                class="flex-1"
                @keyup.enter="addCustomSavePattern"
              />
              <UButton
                label="Add"
                color="neutral"
                variant="subtle"
                size="lg"
                @click="addCustomSavePattern"
              />
              <UButton
                label="File"
                icon="i-heroicons-document-plus"
                color="neutral"
                variant="outline"
                size="lg"
                @click="browseSaveFile"
              />
              <UButton
                label="Folder"
                icon="i-heroicons-folder-plus"
                color="neutral"
                variant="outline"
                size="lg"
                @click="browseSaveFolder"
              />
            </UFieldGroup>
          </div>
        </div>
      </div>
    </template>

    <template #footer>
      <div class="flex justify-end gap-2 w-full">
        <UButton
          label="Cancel"
          color="neutral"
          variant="ghost"
          size="lg"
          @click="isOpen = false"
        />
        <UButton
          label="Save Changes"
          icon="i-heroicons-check"
          color="primary"
          size="lg"
          :loading="isSaving"
          :disabled="!form.title.trim()"
          @click="handleSave"
        />
      </div>
    </template>
  </USlideover>
</template>

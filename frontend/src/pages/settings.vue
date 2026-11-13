<script setup lang="ts">
import { ref, onMounted } from 'vue'
import type { TabsItem } from '@nuxt/ui'
import { open } from '@tauri-apps/plugin-dialog'
import {
  VnSyncApi,
  type AppSettings,
  type StorageProfile
} from '@/services/vnsyncApi'

const toast = useToast()

const tabItems: TabsItem[] = [
  {
    label: 'Cloud & Storage Backend',
    icon: 'i-heroicons-cloud-arrow-up',
    slot: 'storage'
  },
  {
    label: 'Device & Auto-Sync',
    icon: 'i-heroicons-cpu-chip',
    slot: 'automation'
  }
]

const settings = ref<AppSettings>({
  deviceId: '',
  deviceName: 'My-PC',
  theme: 'system',
  language: 'en',
  autoSyncOnLaunch: true,
  autoSyncOnExit: true,
  backupBeforeSync: true,
  maxLocalSnapshots: 20,
  enableLanP2p: true,
  lanPort: 47832,
  gameLibraryPath: '',
  closeToTray: true,
  minimizeToTray: false,
  askOnClose: true
})

const activeProfile = ref<StorageProfile>({
  id: 'default-storage',
  name: 'Primary Cloud Storage',
  providerType: 's3',
  endpoint: '',
  bucket: '',
  region: 'auto',
  rootPrefix: 'vnsync',
  accessKey: '',
  secretKey: '',
  isActive: true
})

const providerOptions = [
  { label: 'S3-Compatible (Cloudflare R2, AWS S3, MinIO, Backblaze B2)', value: 's3' },
  { label: 'WebDAV (Nextcloud, ownCloud, Synology, Self-Hosted)', value: 'webdav' },
  { label: 'Local / Shared Folder (Syncthing, Google Drive, NAS)', value: 'local_fs' },
  { label: 'VNSync Server (Self-Hosted)', value: 'vnsync_server' }
]

const isSaving = ref(false)
const isTesting = ref(false)

onMounted(async () => {
  try {
    const loadedSettings = await VnSyncApi.getAppSettings()
    if (loadedSettings) {
      settings.value = loadedSettings
    }
    const profiles = await VnSyncApi.getStorageProfiles()
    const current = profiles.find(p => p.isActive) || profiles[0]
    if (current) {
      activeProfile.value = current
    }
  } catch (err) {
    console.error('Failed loading settings:', err)
  }
})

const browseLocalSyncFolder = async () => {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Select Sync Mirror Folder'
    })
    if (typeof selected === 'string' && selected) {
      activeProfile.value.endpoint = selected
    }
  } catch (err) {
    console.error('Folder dialog error:', err)
  }
}

const browseGameLibraryRoot = async () => {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Select Default Game Library Root Folder'
    })
    if (typeof selected === 'string' && selected) {
      settings.value.gameLibraryPath = selected
    }
  } catch (err) {
    console.error('Folder dialog error:', err)
  }
}

const handleHealPaths = async () => {
  try {
    await VnSyncApi.saveAppSettings(settings.value)
    const res = await VnSyncApi.healGamePaths()
    toast.add({
      title: 'Library Paths Updated',
      description: res.message,
      color: res.remappedPaths > 0 ? 'success' : 'neutral',
      icon: 'i-heroicons-wrench-screwdriver'
    })
  } catch (err: any) {
    toast.add({
      title: 'Failed checking paths',
      description: String(err),
      color: 'error'
    })
  }
}

const handleTestConnection = async () => {
  isTesting.value = true
  try {
    const msg = await VnSyncApi.testStorageProfile(activeProfile.value)
    toast.add({
      title: 'Connection Verified',
      description: msg,
      color: 'success',
      icon: 'i-heroicons-check-circle'
    })
  } catch (err: any) {
    toast.add({
      title: 'Connection Failed',
      description: String(err),
      color: 'error',
      icon: 'i-heroicons-exclamation-triangle'
    })
  } finally {
    isTesting.value = false
  }
}

const handleSaveAll = async () => {
  isSaving.value = true
  try {
    activeProfile.value.isActive = true
    await VnSyncApi.saveStorageProfile(activeProfile.value)
    await VnSyncApi.saveAppSettings(settings.value)
    toast.add({
      title: 'Settings Saved',
      description: 'Storage profile and automation rules updated.',
      color: 'success',
      icon: 'i-heroicons-check-circle'
    })
  } catch (err: any) {
    toast.add({
      title: 'Failed Saving Settings',
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
  <UDashboardPanel id="settings-panel">
    <template #header>
      <UDashboardNavbar title="Cloud & Application Settings">
        <template #leading>
          <UDashboardSidebarCollapse />
        </template>

        <template #right>
          <UButton
            label="Test Backend"
            icon="i-heroicons-signal"
            color="neutral"
            variant="subtle"
            size="lg"
            :loading="isTesting"
            @click="handleTestConnection"
          />
          <UButton
            label="Save Changes"
            icon="i-heroicons-check"
            color="primary"
            size="lg"
            :loading="isSaving"
            @click="handleSaveAll"
          />
        </template>
      </UDashboardNavbar>
    </template>

    <template #body>
      <div class="max-w-4xl mx-auto">
        <UTabs :items="tabItems" class="w-full">
          <!-- Tab 1: Storage Backend -->
          <template #storage>
            <UCard variant="outline" class="mt-4">
              <template #header>
                <div class="flex items-center justify-between">
                  <div>
                    <h2 class="text-sm font-semibold text-highlighted">
                      Storage Provider Configuration
                    </h2>
                    <p class="text-xs text-muted">
                      Store encrypted/compressed & save snapshots on S3, R2, WebDAV, or a synced folder.
                    </p>
                  </div>
                  <UBadge
                    :label="activeProfile.providerType.toUpperCase()"
                    color="primary"
                    variant="subtle"
                    size="sm"
                  />
                </div>
              </template>

              <div class="space-y-4">
                <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
                  <UFormField label="Profile Display Name">
                    <UInput
                      v-model="activeProfile.name"
                      placeholder="e.g. Cloudflare R2 Saves"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>

                  <UFormField label="Storage Backend Type">
                    <USelect
                      v-model="activeProfile.providerType"
                      :items="providerOptions"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>
                </div>

                <USeparator />

                <!-- S3 Config -->
                <div
                  v-if="activeProfile.providerType === 's3'"
                  class="grid grid-cols-1 lg:grid-cols-2 gap-4"
                >
                  <UFormField
                    label="Endpoint URL"
                    help="Leave blank for AWS S3, or enter Cloudflare R2 / MinIO URL."
                  >
                    <UInput
                      v-model="activeProfile.endpoint"
                      placeholder="https://<account>.r2.cloudflarestorage.com"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>

                  <UFormField label="Bucket Name" required>
                    <UInput
                      v-model="activeProfile.bucket"
                      placeholder="vnsync-vault"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>

                  <UFormField label="Region">
                    <UInput
                      v-model="activeProfile.region"
                      placeholder="auto"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>

                  <UFormField label="Root Folder Prefix">
                    <UInput
                      v-model="activeProfile.rootPrefix"
                      placeholder="vnsync"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>

                  <UFormField label="Access Key ID">
                    <UInput v-model="activeProfile.accessKey" size="md" class="w-full" />
                  </UFormField>

                  <UFormField label="Secret Access Key">
                    <UInput
                      v-model="activeProfile.secretKey"
                      type="password"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>
                </div>

                <!-- WebDAV Config -->
                <div
                  v-if="activeProfile.providerType === 'webdav'"
                  class="grid grid-cols-1 lg:grid-cols-2 gap-4"
                >
                  <UFormField
                    label="WebDAV Endpoint URL"
                    class="lg:col-span-2"
                    help="Full WebDAV root URL (Nextcloud, ownCloud, Synology, Koofr)."
                  >
                    <UInput
                      v-model="activeProfile.endpoint"
                      placeholder="https://cloud.example.com/remote.php/dav/files/username/"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>

                  <UFormField label="Username">
                    <UInput v-model="activeProfile.accessKey" size="md" class="w-full" />
                  </UFormField>

                  <UFormField label="Password / App Password">
                    <UInput
                      v-model="activeProfile.secretKey"
                      type="password"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>
                </div>

                <!-- VNSync Server Config -->
                <div
                  v-if="activeProfile.providerType === 'vnsync_server'"
                  class="grid grid-cols-1 lg:grid-cols-2 gap-4"
                >
                  <UFormField
                    label="Server URL"
                    help="Address of your vnsync-server. Use HTTPS outside your home network."
                  >
                    <UInput
                      v-model="activeProfile.endpoint"
                      placeholder="http://192.168.1.10:3000"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>

                  <UFormField label="Token" help="The SECRET value from the server's .env file.">
                    <UInput
                      v-model="activeProfile.secretKey"
                      type="password"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>
                </div>

                <!-- Local / Shared Folder Config -->
                <div v-if="activeProfile.providerType === 'local_fs'">
                  <UFormField
                    label="Local / Network Sync Folder"
                    description="Point VNSync to a Syncthing, Google Drive, Dropbox, OneDrive, or SMB network directory."
                  >
                    <UFieldGroup class="w-full" size="md">
                      <UInput
                        v-model="activeProfile.endpoint"
                        placeholder="C:\Users\You\Syncthing\VNSync"
                        size="md"
                        class="flex-1"
                      />
                      <UButton
                        label="Browse"
                        icon="i-heroicons-folder-open"
                        color="neutral"
                        variant="subtle"
                        size="md"
                        @click="browseLocalSyncFolder"
                      />
                    </UFieldGroup>
                  </UFormField>
                </div>
              </div>

              <template #footer>
                <div class="flex flex-col lg:flex-row lg:items-center justify-between gap-3">
                  <span class="text-xs text-muted">
                    Test read/write access before enabling automatic sync.
                  </span>
                  <div class="flex gap-2">
                    <UButton
                      label="Test Connection"
                      icon="i-heroicons-signal"
                      color="neutral"
                      variant="subtle"
                      size="md"
                      :loading="isTesting"
                      @click="handleTestConnection"
                    />
                    <UButton
                      label="Save Backend"
                      icon="i-heroicons-check"
                      color="primary"
                      size="md"
                      :loading="isSaving"
                      @click="handleSaveAll"
                    />
                  </div>
                </div>
              </template>
            </UCard>
          </template>

          <!-- Tab 2: Device Identity & Auto-Sync -->
          <template #automation>
            <UCard variant="outline" class="mt-4">
              <template #header>
                <div>
                  <h2 class="text-sm font-semibold text-highlighted">
                    Device Identity & Sync Lifecycle
                  </h2>
                  <p class="text-xs text-muted">
                    Configure how VNSync tags snapshots from this machine and handles game launch/exit events.
                  </p>
                </div>
              </template>

              <div class="space-y-5">
                <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
                  <UFormField
                    label="Device Name"
                    description="Displayed on every save snapshot in version history."
                  >
                    <UInput
                      v-model="settings.deviceName"
                      placeholder="Desktop-Win11 or Steam-Deck"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>

                  <UFormField
                    label="Max Local Vault Snapshots"
                    description="Number of historical checkpoints retained per game."
                  >
                    <UInput
                      v-model.number="settings.maxLocalSnapshots"
                      type="number"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>
                </div>

                <UFormField
                  label="Default Game Library Root Folder"
                  description="Used to auto-locate synced games when moving across PCs or OS user accounts."
                >
                  <UFieldGroup class="w-full" size="md">
                    <UInput
                      v-model="settings.gameLibraryPath"
                      placeholder="C:\Users\You\Games or D:\VisualNovels"
                      size="md"
                      class="flex-1"
                    />
                    <UButton
                      label="Browse"
                      icon="i-heroicons-folder-open"
                      color="neutral"
                      variant="subtle"
                      size="md"
                      @click="browseGameLibraryRoot"
                    />
                    <UButton
                      label="Remap User Paths"
                      icon="i-heroicons-wrench-screwdriver"
                      color="primary"
                      variant="soft"
                      size="md"
                      @click="handleHealPaths"
                    />
                  </UFieldGroup>
                </UFormField>

                <USeparator label="Automatic Sync Rules" />

                <div class="space-y-4">
                  <USwitch
                    v-model="settings.autoSyncOnLaunch"
                    label="Pull latest saves on Game Launch"
                    description="Automatically check active storage and fast-forward pull newer saves before starting the game executable."
                  />

                  <USwitch
                    v-model="settings.autoSyncOnExit"
                    label="Snapshot & Push on Game Exit"
                    description="Wait for the game process to close, pack changed saves into a .tar.zst snapshot, and upload."
                  />

                  <USwitch
                    v-model="settings.backupBeforeSync"
                    label="Pre-Restore Safety Vault Backup"
                    description="Always create a local snapshot before overwriting local files during a pull or manual restore."
                  />
                </div>

                <USeparator label="System Tray & Window Behavior" />

                <div class="space-y-4">
                  <USwitch
                    v-model="settings.closeToTray"
                    label="Hide to System Tray on Close"
                    description="Keep VNSync running in the system tray when closing the window instead of exiting."
                  />

                  <USwitch
                    v-model="settings.askOnClose"
                    label="Ask Before Closing"
                    description="Prompt whether to minimize to the system tray or exit when clicking the close button."
                  />

                  <USwitch
                    v-model="settings.minimizeToTray"
                    label="Hide to System Tray on Minimize"
                    description="Hide the window to the system tray instead of the taskbar when minimizing."
                  />
                </div>
              </div>

              <template #footer>
                <div class="flex justify-end">
                  <UButton
                    label="Save Preferences"
                    icon="i-heroicons-check"
                    color="primary"
                    size="md"
                    :loading="isSaving"
                    @click="handleSaveAll"
                  />
                </div>
              </template>
            </UCard>
          </template>
        </UTabs>
      </div>
    </template>
  </UDashboardPanel>
</template>

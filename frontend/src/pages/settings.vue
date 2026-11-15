<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import type { TabsItem } from '@nuxt/ui'
import { open } from '@tauri-apps/plugin-dialog'
import {
  VnSyncApi,
  type AppSettings,
  type ServerDevice,
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
  },
  {
    label: 'Security',
    icon: 'i-heroicons-lock-closed',
    slot: 'security'
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
  askOnClose: true,
  syncOnAppOpen: true,
  backgroundSyncMinutes: 0,
  launchOnStartup: false,
  startMinimized: true,
  lockIdleMinutes: 5,
  lockBiometric: false
})

const backgroundSyncOptions = [
  { label: 'Off', value: 0 },
  { label: 'Every 5 minutes', value: 5 },
  { label: 'Every 15 minutes', value: 15 },
  { label: 'Every 30 minutes', value: 30 },
  { label: 'Every hour', value: 60 },
  { label: 'Every 3 hours', value: 180 }
]

const lockIdleOptions = [
  { label: 'Only when the app starts', value: 0 },
  { label: 'After 1 minute idle', value: 1 },
  { label: 'After 5 minutes idle', value: 5 },
  { label: 'After 15 minutes idle', value: 15 },
  { label: 'After 30 minutes idle', value: 30 },
  { label: 'After 1 hour idle', value: 60 }
]

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

const errorToast = (title: string, err: unknown) => {
  toast.add({ title, description: String(err), color: 'error', icon: 'i-heroicons-exclamation-circle' })
}

// VNSync server sign-in. The server password is sent once to get a device token, never stored.
const serverPassword = ref('')
const isSigningIn = ref(false)
const devices = ref<ServerDevice[]>([])
const signedIn = computed(() => !!activeProfile.value.secretKey)
const thisDeviceListed = computed(() => devices.value.some(d => d.current))

const loadDevices = async () => {
  if (activeProfile.value.providerType !== 'vnsync_server' || !signedIn.value) return
  try {
    devices.value = await VnSyncApi.serverGetDevices()
  } catch (err) {
    devices.value = []
    errorToast('Could not load devices', err)
  }
}

const handleServerSignIn = async () => {
  isSigningIn.value = true
  try {
    await VnSyncApi.saveAppSettings(settings.value) // the server shows this device name
    activeProfile.value = await VnSyncApi.serverSignIn(activeProfile.value, serverPassword.value)
    serverPassword.value = ''
    toast.add({
      title: 'Signed in',
      description: `${settings.value.deviceName} is now registered on the server.`,
      color: 'success',
      icon: 'i-heroicons-check-circle'
    })
    await loadDevices()
  } catch (err) {
    errorToast('Sign in failed', err)
  } finally {
    isSigningIn.value = false
  }
}

const handleServerSignOut = async () => {
  try {
    activeProfile.value = await VnSyncApi.serverSignOut()
    devices.value = []
  } catch (err) {
    errorToast('Sign out failed', err)
  }
}

const handleRemoveDevice = async (device: ServerDevice) => {
  if (device.current) return handleServerSignOut()
  try {
    await VnSyncApi.serverRemoveDevice(device.id)
    await loadDevices()
  } catch (err) {
    errorToast('Could not sign out device', err)
  }
}

const formatDate = (iso: string) => new Date(iso).toLocaleString()

// App lock PIN. PIN changes apply at once; idle time and biometric save with the preferences.
const lockEnabled = ref(false)
const biometricAvailable = ref(false)
const currentPin = ref('')
const newPin = ref('')
const confirmPin = ref('')
const isSavingPin = ref(false)

const refreshLock = async () => {
  lockEnabled.value = (await VnSyncApi.getLockStatus()).enabled
}

const handleSetPin = async () => {
  if (newPin.value !== confirmPin.value) {
    return errorToast('PIN not saved', 'The new PINs do not match')
  }
  isSavingPin.value = true
  try {
    await VnSyncApi.setLockPin(currentPin.value, newPin.value)
    toast.add({
      title: lockEnabled.value ? 'PIN changed' : 'App lock turned on',
      color: 'success',
      icon: 'i-heroicons-lock-closed'
    })
    currentPin.value = newPin.value = confirmPin.value = ''
    await refreshLock()
  } catch (err) {
    errorToast('PIN not saved', err)
  } finally {
    isSavingPin.value = false
  }
}

const handleClearPin = async () => {
  try {
    await VnSyncApi.clearLockPin(currentPin.value)
    currentPin.value = ''
    toast.add({ title: 'App lock turned off', color: 'neutral', icon: 'i-heroicons-lock-open' })
    await refreshLock()
  } catch (err) {
    errorToast('App lock not turned off', err)
  }
}

const handleLockNow = () => VnSyncApi.lockApp().catch(err => errorToast('Could not lock', err))

onMounted(async () => {
  try {
    await refreshLock()
    biometricAvailable.value = await VnSyncApi.isBiometricAvailable()
  } catch (err) {
    console.error('Failed loading lock status:', err)
  }
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
    await loadDevices()
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

                  <UFormField
                    v-if="!signedIn"
                    label="Server Password"
                    help="The SECRET value from the server's .env file. Used once to sign in, not stored."
                  >
                    <UFieldGroup class="w-full" size="md">
                      <UInput
                        v-model="serverPassword"
                        type="password"
                        size="md"
                        class="flex-1"
                        @keydown.enter="handleServerSignIn"
                      />
                      <UButton
                        label="Sign In"
                        icon="i-heroicons-arrow-right-end-on-rectangle"
                        color="primary"
                        size="md"
                        :loading="isSigningIn"
                        :disabled="!serverPassword || !activeProfile.endpoint"
                        @click="handleServerSignIn"
                      />
                    </UFieldGroup>
                  </UFormField>

                  <UFormField v-else label="Session" help="This PC is signed in with its own device token.">
                    <UButton
                      label="Sign Out This PC"
                      icon="i-heroicons-arrow-left-start-on-rectangle"
                      color="error"
                      variant="subtle"
                      size="md"
                      @click="handleServerSignOut"
                    />
                  </UFormField>

                  <div v-if="signedIn" class="lg:col-span-2 space-y-2">
                    <div class="flex items-center justify-between">
                      <h3 class="text-xs font-semibold text-highlighted">Signed-In Devices</h3>
                      <UButton
                        label="Refresh"
                        icon="i-heroicons-arrow-path"
                        color="neutral"
                        variant="ghost"
                        size="xs"
                        @click="loadDevices"
                      />
                    </div>
                    <p v-if="devices.length && !thisDeviceListed" class="text-xs text-warning">
                      This PC uses the server password directly, so it is not listed.
                      Sign out and sign in again to register it.
                    </p>
                    <ul class="divide-y divide-default rounded-md border border-default">
                      <li
                        v-for="device in devices"
                        :key="device.id"
                        class="flex items-center justify-between gap-3 px-3 py-2"
                      >
                        <div class="flex items-center gap-2 min-w-0">
                          <UIcon name="i-heroicons-computer-desktop" class="size-4 text-muted shrink-0" />
                          <div class="min-w-0">
                            <p class="text-sm text-highlighted truncate">
                              {{ device.name }}
                              <UBadge
                                v-if="device.current"
                                label="This PC"
                                size="sm"
                                variant="subtle"
                                class="ml-1"
                              />
                            </p>
                            <p class="text-[11px] text-muted truncate">
                              {{ device.os || 'Unknown OS' }} · signed in {{ formatDate(device.createdAt) }}
                              · last seen {{ formatDate(device.lastSeenAt) }}
                            </p>
                          </div>
                        </div>
                        <UButton
                          label="Sign Out"
                          color="error"
                          variant="ghost"
                          size="xs"
                          @click="handleRemoveDevice(device)"
                        />
                      </li>
                      <li v-if="!devices.length" class="px-3 py-2 text-xs text-muted">
                        No devices found.
                      </li>
                    </ul>
                  </div>
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
                    label="Sync before Game Launch"
                    description="Before starting a game, check active storage and fast-forward pull newer saves."
                  />

                  <USwitch
                    v-model="settings.autoSyncOnExit"
                    label="Snapshot on Game Exit"
                    description="When the game closes, pack changed saves into a local .tar.zst snapshot. It uploads on the next sync."
                  />

                  <USwitch
                    v-model="settings.backupBeforeSync"
                    label="Pre-Restore Safety Vault Backup"
                    description="Always create a local snapshot before overwriting local files during a pull or manual restore."
                  />
                </div>

                <USeparator label="App Sync & Startup" />

                <div class="space-y-4">
                  <USwitch
                    v-model="settings.syncOnAppOpen"
                    label="Sync When VNSync Opens"
                    description="Sync the library and every game once when the app starts."
                  />

                  <UFormField
                    label="Background Sync"
                    description="Keep syncing while VNSync runs, also when hidden in the tray. Running games are skipped."
                  >
                    <USelect
                      v-model="settings.backgroundSyncMinutes"
                      :items="backgroundSyncOptions"
                      size="md"
                      class="w-64"
                    />
                  </UFormField>

                  <USwitch
                    v-model="settings.launchOnStartup"
                    label="Launch VNSync on System Startup"
                    description="Start VNSync automatically when you sign in to your computer."
                  />

                  <USwitch
                    v-model="settings.startMinimized"
                    :disabled="!settings.launchOnStartup"
                    label="Start Hidden in System Tray"
                    description="When started at sign-in, stay in the tray instead of opening the window."
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

          <!-- Tab 3: App Lock -->
          <template #security>
            <UCard variant="outline" class="mt-4">
              <template #header>
                <div class="flex items-center justify-between">
                  <div>
                    <h2 class="text-sm font-semibold text-highlighted">App Lock</h2>
                    <p class="text-xs text-muted">
                      Ask for a PIN when VNSync starts and after it sits idle.
                    </p>
                  </div>
                  <UBadge
                    :label="lockEnabled ? 'ON' : 'OFF'"
                    :color="lockEnabled ? 'success' : 'neutral'"
                    variant="subtle"
                    size="sm"
                  />
                </div>
              </template>

              <div class="space-y-5">
                <form class="grid grid-cols-1 lg:grid-cols-3 gap-4" @submit.prevent="handleSetPin">
                  <UFormField v-if="lockEnabled" label="Current PIN">
                    <UInput
                      v-model="currentPin"
                      type="password"
                      inputmode="numeric"
                      autocomplete="off"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>
                  <UFormField :label="lockEnabled ? 'New PIN' : 'PIN'" help="4 to 12 digits.">
                    <UInput
                      v-model="newPin"
                      type="password"
                      inputmode="numeric"
                      autocomplete="off"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>
                  <UFormField label="Confirm PIN">
                    <UInput
                      v-model="confirmPin"
                      type="password"
                      inputmode="numeric"
                      autocomplete="off"
                      size="md"
                      class="w-full"
                    />
                  </UFormField>
                  <div class="lg:col-span-3 flex flex-wrap gap-2">
                    <UButton
                      type="submit"
                      :label="lockEnabled ? 'Change PIN' : 'Turn On App Lock'"
                      icon="i-heroicons-lock-closed"
                      color="primary"
                      size="md"
                      :loading="isSavingPin"
                      :disabled="!newPin || !confirmPin"
                    />
                    <UButton
                      v-if="lockEnabled"
                      label="Turn Off App Lock"
                      icon="i-heroicons-lock-open"
                      color="error"
                      variant="subtle"
                      size="md"
                      :disabled="!currentPin"
                      @click="handleClearPin"
                    />
                    <UButton
                      v-if="lockEnabled"
                      label="Lock Now"
                      icon="i-heroicons-lock-closed"
                      color="neutral"
                      variant="subtle"
                      size="md"
                      @click="handleLockNow"
                    />
                  </div>
                </form>

                <USeparator />

                <UFormField label="Auto-Lock" description="VNSync always locks when it starts.">
                  <USelect
                    v-model="settings.lockIdleMinutes"
                    :items="lockIdleOptions"
                    size="md"
                    class="w-64"
                  />
                </UFormField>

                <USwitch
                  v-model="settings.lockBiometric"
                  :disabled="!biometricAvailable"
                  label="Unlock with Windows Hello"
                  :description="biometricAvailable
                    ? 'Use fingerprint, face or your Windows PIN. The VNSync PIN always works too.'
                    : 'Not available on this device. Only Windows with Windows Hello set up is supported.'"
                />
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

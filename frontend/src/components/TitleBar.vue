<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import AppLogo from './AppLogo.vue'
import { WindowService } from '../services/windowService'
import { VnSyncApi } from '../services/vnsyncApi'

const route = useRoute()
const isMaximized = ref(false)
const isCloseModalOpen = ref(false)
const rememberCloseChoice = ref(false)

const title = computed(() => (route.meta?.title as string) || 'VNSync')

onMounted(async () => {
  isMaximized.value = await WindowService.isMaximized()
})

const minimize = async () => {
  await WindowService.minimize()
}

const toggleMaximize = async () => {
  await WindowService.toggleMaximize()
  isMaximized.value = await WindowService.isMaximized()
}

const close = async () => {
  try {
    const settings = await VnSyncApi.getAppSettings()
    if (settings.askOnClose) {
      rememberCloseChoice.value = false
      isCloseModalOpen.value = true
      return
    }
    if (settings.closeToTray) {
      await WindowService.hideToTray()
    } else {
      await WindowService.quitApp()
    }
  } catch {
    await WindowService.close()
  }
}

const confirmCloseAction = async (action: 'tray' | 'exit') => {
  isCloseModalOpen.value = false
  try {
    if (rememberCloseChoice.value) {
      const settings = await VnSyncApi.getAppSettings()
      settings.closeToTray = action === 'tray'
      settings.askOnClose = false
      await VnSyncApi.saveAppSettings(settings)
    }
  } catch {
    // ignore settings save failure on close
  }

  if (action === 'tray') {
    await WindowService.hideToTray()
  } else {
    await WindowService.quitApp()
  }
}
</script>

<template>
  <header
    class="fixed top-0 inset-x-0 h-8 flex items-center justify-between bg-elevated border-b border-default select-none z-50"
    data-tauri-drag-region
  >
    <div class="flex items-center gap-2 px-3" data-tauri-drag-region>
      <AppLogo class="size-4 text-primary shrink-0" />
      <span class="text-xs font-semibold tracking-tight text-highlighted" data-tauri-drag-region>
        VNSync
      </span>
      <UBadge label="v0.1" size="xs" color="primary" variant="subtle" />
    </div>

    <div class="flex-1 flex justify-center items-center" data-tauri-drag-region>
      <span class="text-xs font-medium text-muted" data-tauri-drag-region>
        {{ title }}
      </span>
    </div>

    <div class="flex items-center h-full">
      <UButton
        icon="i-tabler-minus"
        color="neutral"
        variant="ghost"
        size="xs"
        class="h-8 w-10 rounded-none justify-center"
        aria-label="Minimize"
        @click="minimize"
      />
      <UButton
        :icon="isMaximized ? 'i-tabler-arrows-minimize' : 'i-tabler-square'"
        color="neutral"
        variant="ghost"
        size="xs"
        class="h-8 w-10 rounded-none justify-center"
        aria-label="Maximize"
        @click="toggleMaximize"
      />
      <UButton
        icon="i-tabler-x"
        color="error"
        variant="ghost"
        size="xs"
        class="h-8 w-10 rounded-none justify-center hover:bg-error hover:text-white"
        aria-label="Close"
        @click="close"
      />
    </div>

    <UModal
      v-model:open="isCloseModalOpen"
      title="Close VNSync"
      description="Choose whether to keep VNSync running in the system tray or exit completely."
    >
      <template #body>
        <UCheckbox
          v-model="rememberCloseChoice"
          label="Remember my choice and don't ask again"
          description="You can change this behavior anytime in Cloud & Settings."
        />
      </template>

      <template #footer>
        <div class="flex items-center justify-end gap-2 w-full">
          <UButton
            label="Cancel"
            color="neutral"
            variant="ghost"
            size="lg"
            @click="isCloseModalOpen = false"
          />
          <UButton
            label="Minimize to Tray"
            icon="i-heroicons-arrow-down-on-square"
            color="primary"
            variant="subtle"
            size="lg"
            @click="confirmCloseAction('tray')"
          />
          <UButton
            label="Exit VNSync"
            icon="i-heroicons-power"
            color="error"
            size="lg"
            @click="confirmCloseAction('exit')"
          />
        </div>
      </template>
    </UModal>
  </header>
</template>

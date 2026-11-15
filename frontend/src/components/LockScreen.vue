<script setup lang="ts">
import { ref, onMounted } from 'vue'
import AppLogo from './AppLogo.vue'
import { VnSyncApi } from '../services/vnsyncApi'

const props = defineProps<{ biometric: boolean }>()

const pin = ref('')
const error = ref('')
const busy = ref(false)
const biometricReady = ref(false)

const unlock = async () => {
  if (!pin.value || busy.value) return
  busy.value = true
  try {
    await VnSyncApi.unlockApp(pin.value)
  } catch (err) {
    error.value = String(err)
    pin.value = ''
  } finally {
    busy.value = false
  }
}

const unlockBiometric = async () => {
  busy.value = true
  try {
    await VnSyncApi.unlockAppBiometric()
  } catch (err) {
    error.value = String(err)
  } finally {
    busy.value = false
  }
}

onMounted(async () => {
  biometricReady.value = props.biometric && (await VnSyncApi.isBiometricAvailable())
  if (biometricReady.value) await unlockBiometric()
})
</script>

<template>
  <div class="fixed top-8 inset-x-0 bottom-0 z-50 flex items-center justify-center bg-default">
    <form class="w-72 flex flex-col items-center gap-4" @submit.prevent="unlock">
      <div class="size-14 rounded-full bg-primary/15 text-primary flex items-center justify-center">
        <AppLogo class="size-8" />
      </div>
      <div class="text-center">
        <h1 class="text-lg font-semibold text-highlighted">VNSync is locked</h1>
        <p class="text-xs text-muted">Enter your PIN to continue.</p>
      </div>
      <UInput
        v-model="pin"
        type="password"
        inputmode="numeric"
        autocomplete="off"
        placeholder="PIN"
        size="lg"
        class="w-full"
        autofocus
        aria-label="PIN"
      />
      <p v-if="error" class="text-xs text-error" role="alert">{{ error }}</p>
      <UButton type="submit" label="Unlock" block size="lg" :loading="busy" :disabled="!pin" />
      <UButton
        v-if="biometricReady"
        label="Use Windows Hello"
        icon="i-heroicons-finger-print"
        color="neutral"
        variant="subtle"
        block
        :disabled="busy"
        @click="unlockBiometric"
      />
    </form>
  </div>
</template>

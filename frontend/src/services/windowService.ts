import { getCurrentWindow } from '@tauri-apps/api/window'
import { VnSyncApi } from './vnsyncApi'

function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
}

export class WindowService {
  static async minimize(): Promise<void> {
    if (!isTauri()) return
    try {
      const settings = await VnSyncApi.getAppSettings().catch(() => null)
      if (settings?.minimizeToTray) {
        await VnSyncApi.hideToTray()
      } else {
        await getCurrentWindow().minimize()
      }
    } catch (error) {
      console.error('Failed to minimize window:', error)
    }
  }

  static async hideToTray(): Promise<void> {
    if (!isTauri()) return
    try {
      await VnSyncApi.hideToTray()
    } catch (error) {
      console.error('Failed to hide window to tray:', error)
    }
  }

  static async quitApp(): Promise<void> {
    if (!isTauri()) return
    try {
      await VnSyncApi.quitApp()
    } catch (error) {
      console.error('Failed to quit application:', error)
    }
  }

  static async toggleMaximize(): Promise<void> {
    if (!isTauri()) return
    try {
      await getCurrentWindow().toggleMaximize()
    } catch (error) {
      console.error('Failed to toggle maximize:', error)
    }
  }

  static async close(): Promise<void> {
    if (!isTauri()) return
    try {
      await getCurrentWindow().close()
    } catch (error) {
      console.error('Failed to close application:', error)
    }
  }

  static async isMaximized(): Promise<boolean> {
    if (!isTauri()) return false
    try {
      return await getCurrentWindow().isMaximized()
    } catch (error) {
      console.error('Failed to check maximize state:', error)
      return false
    }
  }

  static async center(): Promise<void> {
    if (!isTauri()) return
    try {
      await getCurrentWindow().center()
    } catch (error) {
      console.error('Failed to center window:', error)
    }
  }
}

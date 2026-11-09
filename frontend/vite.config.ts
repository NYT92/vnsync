import { fileURLToPath, URL } from 'node:url'

import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwind from '@tailwindcss/vite'
import ui from "@nuxt/ui/vite";
import vueDevTools from 'vite-plugin-vue-devtools'

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    vue(),
    tailwind(),
    vueDevTools(),
    ui({
      ui: {
        colors: {
          primary: "indigo",
          neutral: "stone",
        },
        dashboardGroup: {
          base: "fixed top-8 inset-x-0 bottom-0 flex overflow-hidden",
        },
        dashboardSidebar: {
          slots: {
            root: "min-h-0 h-full",
          },
        },
        dashboardPanel: {
          slots: {
            root: "min-h-0 h-full",
          },
        },
        button: {
          defaultVariants: {
            size: "lg",
          },
        },
        badge: {
          defaultVariants: {
            size: "lg",
          },
        },
        input: {
          defaultVariants: {
            size: "lg",
          },
        },
        select: {
          defaultVariants: {
            size: "lg",
          },
        },
        textarea: {
          defaultVariants: {
            size: "lg",
          },
        },
        formField: {
          defaultVariants: {
            size: "lg",
          },
        },
      },
    }),
  ],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  
});

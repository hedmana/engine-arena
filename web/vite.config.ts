import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

const server = '127.0.0.1:7878'

export default defineConfig({
  plugins: [svelte()],
  server: {
    proxy: {
      '/api': `http://${server}`,
      '/ws': { target: `ws://${server}`, ws: true },
    },
  },
})

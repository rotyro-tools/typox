import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [tailwindcss()],
  clearScreen: false,
  build: {
    // Emit every asset as a file: the CSP allows images from 'self' only, not data: URIs.
    assetsInlineLimit: 0,
    rollupOptions: {
      input: {
        widget: new URL('widget.html', import.meta.url).pathname,
        settings: new URL('settings.html', import.meta.url).pathname,
      },
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: 'ws',
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
});

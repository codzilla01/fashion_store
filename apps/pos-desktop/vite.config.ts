import { defineConfig } from 'vite';

export default defineConfig({
  clearScreen: false,
  esbuild: { jsx: 'automatic' },
  server: {
    host: '0.0.0.0',
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
    allowedHosts: true,
  },
  preview: { host: '0.0.0.0', port: 1421, strictPort: true, allowedHosts: true },
});

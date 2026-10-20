import { defineConfig } from 'vite';
export default defineConfig({
  esbuild: { jsx: 'automatic' },
  server: { host: '0.0.0.0', port: 1422, strictPort: true, allowedHosts: true },
  preview: { host: '0.0.0.0', port: 1423, strictPort: true, allowedHosts: true },
});

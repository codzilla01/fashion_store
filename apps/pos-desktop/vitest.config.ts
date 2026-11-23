import { defineConfig } from 'vitest/config';

export default defineConfig({
  esbuild: { jsx: 'automatic' },
  test: {
    // The component tests need a DOM; the arithmetic tests do not care.
    environment: 'jsdom',
    include: ['src/**/*.test.{ts,tsx}'],
    // Enabled so @testing-library can register its automatic DOM cleanup after
    // each test; without it the renders pile up in the same document.
    globals: true,
  },
});

import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// Dev: `pnpm web:dev` serves on :5173 and proxies the API (run it with
// PUBLIC_URL=http://localhost:5173 so sign-in returns here).
// Prod: `vite build` → dist/, served by the API (STATIC_DIR).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      '/api': 'http://localhost:8090',
      '/auth': 'http://localhost:8090',
    },
  },
  build: { outDir: 'dist', sourcemap: false },
  test: { environment: 'node' },
});

/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import path from 'node:path';

export default defineConfig({
  plugins: [react()],
  clearScreen: false, // keep Rust errors visible
  server: { port: 1420, strictPort: true }, // must match build.devUrl in tauri.conf.json
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  resolve: { alias: { '@': path.resolve(import.meta.dirname, 'src') } },
  build: { target: 'es2022', sourcemap: true },
  test: { environment: 'jsdom', globals: true, setupFiles: ['./src/test/setup.ts'] },
});

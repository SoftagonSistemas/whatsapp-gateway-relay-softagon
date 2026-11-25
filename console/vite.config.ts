import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

export default defineConfig({
  base: '/console/',
  plugins: [vue()],
  server: {
    port: 5173,
    proxy: {
      '/v1': 'http://127.0.0.1:8090',
      '/openapi.yaml': 'http://127.0.0.1:8090',
      '/docs': 'http://127.0.0.1:8090',
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    sourcemap: false,
    rollupOptions: {
      output: {
        manualChunks: {
          'vue-vendor': ['vue', 'vue-router', 'vue-i18n'],
        },
      },
    },
  },
});

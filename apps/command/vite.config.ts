import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Dev server proxies the two backend services; in production aips-core serves this build.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api/emergency': 'http://localhost:8091',
      '/api': 'http://localhost:8090',
      '/config.json': 'http://localhost:8090',
    },
  },
  build: { sourcemap: false, chunkSizeWarningLimit: 600 },
});

import { defineConfig } from "vite";

// En développement (`npm run dev`), Vite relaie l'API et le WebSocket vers le serveur Rust local.
export default defineConfig({
  server: {
    proxy: {
      "/ws": { target: "ws://localhost:8080", ws: true },
      "/api": "http://localhost:8080",
      "/health": "http://localhost:8080",
    },
  },
});

import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { resolve } from "node:path";

// Three windows, three HTML entry points: the panel, the reminder card, the quick-add box.
export default defineConfig({
  root: "ui",
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "chrome120",
    rollupOptions: {
      input: {
        main: resolve(import.meta.dirname, "ui/index.html"),
        reminder: resolve(import.meta.dirname, "ui/reminder.html"),
        quickadd: resolve(import.meta.dirname, "ui/quickadd.html"),
      },
    },
  },
});

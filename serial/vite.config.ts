import { defineConfig } from "vitest/config";
import solid from "@solidjs/vite-plugin";

export default defineConfig({
  // Relative, so the built app works both at a domain root and under the
  // /<repo>/ path GitHub Pages serves a project site from
  base: "./",
  plugins: [solid()],
  test: {
    // Everything under src/modbus and src/devices is plain TypeScript over
    // bytes, with no DOM in sight. The components are verified by `tsc` and by
    // running the thing against a real fan
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});

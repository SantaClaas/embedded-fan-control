import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { defineConfig } from "vitest/config";
import solid from "@solidjs/vite-plugin";

/**
 * A git command whose failure is not fatal. There is no git in a build made from a downloaded
 * archive, and the page is still perfectly usable without knowing where it came from
 */
function git(...arguments_: readonly string[]): string | undefined {
  try {
    return execFileSync("git", arguments_, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {
    return undefined;
  }
}

/**
 * The origin remote as a browsable https URL, so a fork links to the fork rather than to a commit
 * that only exists here. Falls back to this repository, which is where the deployed page is built
 * from
 */
function repository(): string {
  const remote = git("remote", "get-url", "origin");
  const match =
    remote === undefined ? null : /^(?:https:\/\/github\.com\/|git@github\.com:)(.+?)(?:\.git)?$/.exec(remote);
  return `https://github.com/${match?.[1] ?? "SantaClaas/embedded-fan-control"}`;
}

const { version } = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")) as {
  version: string;
};

const commit = git("rev-parse", "HEAD");

/**
 * What the page reports about itself. The version is package.json's and the hash is semver build
 * metadata on it, the same shape the firmware's build script stamps into its reported software
 * version.
 *
 * Computed here rather than fetched at runtime: the page is served as static files from Pages,
 * with no server to ask, and a build knows exactly what it was built from
 */
const build = {
  version,
  // The link is to the tree rather than the commit, so it opens the repository as this page was
  // built from it rather than the diff that got there
  commit: commit === undefined ? null : { short: commit.slice(0, 7), url: `${repository()}/tree/${commit}` },
};

export default defineConfig({
  // Relative, so the built app works both at a domain root and under the
  // /<repo>/ path GitHub Pages serves a project site from
  base: "./",
  plugins: [solid()],
  define: { __BUILD__: JSON.stringify(build) },
  test: {
    // Everything under src/modbus and src/devices is plain TypeScript over
    // bytes, with no DOM in sight. The components are verified by `tsc` and by
    // running the thing against a real fan
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});

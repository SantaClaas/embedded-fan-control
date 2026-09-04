/**
 * Stamped into the bundle by `define` in vite.config.ts, which is also where the shape is built.
 * `commit` is null when there was no git to ask
 */
declare const __BUILD__: {
  readonly version: string;
  readonly commit: { readonly short: string; readonly url: string } | null;
};

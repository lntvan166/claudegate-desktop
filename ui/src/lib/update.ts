// The app-update chip: what the header and its popover say about a newer release.
// Mirrors src-tauri/src/update.rs `UpdateView` (serde camelCase).

export type UpdateView = {
  version: string;
  current: string;
  notes: string | null;
  /** The app can replace itself (macOS, AppImage). */
  canInstall: boolean;
  /** For a .deb install: the command that installs the new package. */
  command: string | null;
};

/** Check once shortly after start, then every six hours while the app stays open. */
export const FIRST_CHECK_MS = 5_000;
export const RECHECK_MS = 6 * 60 * 60 * 1000;

export const chipLabel = (u: UpdateView) => `Update ${u.version}`;

export const summary = (u: UpdateView) =>
  u.canInstall
    ? `ClaudeGate ${u.version} is available (you have ${u.current}). Installing restarts the app.`
    : `ClaudeGate ${u.version} is available (you have ${u.current}). This copy came from a .deb package, so run this in a terminal to update it:`;

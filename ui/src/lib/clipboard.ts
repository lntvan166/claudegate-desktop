// Text to the system clipboard. The webview's own async clipboard API, called from a key press or
// a click (the user gesture it requires).

/** Writes `text`; rejects with a readable reason when the webview refuses. */
export async function writeClipboard(text: string): Promise<void> {
  if (!navigator.clipboard?.writeText) throw new Error("this webview has no clipboard access");
  await navigator.clipboard.writeText(text);
}

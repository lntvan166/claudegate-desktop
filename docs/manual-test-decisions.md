# Manual test: decisions with the extension running

You run this on the real machine. It checks that ClaudeGate Desktop and the VS Code extension stay in step while you accept, reject and undo. Use a scratch repo so nothing you care about is touched.

## Setup

1. Make the scratch repo and register it in VS Code:
   ```bash
   mkdir -p ~/tmp/cg-scratch && cd ~/tmp/cg-scratch && git init
   printf 'one\n' > a.txt; printf 'two\n' > b.txt; printf 'three\n' > c.txt
   git add . && git commit -m init
   code ~/tmp/cg-scratch
   ```
   Expected: VS Code opens it and the Claude Gate panels are present.
2. Start the app from the desktop repo:
   ```bash
   cd ~/Documents/src/personal/claudegate-desktop
   export PATH=$HOME/.cargo/bin:$PATH && npm run tauri dev
   ```
   Expected: the window opens. Read-only is off (the switch in settings is not set).
3. Note the backups before you start: `ls ~/.claudegate-desktop/backups/ 2>&1; find ~/.claudegate-desktop/backups -name '*.json' 2>/dev/null | wc -l`.

## Steps

1. In a Claude Code session in `~/tmp/cg-scratch`, ask Claude to edit `a.txt`, `b.txt` and `c.txt` (any change to each).
   Expected: three pending files in both VS Code and the app.
2. In the app, accept `a.txt`.
   Expected: `a.txt` moves to Accepted in the app. VS Code's panel shows it in Accepted within about 1 second. `a.txt` on disk still has Claude's version.
3. In the app, reject `b.txt`.
   Expected: `b.txt` is restored on disk (`cat ~/tmp/cg-scratch/b.txt` prints `two`). It appears in Rejected in the app and in VS Code.
4. In VS Code, accept `c.txt`.
   Expected: the app shows `c.txt` in Accepted within about a second, with no refresh. Pending is now empty.
5. In the app, use Undo on the `b.txt` reject (or Reapply from its Rejected row).
   Expected: `b.txt` holds Claude's version again (`cat` shows the edited text) and is pending in the app and in VS Code.
6. Check backups: `find ~/.claudegate-desktop/backups -name '*.json' | wc -l`.
   Expected: about one backup per decision you made in the app (steps 2, 3 and 5 are three writes, so three new files). Writes made by VS Code in step 4 are not backed up by the app. Each file is under `backups/<hash>/`, named like `20261116T093012.345Z.json`.
7. While Claude edits, decide at the same time: ask Claude to edit `a.txt` again, and as soon as it lands, accept something in the app.
   Expected: the new pending `a.txt` is still there afterwards. Nothing Claude captured is lost.
8. Read-only switch: turn read-only on in the app.
   Expected: accept and reject are disabled or refused with a read-only message. Turn it off again and they work.
9. Edit the config by hand to break it: `echo '{' > ~/.claudegate-desktop/config.json`, then try a decision.
   Expected: decisions are paused with the settings-unreadable message. Restore the file (`{"readOnly": false}`) afterwards.

## Cleanup

- Quit the app (Ctrl+C in the terminal).
- `rm -rf ~/tmp/cg-scratch`, and remove it from the VS Code recent list if you like.
- Leave `~/.claudegate-desktop/backups/` in place; it keeps only the newest 50 per project.

## If something is off

- Panels disagree: check `~/.claudegate/hook.log` and the Claude Gate output channel in VS Code.
- A decision was refused: the message names the reason (read-only, newer session format, unreadable file).
- To restore a session by hand, copy the newest file from `~/.claudegate-desktop/backups/<hash>/` over `~/.claudegate/sessions/<hash>.json` with both apps closed.

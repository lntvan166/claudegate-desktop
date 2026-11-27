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
   cd claudegate-desktop   # your clone of this repo
   export PATH=$HOME/.cargo/bin:$PATH && npm run tauri dev
   ```
   Expected: the window opens. Read-only is off (the switch in settings is not set).
3. Record the backup count before you start: `find ~/.claudegate-desktop/backups -name '*.json' 2>/dev/null | wc -l`. Write the number down as B0.
   Expected: a number (0 if you have never decided in the app, otherwise the files already kept, at most 50 per project). `find` printing nothing else is fine if the folder does not exist yet.

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
   Expected: B0 (Setup 3) plus 3, one new file per decision you made in the app (steps 2, 3 and 5), unless a project already holds 50 (the newest 50 are kept, so the total stops growing). VS Code's write in step 4 is not backed up by the app. New files sit under `backups/<hash>/`, named like `20261116T093012.345Z.json`.
7. Reject all. Make sure at least 3 changes are pending, one of them a file Claude created (ask Claude to edit `a.txt` and `c.txt` again and to create `new.txt`; by now steps 2 to 5 have left nothing else pending).
   - Click Reject all.
     Expected: an inline confirmation reads "Reject N files? Originals are restored and files Claude created are deleted.", where N is the number of real pending changes. Cancel is focused.
   - Press Esc.
     Expected: the confirmation closes and nothing changes on disk or in the panels.
   - Click Reject all again and confirm.
     Expected: edited files are back to their originals, `new.txt` is deleted, and the result bar reads "Rejected N files" (or shows "left on disk" counts if some file had no original).
8. While Claude edits, decide at the same time: ask Claude to edit `a.txt` again, and as soon as it lands, accept something in the app.
   Expected: the new pending `a.txt` is still there afterwards. Nothing Claude captured is lost.
9. Read-only switch: click the read-only toggle in the app (its tooltip is "Turn on read-only mode to block every decision.").
   Expected: the accept, reject and other decision actions are hidden, and a banner reads "Read-only mode is on" with a [Turn off] button. A refused write would say: "ClaudeGate Desktop is in read-only mode. Turn it off to make decisions." Click [Turn off]: the banner goes and the actions return.
10. Break the settings file: `echo '{' > ~/.claudegate-desktop/config.json`. Before any decision the app still looks normal and the actions stay enabled, because it only finds out when a decision is attempted. Now try one, for example accept a pending file.
   Expected: the decision is refused and the banner reads "ClaudeGate Desktop couldn't read its settings file (~/.claudegate-desktop/config.json), so decisions are paused. Fix or delete that file." with a [Try again] button. Restore the file (`echo '{"readOnly": false}' > ~/.claudegate-desktop/config.json`), click [Try again], and decisions return.
11. Optional, only if you have a session file to spare: a project whose session cannot be read shows "This project's review log couldn't be read, so decisions are paused." and a session from a newer ClaudeGate shows "This project was written by a newer ClaudeGate. Update the app to make decisions here.". In both cases decisions stay off for that project.

## Cleanup

- Quit the app (Ctrl+C in the terminal).
- `rm -rf ~/tmp/cg-scratch`, and remove it from the VS Code recent list if you like.
- Leave `~/.claudegate-desktop/backups/` in place; it keeps only the newest 50 per project.

## If something is off

- Panels disagree: check `~/.claudegate/hook.log` and the Claude Gate output channel in VS Code.
- A decision was refused: the message names the reason (read-only, newer session format, unreadable file).
- To restore a session by hand, copy the newest file from `~/.claudegate-desktop/backups/<hash>/` over `~/.claudegate/sessions/<hash>.json` with both apps closed.

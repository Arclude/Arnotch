# linux-ai — arnotch

A Dynamic-Island-style assistant for the Linux desktop. A small island with a robot face
lives at the top center of the screen, and it expands to show what your coding agents
(Claude Code, ...) and scripts are doing.

![states: collapsed, working, waiting, done]

## Architecture

```
crates/
  arnotch-core/   event protocol, Unix socket IPC, Claude Code hook adapter
  arnotch-ui/     the island: Rust (cxx-qt) + QML, placed with wlr-layer-shell, tray icon, settings
  arnotchctl/     CLI that sends events to the island (used by hooks and scripts)
```

Clients write newline-delimited JSON events to `$XDG_RUNTIME_DIR/arnotch.sock`:

```json
{"type":"notify","title":"Backup","body":"Finished in 42s"}
{"type":"agent","session_id":"abc","project":"korus","phase":"tool","tool":"Edit","detail":"invoice.ts"}
```

Agent phases: `started`, `tool` (optionally with a `diff`), `tool_result` (with `ok` and the
command's last `output` lines), `waiting`, `done`, `ended`.

## Requirements

- Rust (stable)
- Qt 6 (`qt6-base`, `qt6-declarative`)
- [LayerShellQt](https://invent.kde.org/plasma/layer-shell-qt) (`layer-shell-qt`) for Wayland

Works on Wayland compositors with `wlr-layer-shell` (KDE Plasma, Hyprland, Sway, niri, COSMIC)
and on X11. GNOME does not support layer-shell yet.

## Build and run

```sh
cargo build --release
./target/release/arnotch &
./target/release/arnotchctl notify "Hello" "from arnotch"
./target/release/arnotchctl open terminals   # or: open activity|chat|shelf, close
./target/release/arnotchctl toggle chat      # open on a tab, or close it (for shortcuts)
./target/release/arnotchctl settings
./target/release/arnotchctl terminals        # list open terminals as JSON
```

## Install

```sh
contrib/install.sh          # build, install to ~/.local/bin and start with every session
contrib/install.sh --hooks  # also add the Claude Code hooks to ~/.claude/settings.json
contrib/install.sh --uninstall
```

arnotch runs as the systemd user service `arnotch.service`, tied to the graphical session and
restarted if it crashes. Its log: `journalctl --user -u arnotch`. Run the script again after
pulling changes to update. Coming from an older version called *notch*, it moves the service,
the Claude Code hooks and the chat shortcut over, and arnotch copies your settings and shelf.

## Terminals

The *Terminals* tab lists every open terminal tab with the command running in it, its folder,
running time and CPU use. You can stop a command (Ctrl+C, press again to terminate) or jump to
its terminal window. Commands that ran for 10 seconds or more announce when they finish.

It reads `/proc`, so it works with any terminal emulator (Konsole, kitty, WezTerm, Alacritty,
foot, GNOME Terminal, ...) and inside tmux. Jumping to a window is supported on KDE Plasma,
Hyprland and Sway.

Typing into a terminal (✎) and selecting the exact tab use the terminal's own remote control,
so they need a supported terminal:

| Terminal | Setup |
|---|---|
| Konsole | Settings › Configure Konsole › General › *Enable the security sensitive parts of the DBus API* |
| tmux | none |
| WezTerm | none (`wezterm cli`) |
| kitty | `allow_remote_control yes` and `listen_on unix:@kitty-{kitty_pid}` in `kitty.conf` (or set `ARNOTCH_KITTY_SOCKET`) |
| Arterm | a version with the control socket (`$XDG_RUNTIME_DIR/arterm/control-<pid>.sock`) |

When Claude waits for you, the activity card offers *Reply to Claude*, which types your message
into the terminal running that Claude session.

## Settings

Open them from the tray icon (or `arnotchctl settings`). Stored in `~/.config/arnotch/arnotch.conf`.

- **Show on**: the active screen (follows focus), all screens, or one specific monitor.
- **Top panel**: on a screen with a top panel, the island can sit *over* the panel (covering
  its center, e.g. the clock) or *below* it.
- **Clock**: show the time inside the island, handy when it replaces the panel's clock.
- **Events**: open the island automatically when something happens.

## Files

Drag files onto the island and drop them on one of its targets:

- **Keep on shelf**: the *Shelf* tab keeps them (by path) across restarts. Drag them back out
  into any app, open them, share them or send them to Claude.
- **Send to Claude**: types `@path` into the prompt of a Claude Code session running in a
  supported terminal, without submitting, and switches to it.
- **Share link**: uploads the file and copies the link, with a progress bar. Off by default:
  pick a service in the settings (0x0.st, catbox.moe, litterbox for 24 hours, or your own
  command, where `{file}` is the quoted path and the last line printed is the link).

`arnotchctl shelf <file>...` and `arnotchctl share <file>` do the same from a terminal.

Below the files, the *Shelf* tab lists the last eight texts you copied (KDE's clipboard history,
Klipper); a click copies one again. Entries that look like a private key or an access token are
hidden on screen but can still be copied.

## Chat

The *Chat* tab talks to Claude without opening a terminal. Each message runs the Claude Code
CLI in print mode (`claude -p`), so it uses your Claude Code login and needs no API key; the
conversation continues until you start a new one (+). It is a plain chat: no tools, no MCP
servers and none of your Claude Code settings or hooks. The answer is streamed as it is
written; ■ stops it. Set `ARNOTCH_CLAUDE` if `claude` is not on the `PATH` or in `~/.local/bin`.

On KDE Plasma, `contrib/install.sh` binds **Meta+Space** to open and close the chat (a custom
shortcut named *arnotch: Chat*, which you can change in System Settings › Shortcuts). Elsewhere,
bind `arnotchctl toggle chat` to a key. An answer that finishes while the chat is closed shows up
as a short note in the island.

## Indicators

Changing the volume or a screen's brightness widens the closed island for a moment with a
level bar, and plugging in the charger shows the battery's charge. A battery that drops below
20%, 10% and 5% opens the island with a warning. Volume comes from `pactl` (PulseAudio or
PipeWire), brightness from KDE's screen brightness service and the battery from UPower; all
three are event driven. Turn them off in the settings (*Indicators*).

## Calendar

Paste your calendar's secret iCalendar address in the settings (*Calendar*): in Google Calendar,
Settings › your calendar › *Secret address in iCal format*; Outlook and iCloud offer one too.
arnotch downloads it every 10 minutes. The *Activity* tab shows the meeting under way or coming
next, with a **Join** button when it has a Meet, Zoom or Teams link, and the island opens ten
minutes before a meeting and when it starts. Recurring meetings, cancelled and moved instances
are handled. The address is a secret: it is never logged.

## Media

The *Activity* tab shows what's playing, with its cover and previous, play/pause and next
buttons; while music plays, the closed island shows the cover. It works with every player that
supports MPRIS (Spotify, Firefox, Chromium, VLC, mpv, ...) and follows the one that started last.

## Performance

The island uses Qt's software renderer by default: for a surface this small it needs about half
the memory of a GPU context (≈35 MB PSS). It uses about 0.2% CPU when idle and 0.5% while an
agent works: animations only draw frames while something moves, and only while they are visible.
They run at the display's refresh rate (144 Hz, 180 Hz, ...) rather than Qt's usual 60 steps a
second; `ARNOTCH_ANIMATION_HZ` caps it.
Set `ARNOTCH_RENDERER=gpu` to use the GPU instead.

## Claude Code integration

Run `contrib/install.sh --hooks`, or put `arnotchctl` on your `PATH` and merge [`contrib/claude-code-hooks.json`](contrib/claude-code-hooks.json)
into `~/.claude/settings.json`. Every Claude Code session will then show up in the island,
with its recent steps, a live diff of each file edit and the output of commands such as test runs.
The hook takes about 1 ms and never fails or blocks, even when arnotch is not running.

When Claude asks for permission, the island shows the command or diff with **Allow**, **Deny** and
**Ask in terminal** buttons. `arnotchctl answer allow|deny|ask` answers the latest request, which is
handy on a global shortcut. Without an answer within 45 seconds (`ARNOTCH_PERMISSION_TIMEOUT`), or
when arnotch is not running, Claude Code simply asks in the terminal as usual.

## License

MIT

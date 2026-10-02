# linux-ai — notch

A Dynamic-Island-style assistant for the Linux desktop. A small island with a robot face
lives at the top center of the screen, and it expands to show what your coding agents
(Claude Code, ...) and scripts are doing.

![states: collapsed, working, waiting, done]

## Architecture

```
crates/
  notch-core/   event protocol, Unix socket IPC, Claude Code hook adapter
  notch-ui/     the island: Rust (cxx-qt) + QML, placed with wlr-layer-shell, tray icon, settings
  notchctl/     CLI that sends events to the island (used by hooks and scripts)
```

Clients write newline-delimited JSON events to `$XDG_RUNTIME_DIR/notch.sock`:

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
./target/release/notch &
./target/release/notchctl notify "Hello" "from the notch"
./target/release/notchctl open terminals   # or: open activity, close
./target/release/notchctl settings
./target/release/notchctl terminals        # list open terminals as JSON
```

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
| kitty | `allow_remote_control yes` and `listen_on unix:@kitty-{kitty_pid}` in `kitty.conf` (or set `NOTCH_KITTY_SOCKET`) |
| Arterm | a version with the control socket (`$XDG_RUNTIME_DIR/arterm/control-<pid>.sock`) |

When Claude waits for you, the activity card offers *Reply to Claude*, which types your message
into the terminal running that Claude session.

## Settings

Open them from the tray icon (or `notchctl settings`). Stored in `~/.config/notch/notch.conf`.

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

`notchctl shelf <file>...` and `notchctl share <file>` do the same from a terminal.

## Performance

The island uses Qt's software renderer by default: for a surface this small it needs about half
the memory of a GPU context (≈22 MB PSS) and stays around 0.1% CPU when idle. Animations only run
while they are visible. Set `NOTCH_RENDERER=gpu` to use the GPU instead.

## Claude Code integration

Put `notchctl` on your `PATH` and merge [`contrib/claude-code-hooks.json`](contrib/claude-code-hooks.json)
into `~/.claude/settings.json`. Every Claude Code session will then show up in the island,
with its recent steps, a live diff of each file edit and the output of commands such as test runs.
The hook takes about 1 ms and never fails or blocks, even when the notch is not running.

When Claude asks for permission, the island shows the command or diff with **Allow**, **Deny** and
**Ask in terminal** buttons. `notchctl answer allow|deny|ask` answers the latest request, which is
handy on a global shortcut. Without an answer within 45 seconds (`NOTCH_PERMISSION_TIMEOUT`), or
when the notch is not running, Claude Code simply asks in the terminal as usual.

## License

MIT

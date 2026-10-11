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
- `curl`; `secret-tool` (`libsecret`) for *Mail*

Works on Wayland compositors with `wlr-layer-shell` (KDE Plasma, Hyprland, Sway, niri, COSMIC)
and on X11. GNOME does not support layer-shell yet.

## Build and run

```sh
cargo build --release
./target/release/arnotch &
./target/release/arnotchctl notify "Hello" "from arnotch"
./target/release/arnotchctl open terminals   # or: open activity|chat|shelf, close
./target/release/arnotchctl toggle chat      # open on a tab, or close it (for shortcuts)
./target/release/arnotchctl timer 25m Focus  # start a timer (or: timer cancel)
./target/release/arnotchctl focus            # focus for 25 minutes, then a break (or: focus 50, focus stop)
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

**Meta+Shift+C** (set by `contrib/install.sh` on KDE, or bind `arnotchctl screenshot`) lets you
select a region of the screen with Spectacle; the island then asks where the picture goes, with
the same targets as a drop: a Claude session, the chat, the shelf or a share link. Pictures are
kept in `~/.cache/arnotch/screenshots` (the newest 20).

Below the files, the *Shelf* tab lists the last eight texts you copied (KDE's clipboard history,
Klipper); a click copies one again. Entries that look like a private key or an access token are
hidden on screen but can still be copied.

## Claude usage

The *Activity* tab shows how much of your Claude plan's 5-hour and 7-day limits is used and when
each resets, and a session's card shows its cost and how full its context is. The island opens
when the 5-hour window passes 80% and 95%. The numbers come from Claude Code's status line input:
`contrib/install.sh` points the status line at `arnotchctl statusline`, which passes them on and
then runs the status line you had before (saved in `~/.config/arnotch/statusline-command`), so it
looks the same. `--uninstall` puts it back. Limits are shown for Pro and Max plans.

## Chat

The *Chat* tab talks to Claude without opening a terminal. Each message runs the Claude Code
CLI in print mode (`claude -p`), so it uses your Claude Code login and needs no API key; the
conversation continues until you start a new one (+). Attach files with 📎 or by dropping them on the island (*Ask in
chat*): images go to Claude as images, PDFs as documents it reads page by page (up to 20 MB),
text files with their contents. In the settings, Claude
can be allowed to search the web (on by default) and to read files on this computer (off); both
are read-only. Claude can also change files (on by default): each change waits for your Allow
in the island, which shows the diff, and once made the chat shows it under the answer (▾ folds
it). What Claude may do without asking follows Claude Code's permission modes, switched with the
button next to the chat's field (or Shift+Tab in it) or in the settings: *Auto* (the default:
safe commands, such as fetching a page for research, run right away, risky ones and file changes
come to the island), *Accept edits* (files change without asking, commands are asked), *Plan*
(Claude only looks into things and answers with a plan; *Carry out the plan* under it goes ahead
in the previous mode) and *Ask first*. None of your Claude Code settings, hooks or MCP servers are used. The answer is streamed as it is
written; ■ stops it. Set `ARNOTCH_CLAUDE` if `claude` is not on the `PATH` or in `~/.local/bin`.

On KDE Plasma, `contrib/install.sh` binds **Meta+Space** to open and close the chat (a custom
shortcut named *arnotch: Chat*, which you can change in System Settings › Shortcuts). Elsewhere,
bind `arnotchctl toggle chat` to a key. An answer that finishes while the chat is closed shows up
as a short note in the island.

### Choosing the model

Each message goes to the model that suits it, with as much thinking (*Model* in the settings:
*Automatic*, or always Haiku, Sonnet or Opus). Commands and small talk ("open Spotify", "turn it
down") go to Haiku; questions, research, summaries and attached images or PDFs to Sonnet; code,
debugging, analysis, long questions and attached source files to Opus, with high effort. A
short follow-up ("go on") stays with the conversation's model, and one saying the last answer
didn't work ("that's wrong", "olmadı") goes a model up. The model is shown under the answer.

An answer that didn't get the job done is tried again, in the same conversation, a model up:
when the last command Claude ran failed, or when the answer ends saying it couldn't (shown as
*Haiku → Sonnet*). Opus gets no second try.

**Claude Code in terminals.** With *Also for Claude Code in terminals* on, the
`UserPromptSubmit` hook (`contrib/install.sh --hooks`) routes the prompts you type to Claude
Code the same way. Hooks can't change a session's model, so a prompt for another model is held
back, arnotch types `/model …` (answering Claude Code's *Switch model?*) and `/effort …` into the
terminal, and then the prompt again. Switching drops the prompt cache, so a session goes up to a
stronger model at once but down only while its context is small (under 60k tokens) or after
three light prompts in a row; prompts of several lines, pasted images and `/` commands go
through as they are. `/model` and `/effort` also save themselves as the default of new
sessions; arnotch puts your own defaults back. It needs a terminal arnotch can type into (see
[Terminals](#terminals)).

### Notes in Obsidian

With an Obsidian vault set in the settings (*Notes*), Claude writes down what it researches on
the web as a short note in the vault's `Araştırmalar` folder (date, topic, answer, sources),
through [MCPVault](https://github.com/bitbonsai/mcpvault) (`npx @bitbonsai/mcpvault`, so it
needs Node.js); Obsidian needs no plugin.

### Asking by voice

🎙 in the chat, or **Meta+Shift+Space** (set by `contrib/install.sh`; `arnotchctl listen` elsewhere),
listens to the microphone; the face's halo follows your voice. A pause of a second and a half,
a click or the shortcut again sends the question, × forgets it. Speech is turned into text on
this computer with [whisper.cpp](https://github.com/ggerganov/whisper.cpp), in whatever
language you speak. It needs `whisper-cli` and a model:

```sh
sudo pacman -S whisper-cpp     # or your distribution's whisper.cpp
arnotchctl voice-model         # small (466 MB); also: large-v3-turbo, medium, base, tiny
```

The best model downloaded is used (`~/.local/share/arnotch/whisper`), or the file
`ARNOTCH_WHISPER_MODEL` names. small mishears Turkish now and then ("kıs" as "kız");
large-v3-turbo doesn't, but takes some 5 seconds a question on a CPU. With a graphics card,
install whisper.cpp's GPU backend (`sudo pacman -S ggml-vulkan`; `ggml-cuda` for NVIDIA's
own) and `arnotchctl voice-model large-v3-turbo-q5_0` (574 MB): it answers in under a second.

### Waking it by name

With *Wake word* on in the settings, saying "Claude" (or the name you set) starts listening
for a question after a tone; "Claude, what's the weather?" asks right away. The microphone
stays on (arnotch shows in the privacy dots); short bursts of speech are turned into text with
the quickest model downloaded (`arnotchctl voice-model base`, 142 MB; `ARNOTCH_WAKE_MODEL`
names another), and long talk is skipped unheard. The name is compared by how it sounds, as
whisper spells a made-up word many ways; a sentence starting with a word that sounds alike
("Kalot…") may wake it too; a name of two or more syllables is heard more reliably than a short one. It doesn't listen while arnotch listens for a question or speaks.

### Answers read aloud

Answers to questions asked by voice are read aloud as Claude writes them, a sentence at a time,
while the face's mouth moves (*Read aloud* in the settings: also every answer, or never). 🔇,
■, a new question or listening again stops it. Code blocks, link addresses and emoji are left
out. Speech is made on this computer with [Piper](https://github.com/rhasspy/piper):

```sh
arnotchctl speech-voice                    # Piper and a voice in your language (tr_TR-dfki-medium, en_US-lessac-medium)
arnotchctl speech-voice en_GB-alba-medium  # any voice of https://huggingface.co/rhasspy/piper-voices
```

A voice in your language is preferred; `ARNOTCH_PIPER_VOICE` names another, `ARNOTCH_PIPER` the
program.

## Indicators

Changing the volume or a screen's brightness widens the closed island for a moment with a
level bar, and plugging in the charger shows the battery's charge. A battery that drops below
20%, 10% and 5% opens the island with a warning. Volume comes from `pactl` (PulseAudio or
PipeWire), brightness from KDE's screen brightness service and the battery from UPower; all
three are event driven. Turn them off in the settings (*Indicators*).

## Timer

The *Activity* tab starts a 5, 15 or 25 minute timer, or a custom one (`10 laundry`,
`1h30m focus`); `arnotchctl timer 25m Focus` and `arnotchctl timer cancel` do it from a terminal.
The closed island counts down in place of the clock (beside the song while music plays), and
when time is up the island opens and plays the sound theme's alarm. A running timer survives a
restart.

## Focus

*Focus* next to the timer buttons (or `arnotchctl focus`, `arnotchctl focus 50`) starts a
Pomodoro: 25 minutes of focus, a 5 minute break, and a 15 minute break after every fourth round,
until you stop it (lengths in the settings). The closed island counts down in red while you
focus and in green during a break; ⏭ moves on to the break, or skips it.

While you focus, notifications that can wait are held back: the desktop's (Do Not Disturb,
ended even if arnotch crashes) and the island's own, such as finished commands, messages, Claude
finishing a turn or a chat answer. The break starts with what came in meanwhile. Calls, meetings,
Claude asking for permission and a low battery still come through. *Hold notifications while
focusing* in the settings turns that off.

## Downloads and progress

Browser downloads (with Plasma Browser Integration), copies and moves in Dolphin and other KDE
apps' transfers take over the closed island while they run: a ring filling up, the file's name
and the percentage (with several, the first one and how many more, and their progress
together). The *Activity* tab lists each with how much is done, the speed and the time left. A
finished download stays a few seconds with *Open* and *Show in folder*. arnotch reads them the
way Plasma shows them in its notifications, by watching the session bus (as `dbus-monitor`
does), so it needs KDE Plasma.

Scripts can show their own progress the same way:

```sh
arnotchctl progress backup 40 "Backing up" "photos/2026"   # id, percent, title, detail
arnotchctl progress backup busy                             # no percentage: a turning ring
arnotchctl progress backup done                             # or: fail
rsync -a --info=progress2 src/ dst/ | arnotchctl progress backup - "Backing up"
```

With `-`, arnotchctl passes its input through and follows the last percentage in it (curl, rsync,
pv, ...), and is done when the input ends. A script that stops reporting is forgotten after 10
minutes.

## Microphone and camera

While an app uses the microphone, a camera or records the screen, an orange, green or purple
dot sits on the face, as on a phone; the *Activity* tab names the apps. PipeWire tells about its
streams (`pw-dump --monitor`), and apps that open a camera themselves (`/dev/video*`) are found
through inotify and `/proc`, so nothing is polled. It can be turned off in the settings
(*Privacy*).

## Weather

Type your city in the settings (*Weather*): "İstanbul", or "Kadıköy, İstanbul" for a district.
The closed island shows the temperature on the right while nothing else is there, the
*Activity* tab the conditions and today's high and low. It comes from
[Open-Meteo](https://open-meteo.com) (free, no account), every 20 minutes; only the city name
and its coordinates are sent.

## Phone

With [KDE Connect](https://kdeconnect.kde.org/) paired, the island announces incoming and missed
calls and new text messages (with a warning when the phone's battery drops below 15%). The
*Phone* tab (`arnotchctl open phone`) shows its battery, a **Ring** button that makes a lost phone
ring even when it is on silent, and the calls and messages since arnotch started. Messages synced
from the phone's history when it connects are not announced. *Phone* in the settings turns it all
off.

## Calendar

Paste your calendar's secret iCalendar address in the settings (*Calendar*): in Google Calendar,
Settings › your calendar › *Secret address in iCal format*; Outlook and iCloud offer one too.
arnotch downloads it every 10 minutes. The *Activity* tab shows the meeting under way or coming
next, with a **Join** button when it has a Meet, Zoom or Teams link, and the island opens ten
minutes before a meeting and when it starts. Recurring meetings, cancelled and moved instances
are handled. The address is a secret: it is never logged.

## Mail

The *Mail* tab (`arnotchctl open mail`) shows a Gmail inbox: the 15 newest messages of the last
30 days, with the sender, subject, the start of the text and the time; unread ones are bold, and
delivery failures (from `mailer-daemon` or `postmaster`) are marked in red as *Returned*.
arnotch checks the inbox every 2 minutes, and new mail opens the island, unless you are focusing:
then it waits for the break, like other notifications.

Set it up in the settings (*Mail*): the Gmail address, and a Google [app
password](https://myaccount.google.com/apppasswords) for it (the account needs 2-Step
Verification; a Workspace admin may have to allow app passwords). The password is kept in the
Secret Service (KWallet, GNOME Keyring) through `secret-tool`, never in the settings file, and goes
to curl on its standard input, so it never shows up in the process list. arnotch only reads over
IMAP with curl; curl's fetch marks a message read, so the new messages that were unread are marked
unread again right after.

Pointing at a message shows what can be done with it: **Open in Gmail**, **Summarize** and
**Draft a reply**. The last two ask Claude in the chat, through the Gmail connector of your
claude.ai account (connect Gmail there, under Settings › Connectors, with the same account). The
reply is shown in the chat and saved as a draft in Gmail, in the message's thread; it is never
sent: Claude can read mail and create or update drafts, and the connector's tools that send, reply,
forward or delete are blocked. For the rest of that conversation the chat keeps Gmail and leaves out
commands and file changes. To reach the connector this chat loads your Claude Code user settings,
with their hooks turned off.

## Media

The *Activity* tab shows what's playing, with its cover and previous, play/pause and next
buttons. It works with every player that
supports MPRIS (Spotify, Firefox, Chromium, VLC, mpv, ...) and follows the one that started last.
While a song plays (and for half a minute after pausing it), the closed island shows its title
and artist with a play/pause button instead of the clock; *Music* in the settings turns that off.

## Performance

The island uses Qt's software renderer by default: for a surface this small it needs about half
the memory of a GPU context (≈35 MB PSS). It uses about 0.2% CPU when idle and 0.5% while an
agent works: animations only draw frames while something moves, and only while they are visible.
They run at the display's refresh rate (144 Hz, 180 Hz, ...) rather than Qt's usual 60 steps a
second, one frame per refresh, paced by the compositor; `ARNOTCH_QT_ANIMATIONS=1` goes back to
Qt's own timer.
Set `ARNOTCH_RENDERER=gpu` to use the GPU instead.

## Claude Code integration

Run `contrib/install.sh --hooks`, or put `arnotchctl` on your `PATH` and merge [`contrib/claude-code-hooks.json`](contrib/claude-code-hooks.json)
into `~/.claude/settings.json`. Every Claude Code session will then show up in the island,
with its recent steps, a live diff of each file edit and the output of commands such as test runs.
The hook takes about 1 ms and never fails or blocks, even when arnotch is not running.

When Claude asks for permission, the island shows the command or diff with **Allow**, **Deny** and
**Ask in terminal** buttons.

Plans and questions are answered there too. At the end of plan mode the island shows the plan
with **Approve, auto-accept edits**, **Approve, I'll review each edit** and **Keep planning…**,
which sends Claude a note on what to change. When Claude asks multiple-choice questions, pick
the options (or type your own answer) and **Send**. These wait 5 minutes in the island
(`ARNOTCH_PROMPT_TIMEOUT`); `contrib/install.sh` gives the permission hook a 600 s timeout so
Claude Code doesn't cut it off sooner. Scripts can answer with
`arnotchctl send '{"type":"answer","decision":"allow","answers":{"Which color?":"Blue"}}'`. `arnotchctl answer allow|deny|ask` answers the latest request, which is
handy on a global shortcut. Without an answer within 45 seconds (`ARNOTCH_PERMISSION_TIMEOUT`), or
when arnotch is not running, Claude Code simply asks in the terminal as usual.

Claude Code shows its own prompt in the terminal at the same time, and doesn't tell the hook
when you answer there. arnotch takes the session carrying on as the answer: the tool's result,
the agent's next tool, a new prompt or the end of the turn closes the request in the island.
(An allowed command that runs a long time keeps it up until it finishes.)

## License

MIT

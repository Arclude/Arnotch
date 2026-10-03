#!/bin/sh
# Installs notch for the current user and starts it with the graphical session.
#
#   contrib/install.sh              build, install to ~/.local/bin, enable the user service
#   contrib/install.sh --hooks      also add the Claude Code hooks to ~/.claude/settings.json
#   contrib/install.sh --uninstall  stop and remove the service and the binaries
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
bin="$HOME/.local/bin"
units="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
claude_settings="$HOME/.claude/settings.json"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
# KDE's global shortcut service, and the shortcut that opens the chat (Meta+Space).
kglobalaccel() { method=$1; shift; dbus-send --session --print-reply --dest=org.kde.kglobalaccel /kglobalaccel "org.kde.KGlobalAccel.$method" "$@"; }
shortcut_id='array:string:notch-chat.desktop,_launch,Notch: Chat,Notch: Chat'
meta_space=268435488

hooks=false
case "${1:-}" in
    --hooks) hooks=true ;;
    --uninstall)
        systemctl --user disable --now notch.service 2>/dev/null || true
        rm -f "$units/notch.service" "$bin/notch" "$bin/notchctl" "$apps/notch-chat.desktop"
        kglobalaccel unregister string:notch-chat.desktop string:_launch >/dev/null 2>&1 || true
        systemctl --user daemon-reload
        echo "notch removed. The Claude Code hooks in $claude_settings are left as they are."
        exit 0
        ;;
    "") ;;
    *) echo "usage: $0 [--hooks | --uninstall]" >&2; exit 2 ;;
esac

cargo=$(command -v cargo || echo "$HOME/.cargo/bin/cargo")
(cd "$root" && "$cargo" build --release)

install -Dm755 "$root/target/release/notch" "$bin/notch"
install -Dm755 "$root/target/release/notchctl" "$bin/notchctl"
install -Dm644 "$root/contrib/notch.service" "$units/notch.service"
echo "Installed notch and notchctl to $bin"

# A notch started by hand holds the socket and would keep the service from starting.
if ! systemctl --user is-active --quiet notch.service; then
    pkill -x notch 2>/dev/null && sleep 1 || true
fi
systemctl --user daemon-reload
systemctl --user enable notch.service
systemctl --user restart notch.service
echo "notch runs now and starts with every session (journalctl --user -u notch for its log)"

# On KDE Plasma, Meta+Space opens the chat. It is a custom shortcut, so it can be changed in
# System Settings › Shortcuts; a shortcut that is already set is left alone.
if kglobalaccel isGlobalShortcutAvailable int32:0 string:x >/dev/null 2>&1; then
    mkdir -p "$apps"
    cat >"$apps/notch-chat.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Notch: Chat
Name[tr]=Notch: Sohbet
Exec=$bin/notchctl toggle chat
Icon=dialog-messages
NoDisplay=true
StartupNotify=false
X-KDE-GlobalAccel-CommandShortcut=true
DESKTOP
    kbuildsycoca6 >/dev/null 2>&1 || true
    kglobalaccel doRegister "$shortcut_id" >/dev/null
    if kglobalaccel shortcut "$shortcut_id" | grep -q int32; then
        echo "Chat shortcut: kept as set in System Settings"
    elif kglobalaccel isGlobalShortcutAvailable int32:$meta_space string:notch-chat.desktop | grep -q true; then
        kglobalaccel setShortcut "$shortcut_id" array:int32:$meta_space uint32:6 >/dev/null
        echo "Chat shortcut: Meta+Space"
    else
        echo "Chat shortcut: Meta+Space is taken; pick one in System Settings › Shortcuts (Notch: Chat)"
    fi
fi

if $hooks; then
    # Merges contrib/claude-code-hooks.json, with notchctl's full path since hooks
    # don't always see ~/.local/bin on PATH. Hooks that are already there are kept.
    python3 - "$root/contrib/claude-code-hooks.json" "$claude_settings" "$bin/notchctl" <<'EOF'
import json, os, shutil, sys, time

hooks_file, settings_file, notchctl = sys.argv[1:]
with open(hooks_file) as f:
    wanted = json.load(f)["hooks"]
settings = {}
backup = None
if os.path.exists(settings_file):
    with open(settings_file) as f:
        settings = json.load(f)
    # Timestamped, so an earlier backup is never overwritten.
    backup = f"{settings_file}.bak.{time.strftime('%Y%m%d-%H%M%S')}"
    shutil.copy2(settings_file, backup)

added = 0
current = settings.setdefault("hooks", {})
for event, groups in wanted.items():
    existing = current.setdefault(event, [])
    commands = {h.get("command", "") for g in existing for h in g.get("hooks", [])}
    if any("notchctl" in c and "claude-hook" in c for c in commands):
        continue
    for group in groups:
        for hook in group["hooks"]:
            hook["command"] = hook["command"].replace("notchctl", f"'{notchctl}'", 1)
        existing.append(group)
    added += 1

os.makedirs(os.path.dirname(settings_file), exist_ok=True)
with open(settings_file, "w") as f:
    json.dump(settings, f, indent=2)
    f.write("\n")
print(f"Claude Code hooks: added {added} event(s) to {settings_file}"
      + (f" (backup: {backup})" if backup else ""))
EOF
fi

#!/bin/sh
# Installs arnotch for the current user and starts it with the graphical session.
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
shortcut_id='array:string:arnotch-chat.desktop,_launch,arnotch: Chat,arnotch: Chat'
meta_space=268435488

# Claude Code's status line goes through arnotchctl, which passes the session's cost and the
# plan's rate limits to arnotch and then runs the status line the user had (saved aside).
statusline() {
    [ -f "$claude_settings" ] || return 0
    python3 - "$1" "$claude_settings" "$bin/arnotchctl" "${XDG_CONFIG_HOME:-$HOME/.config}/arnotch/statusline-command" <<'EOF'
import json, os, shutil, sys, time

action, settings_file, arnotchctl, saved = sys.argv[1:]
with open(settings_file) as f:
    settings = json.load(f)
line = settings.get("statusLine") or {}
ours = "arnotchctl" in line.get("command", "") and "statusline" in line.get("command", "")
if action == "install" and not ours:
    os.makedirs(os.path.dirname(saved), exist_ok=True)
    with open(saved, "w") as f:
        f.write(line.get("command", "") if line.get("type", "command") == "command" else "")
    settings["statusLine"] = {**line, "type": "command", "command": f"'{arnotchctl}' statusline"}
    print("Claude Code status line: now passes usage and limits to arnotch (your previous one still shows)")
elif action == "restore" and ours:
    previous = open(saved).read().strip() if os.path.exists(saved) else ""
    if previous:
        settings["statusLine"] = {**line, "command": previous}
    else:
        settings.pop("statusLine", None)
    print("Claude Code status line: restored")
else:
    sys.exit(0)
shutil.copy2(settings_file, f"{settings_file}.bak.{time.strftime('%Y%m%d-%H%M%S')}")
with open(settings_file, "w") as f:
    json.dump(settings, f, indent=2)
    f.write("\n")
EOF
}

hooks=false
case "${1:-}" in
    --hooks) hooks=true ;;
    --uninstall)
        systemctl --user disable --now arnotch.service 2>/dev/null || true
        rm -f "$units/arnotch.service" "$bin/arnotch" "$bin/arnotchctl" "$apps/arnotch-chat.desktop"
        kglobalaccel unregister string:arnotch-chat.desktop string:_launch >/dev/null 2>&1 || true
        statusline restore
        systemctl --user daemon-reload
        echo "arnotch removed. The Claude Code hooks in $claude_settings are left as they are."
        exit 0
        ;;
    "") ;;
    *) echo "usage: $0 [--hooks | --uninstall]" >&2; exit 2 ;;
esac

cargo=$(command -v cargo || echo "$HOME/.cargo/bin/cargo")
(cd "$root" && "$cargo" build --release)

install -Dm755 "$root/target/release/arnotch" "$bin/arnotch"
install -Dm755 "$root/target/release/arnotchctl" "$bin/arnotchctl"
install -Dm644 "$root/contrib/arnotch.service" "$units/arnotch.service"
echo "Installed arnotch and arnotchctl to $bin"

# Earlier versions were called "notch". Point their Claude Code hooks at arnotchctl first,
# so sessions running meanwhile never call a missing command, then remove the old files.
# (arnotch copies the old settings and shelf over on its first start.)
if [ -f "$claude_settings" ] && grep -q '[^r]notchctl.* claude-hook' "$claude_settings"; then
    python3 - "$claude_settings" "$bin/arnotchctl" <<'EOF'
import json, re, shutil, sys, time

settings_file, arnotchctl = sys.argv[1:]
with open(settings_file) as f:
    settings = json.load(f)
shutil.copy2(settings_file, f"{settings_file}.bak.{time.strftime('%Y%m%d-%H%M%S')}")
for groups in settings.get("hooks", {}).values():
    for group in groups:
        for hook in group.get("hooks", []):
            command = hook.get("command", "")
            if re.search(r"(^|[^r])notchctl'? claude-hook", command):
                hook["command"] = f"'{arnotchctl}' claude-hook"
with open(settings_file, "w") as f:
    json.dump(settings, f, indent=2)
    f.write("\n")
print(f"Claude Code hooks now call arnotchctl ({settings_file})")
EOF
fi
# Plan approvals and questions wait up to 5 minutes in the island: older installs gave
# the permission hook 60 seconds, after which Claude Code cuts it off.
if [ -f "$claude_settings" ] && grep -q 'arnotchctl' "$claude_settings"; then
    python3 - "$claude_settings" <<'EOF'
import json, shutil, sys, time

settings_file = sys.argv[1]
with open(settings_file) as f:
    settings = json.load(f)
hooks = [
    hook
    for group in settings.get("hooks", {}).get("PermissionRequest", [])
    for hook in group.get("hooks", [])
    if "arnotchctl" in hook.get("command", "") and hook.get("timeout", 600) < 600
]
if hooks:
    shutil.copy2(settings_file, f"{settings_file}.bak.{time.strftime('%Y%m%d-%H%M%S')}")
    for hook in hooks:
        hook["timeout"] = 600
    with open(settings_file, "w") as f:
        json.dump(settings, f, indent=2)
        f.write("\n")
    print("Claude Code permission hook: timeout raised to 600 s for plans and questions")
EOF
fi
statusline install

if [ -e "$units/notch.service" ] || [ -e "$bin/notch" ]; then
    systemctl --user disable --now notch.service 2>/dev/null || true
    rm -f "$units/notch.service" "$bin/notch" "$bin/notchctl"
    echo "Removed the old notch service and binaries"
fi

# An arnotch started by hand holds the socket and would keep the service from starting.
if ! systemctl --user is-active --quiet arnotch.service; then
    pkill -x arnotch 2>/dev/null && sleep 1 || true
fi
systemctl --user daemon-reload
systemctl --user enable arnotch.service
systemctl --user restart arnotch.service
echo "arnotch runs now and starts with every session (journalctl --user -u arnotch for its log)"

# On KDE Plasma, Meta+Space opens the chat. It is a custom shortcut, so it can be changed in
# System Settings › Shortcuts; a shortcut that is already set is left alone.
if kglobalaccel isGlobalShortcutAvailable int32:0 string:x >/dev/null 2>&1; then
    mkdir -p "$apps"
    cat >"$apps/arnotch-chat.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=arnotch: Chat
Name[tr]=arnotch: Sohbet
Exec=$bin/arnotchctl toggle chat
Icon=dialog-messages
NoDisplay=true
StartupNotify=false
X-KDE-GlobalAccel-CommandShortcut=true
DESKTOP
    # The shortcut of the old "notch" chat moves over, keys included.
    old_id='array:string:notch-chat.desktop,_launch,Notch: Chat,Notch: Chat'
    old_keys=""
    if [ -e "$apps/notch-chat.desktop" ]; then
        old_keys=$(kglobalaccel shortcut "$old_id" | awk '/int32/ {print $2}' | paste -sd, -)
        kglobalaccel unregister string:notch-chat.desktop string:_launch >/dev/null 2>&1 || true
        # Unregistering leaves the entry in the config file.
        kwriteconfig6 --file kglobalshortcutsrc --group services --group notch-chat.desktop \
            --key _launch --delete 2>/dev/null || true
        rm -f "$apps/notch-chat.desktop"
    fi
    kbuildsycoca6 >/dev/null 2>&1 || true
    kglobalaccel doRegister "$shortcut_id" >/dev/null
    if [ -n "$old_keys" ] && ! kglobalaccel shortcut "$shortcut_id" | grep -q int32; then
        kglobalaccel setShortcut "$shortcut_id" "array:int32:$old_keys" uint32:6 >/dev/null
        echo "Chat shortcut: kept from notch"
    elif kglobalaccel shortcut "$shortcut_id" | grep -q int32; then
        echo "Chat shortcut: kept as set in System Settings"
    elif kglobalaccel isGlobalShortcutAvailable int32:$meta_space string:arnotch-chat.desktop | grep -q true; then
        kglobalaccel setShortcut "$shortcut_id" array:int32:$meta_space uint32:6 >/dev/null
        echo "Chat shortcut: Meta+Space"
    else
        echo "Chat shortcut: Meta+Space is taken; pick one in System Settings › Shortcuts (arnotch: Chat)"
    fi
fi

if $hooks; then
    # Merges contrib/claude-code-hooks.json, with arnotchctl's full path since hooks
    # don't always see ~/.local/bin on PATH. Hooks that are already there are kept.
    python3 - "$root/contrib/claude-code-hooks.json" "$claude_settings" "$bin/arnotchctl" <<'EOF'
import json, os, shutil, sys, time

hooks_file, settings_file, arnotchctl = sys.argv[1:]
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
    if any("arnotchctl" in c and "claude-hook" in c for c in commands):
        continue
    for group in groups:
        for hook in group["hooks"]:
            hook["command"] = hook["command"].replace("arnotchctl", f"'{arnotchctl}'", 1)
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

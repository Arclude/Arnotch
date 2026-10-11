// arnotch for OpenCode: shows the session in the notch and asks there before commands and
// file changes. Written to ~/.config/opencode/plugins/ by `arnotchctl connect opencode`.
// Each hook goes to arnotchctl in Claude Code's hook format.
import { spawn } from "node:child_process"

const ARNOTCHCTL = "@ARNOTCHCTL@"

// Runs `arnotchctl agent-hook opencode` with `payload` on stdin; resolves to its output.
// A missing or broken notch never holds OpenCode up: errors resolve to "".
function hook(payload) {
  return new Promise(resolve => {
    try {
      const child = spawn(ARNOTCHCTL, ["agent-hook", "opencode"], { stdio: ["pipe", "pipe", "ignore"] })
      let out = ""
      child.stdout.on("data", chunk => (out += chunk))
      child.on("error", () => resolve(""))
      child.on("close", () => resolve(out.trim()))
      child.stdin.end(JSON.stringify(payload))
    } catch {
      resolve("")
    }
  })
}

export const Arnotch = async ({ directory }) => {
  const base = sessionID => ({ session_id: sessionID || "opencode", cwd: directory })
  return {
    "chat.message": async (input, output) => {
      const text = (output?.parts ?? []).filter(p => p.type === "text").map(p => p.text).join(" ")
      hook({ ...base(input?.sessionID), hook_event_name: "UserPromptSubmit", prompt: text })
    },
    "tool.execute.before": async (input, output) => {
      hook({ ...base(input?.sessionID), hook_event_name: "PreToolUse", tool_name: input?.tool,
             tool_input: output?.args ?? {}, tool_use_id: input?.callID ?? "" })
    },
    "tool.execute.after": async (input, output) => {
      hook({ ...base(input?.sessionID), hook_event_name: "PostToolUse", tool_name: input?.tool,
             tool_input: {}, tool_response: { stdout: String(output?.output ?? "") },
             tool_use_id: input?.callID ?? "" })
    },
    "permission.ask": async (input, output) => {
      const answer = await hook({
        ...base(input?.sessionID),
        hook_event_name: "PermissionRequest",
        tool_name: input?.type ?? input?.tool ?? "",
        tool_input: { command: input?.pattern ?? input?.metadata?.command ?? input?.title ?? "",
                      file_path: input?.metadata?.filePath ?? input?.path ?? "" },
        tool_use_id: input?.callID ?? input?.id ?? "",
      })
      try {
        const behavior = JSON.parse(answer).hookSpecificOutput.decision.behavior
        if (behavior === "allow" || behavior === "deny") output.status = behavior
      } catch {
        // No answer in the island: OpenCode asks as usual.
      }
    },
    event: async ({ event }) => {
      if (event?.type === "session.idle")
        hook({ ...base(event.properties?.sessionID), hook_event_name: "Stop" })
      else if (event?.type === "session.deleted")
        hook({ ...base(event.properties?.info?.id), hook_event_name: "SessionEnd" })
    },
  }
}

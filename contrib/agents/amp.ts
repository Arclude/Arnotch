// arnotch for Amp: shows the thread in the notch and asks there before commands and file
// changes. Written to ~/.config/amp/plugins/ by `arnotchctl connect amp`. Each event goes
// to arnotchctl in Claude Code's hook format.
import { spawn } from "node:child_process"

const ARNOTCHCTL = "@ARNOTCHCTL@"
const ASKS = /^(bash|shell|edit_file|create_file|write|edit|apply_patch|undo_edit)$/i

// Runs `arnotchctl agent-hook amp` with `payload` on stdin; resolves to its output. A
// missing or broken notch never holds Amp up: errors resolve to "".
function hook(payload: Record<string, unknown>): Promise<string> {
  return new Promise(resolve => {
    try {
      const child = spawn(ARNOTCHCTL, ["agent-hook", "amp"], { stdio: ["pipe", "pipe", "ignore"] })
      let out = ""
      child.stdout.on("data", (chunk: Buffer) => (out += chunk))
      child.on("error", () => resolve(""))
      child.on("close", () => resolve(out.trim()))
      child.stdin.end(JSON.stringify(payload))
    } catch {
      resolve("")
    }
  })
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export default function (amp: any) {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const base = (e: any) => ({ session_id: e?.thread?.id ?? e?.threadID ?? "amp", cwd: process.cwd() })

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  amp.on("agent.start", async (e: any) => {
    hook({ ...base(e), hook_event_name: "UserPromptSubmit", prompt: String(e?.message ?? e?.prompt ?? "") })
  })
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  amp.on("tool.call", async (e: any) => {
    const tool = String(e?.tool ?? e?.name ?? "")
    const payload = { ...base(e), tool_name: tool, tool_input: e?.input ?? e?.args ?? {},
                      tool_use_id: String(e?.id ?? e?.toolUseID ?? "") }
    if (!ASKS.test(tool)) {
      hook({ ...payload, hook_event_name: "PreToolUse" })
      return { action: "allow" }
    }
    const answer = await hook({ ...payload, hook_event_name: "PermissionRequest" })
    try {
      const decision = JSON.parse(answer).hookSpecificOutput.decision
      if (decision.behavior === "deny")
        return { action: "reject-and-continue", message: decision.message ?? "Denied from arnotch" }
    } catch {
      // No answer in the island: carry on as Amp would.
    }
    return { action: "allow" }
  })
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  amp.on("tool.result", async (e: any) => {
    hook({ ...base(e), hook_event_name: "PostToolUse", tool_name: String(e?.tool ?? e?.name ?? ""),
           tool_input: {}, tool_response: { stdout: String(e?.output ?? e?.result ?? "") } })
  })
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  amp.on("agent.end", async (e: any) => {
    hook({ ...base(e), hook_event_name: "Stop" })
  })
}

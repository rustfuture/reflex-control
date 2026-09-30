// reflex-control managed hook. Installed by `reflex install`, removed by `reflex uninstall`.
// Sends tool calls and finished turns to `reflex hook __AGENT__ <event>` and applies the verdict.
// Fails open: if reflex is missing, slow or answers with anything unexpected, everything is allowed.
import { execFile } from "node:child_process"

const AGENT = "__AGENT__"
const allow = { action: "allow" }
const confirmed = new Set() // calls that were refused once with "ask the user first"
const failed = new Set() // sessions whose last run ended in an error or was stopped
const checking = new Set() // sessions with a turn-end check in flight

// Runs `reflex hook AGENT event` with `request` on stdin. Never rejects; resolves to a verdict.
function verdict(event, request, timeoutMs) {
  return new Promise((resolve) => {
    try {
      const child = execFile("reflex", ["hook", AGENT, event], { timeout: timeoutMs, windowsHide: true }, (_error, stdout) => {
        try {
          const v = JSON.parse(stdout)
          resolve(v && typeof v.action === "string" ? v : allow)
        } catch {
          resolve(allow)
        }
      })
      child.stdin.on("error", () => {})
      child.stdin.end(JSON.stringify(request))
    } catch {
      resolve(allow)
    }
  })
}

export default {
  id: "reflex-control",
  server: async ({ client, directory }) => ({
    // Throwing is how a plugin refuses a tool call; the message goes to the model.
    "tool.execute.before": async (input, output) => {
      const v = await verdict("pre-tool", { cwd: directory, session_id: input.sessionID, tool: input.tool, args: output.args }, 10000)
      if (v.action === "block") throw new Error(v.reason || "Blocked by Reflex Control.")
      if (v.action === "ask") {
        // No confirmation dialog is available here: refuse once, then let the same call through.
        const key = JSON.stringify([input.sessionID, input.tool, output.args])
        if (!confirmed.delete(key)) {
          confirmed.add(key)
          throw new Error(`${v.reason} Ask the user first; if they agree, repeat the same call.`)
        }
      }
    },
    event: async ({ event }) => {
      try {
        const id = event.properties?.sessionID
        if (!id) return
        if (event.type === "session.error") failed.add(id)
        else if (event.type === "session.status" && event.properties.status?.type === "busy") failed.delete(id)
        if (event.type !== "session.idle" || failed.delete(id) || checking.has(id)) return
        checking.add(id)
        try {
          const session = await client.session.get({ path: { id } })
          if (session.data?.parentID) return // a subagent; the parent session is checked instead
          const v = await verdict("turn-end", { cwd: directory, session_id: id }, 600000)
          if (v.action === "retry") {
            await client.session.promptAsync({ path: { id }, body: { parts: [{ type: "text", text: v.reason }] } })
          } else if (v.action === "notify") {
            await client.app.log({ body: { service: "reflex-control", level: "warn", message: v.reason } })
            await client.tui.showToast({ body: { message: v.reason, variant: "warning" } })
          }
        } finally {
          checking.delete(id)
        }
      } catch {}
    },
  }),
}

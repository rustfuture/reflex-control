// reflex-control managed hook. Installed by `reflex install`, removed by `reflex uninstall`.
// Sends tool calls and finished runs to `reflex hook pi <event>` and applies the verdict.
// Fails open: if reflex is missing, slow or answers with anything unexpected, everything is allowed.
// Every handler catches its own errors, because pi blocks a tool whose tool_call handler throws.
import { execFile } from "node:child_process";

const allow = { action: "allow" };

// Runs `reflex hook pi event` with `request` on stdin. Never rejects; resolves to a verdict.
function verdict(event, request, timeoutMs) {
  return new Promise((resolve) => {
    try {
      const child = execFile("reflex", ["hook", "pi", event], { timeout: timeoutMs, windowsHide: true }, (_error, stdout) => {
        try {
          const v = JSON.parse(stdout);
          resolve(v && typeof v.action === "string" ? v : allow);
        } catch {
          resolve(allow);
        }
      });
      child.stdin.on("error", () => {});
      child.stdin.end(JSON.stringify(request));
    } catch {
      resolve(allow);
    }
  });
}

export default function (pi) {
  pi.on("tool_call", async (event, ctx) => {
    try {
      const v = await verdict("pre-tool", { cwd: ctx.cwd, session_id: ctx.sessionManager.getSessionId(), tool: event.toolName, args: event.input }, 10000);
      if (v.action === "block") return { block: true, reason: v.reason };
      if (v.action === "ask") {
        if (ctx.hasUI && (await ctx.ui.confirm("Reflex Control", v.reason))) return undefined;
        return { block: true, reason: v.reason };
      }
    } catch {}
    return undefined;
  });

  pi.on("agent_before_settle", async (event, ctx) => {
    try {
      if (event.outcome !== "completed") return undefined;
      const v = await verdict("turn-end", { cwd: ctx.cwd, session_id: ctx.sessionManager.getSessionId() }, 600000);
      if (v.action === "retry") {
        // One more model request, with the failure as a message the model sees. Reflex counts the retries.
        const note = { type: "custom_message", customType: "reflex-control", content: v.reason, display: true };
        return { entries: [...event.entries, note], continue: true };
      }
      if (v.action === "notify" && ctx.hasUI) ctx.ui.notify(v.reason, "warning");
    } catch {}
    return undefined;
  });
}

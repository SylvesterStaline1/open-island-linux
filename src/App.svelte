<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow, availableMonitors } from "@tauri-apps/api/window";
  import { LogicalPosition } from "@tauri-apps/api/dpi";

  type SessionPhase = "Idle" | "Working" | "AwaitingPermission" | "AwaitingQuestion" | "Completed";

  interface Session {
    session_id: string;
    title: string | null;
    cwd: string | null;
    phase: SessionPhase;
    summary: string | null;
    pending_permission: { tool_name: string; tool_input: unknown } | null;
    started_at: number;
    updated_at: number;
  }

  interface PermissionEvent {
    sessionId: string;
    toolName: string;
    toolInput: unknown;
  }

  const PILL_H = 52;
  const SESSION_H = 60;
  const PERMISSION_H = 100;
  const MAX_H = 380;

  let sessions: Session[] = $state([]);
  let permissionEvent: PermissionEvent | null = $state(null);
  let expanded = $state(false);
  let unlisteners: UnlistenFn[] = [];

  const activeSessions = $derived(sessions.filter((s) => s.phase !== "Completed"));
  const hasPermission = $derived(permissionEvent !== null);

  $effect(() => {
    if (hasPermission) expanded = true;
  });

  $effect(() => {
    const sessH = expanded ? Math.min(activeSessions.length, 4) * SESSION_H : 0;
    const permH = hasPermission ? PERMISSION_H : 0;
    const total = Math.min(PILL_H + sessH + permH, MAX_H);
    invoke("set_window_height", { height: total }).catch(() => {});
  });

  onMount(async () => {
    try {
      const win = getCurrentWindow();
      let monitor = await win.currentMonitor();
      if (!monitor) {
        const all = await availableMonitors();
        monitor = all[0] ?? null;
      }
      if (monitor) {
        const scale = monitor.scaleFactor;
        const screenW = monitor.size.width / scale;
        const x = Math.max(0, Math.round((screenW - 460) / 2));
        await win.setPosition(new LogicalPosition(x, 38));
      }
    } catch (e) {
      console.error("Position error:", e);
    }

    sessions = await invoke<Session[]>("get_sessions").catch(() => []);

    unlisteners.push(
      await listen<Session[]>("sessions-changed", (e) => {
        sessions = e.payload;
      }),
      await listen<PermissionEvent>("permission-requested", (e) => {
        permissionEvent = e.payload;
      }),
      await listen<string>("permission-resolved", () => {
        permissionEvent = null;
      }),
    );
  });

  onDestroy(() => unlisteners.forEach((u) => u()));

  async function handlePermission(allow: boolean) {
    if (!permissionEvent) return;
    const id = permissionEvent.sessionId;
    permissionEvent = null;
    await invoke("resolve_permission", { sessionId: id, allow }).catch(console.error);
  }

  function shortCwd(cwd: string | null): string {
    if (!cwd) return "~";
    const parts = cwd.replace(/^\/home\/[^/]+/, "~").split("/");
    return parts.slice(-2).join("/") || "~";
  }

  function phaseColor(phase: SessionPhase): string {
    switch (phase) {
      case "Working": return "#7c6af7";
      case "AwaitingPermission": return "#f5a623";
      case "Completed": return "#4caf50";
      default: return "#44445a";
    }
  }

  function formatInput(input: unknown): string {
    if (!input) return "";
    try {
      const s = JSON.stringify(input);
      return s.length > 120 ? s.slice(0, 120) + "…" : s;
    } catch { return ""; }
  }
</script>

<div class="root">
  <!-- Pill bar -->
  <div
    class="pill"
    role="button"
    tabindex="0"
    onclick={() => { if (!hasPermission) expanded = !expanded; }}
    onkeydown={(e) => e.key === "Enter" && !hasPermission && (expanded = !expanded)}
  >
    <div class="pill-left">
      <span class="logo-dot" style="background:{activeSessions.length ? '#7c6af7' : '#333'}"></span>
      <span class="brand">Open Island</span>
    </div>

    <div class="pill-center">
      {#if activeSessions.length === 0}
        <span class="idle">no agents</span>
      {:else}
        {#each activeSessions.slice(0, 2) as s (s.session_id)}
          <span class="session-chip">
            <span class="chip-dot" style="background:{phaseColor(s.phase)};box-shadow:0 0 5px {phaseColor(s.phase)}"></span>
            <span class="chip-cwd">{shortCwd(s.cwd)}</span>
          </span>
        {/each}
        {#if activeSessions.length > 2}
          <span class="more">+{activeSessions.length - 2}</span>
        {/if}
      {/if}
    </div>

    <div class="pill-right">
      {#if hasPermission}
        <span class="badge-perm">needs approval</span>
      {:else if activeSessions.length > 0}
        <span class="chevron" class:open={expanded}>{expanded ? "▲" : "▼"}</span>
      {/if}
    </div>
  </div>

  <!-- Permission dialog -->
  {#if hasPermission && permissionEvent}
    <div class="perm-panel">
      <div class="perm-row">
        <span class="perm-icon">🔐</span>
        <div class="perm-info">
          <span class="perm-tool">{permissionEvent.toolName}</span>
          {#if permissionEvent.toolInput}
            <span class="perm-input">{formatInput(permissionEvent.toolInput)}</span>
          {/if}
        </div>
        <div class="perm-btns">
          <button class="btn-deny" onclick={() => handlePermission(false)}>Deny</button>
          <button class="btn-allow" onclick={() => handlePermission(true)}>Allow</button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Session list (expanded) -->
  {#if expanded && !hasPermission && activeSessions.length > 0}
    <div class="session-list">
      {#each activeSessions as s (s.session_id)}
        <div class="session-row">
          <span class="s-dot" style="background:{phaseColor(s.phase)};box-shadow:0 0 4px {phaseColor(s.phase)}"></span>
          <div class="s-info">
            <span class="s-cwd">{shortCwd(s.cwd)}</span>
            {#if s.summary}
              <span class="s-summary">{s.summary}</span>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  :global(*) { box-sizing: border-box; margin: 0; padding: 0; }

  :global(html, body) {
    background: transparent !important;
    overflow: hidden;
    user-select: none;
    -webkit-user-select: none;
  }

  .root {
    width: 460px;
    font-family: -apple-system, "Inter", "Segoe UI", sans-serif;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .pill {
    height: 52px;
    background: rgba(15, 15, 22, 0.92);
    backdrop-filter: blur(20px);
    border: 1px solid rgba(255,255,255,0.08);
    border-radius: 26px;
    display: flex;
    align-items: center;
    padding: 0 16px;
    gap: 12px;
    cursor: pointer;
    transition: background 0.15s;
    -webkit-app-region: drag;
  }

  .pill:hover { background: rgba(22, 22, 32, 0.95); }

  .pill-left, .pill-right, .pill-center {
    -webkit-app-region: no-drag;
  }

  .pill-left {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .logo-dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    transition: background 0.3s;
  }

  .brand {
    font-size: 13px;
    font-weight: 600;
    color: #c8c8d8;
    letter-spacing: 0.01em;
  }

  .pill-center {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    overflow: hidden;
  }

  .idle {
    font-size: 12px;
    color: #3a3a50;
  }

  .session-chip {
    display: flex;
    align-items: center;
    gap: 5px;
    background: rgba(255,255,255,0.05);
    border-radius: 10px;
    padding: 2px 8px;
    min-width: 0;
  }

  .chip-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    animation: pulse 1.8s ease-in-out infinite;
  }

  .chip-cwd {
    font-size: 12px;
    color: #9090b0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 140px;
  }

  .more {
    font-size: 11px;
    color: #44445a;
  }

  .pill-right {
    flex-shrink: 0;
    display: flex;
    align-items: center;
  }

  .badge-perm {
    font-size: 11px;
    color: #f5a623;
    background: rgba(245,166,35,0.12);
    border-radius: 8px;
    padding: 2px 8px;
    animation: pulse 1.2s ease-in-out infinite;
  }

  .chevron {
    font-size: 10px;
    color: #44445a;
    transition: transform 0.2s;
  }

  .perm-panel {
    background: rgba(20, 12, 35, 0.95);
    border: 1px solid rgba(124,106,247,0.3);
    border-radius: 18px;
    padding: 12px 16px;
  }

  .perm-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .perm-icon { font-size: 18px; flex-shrink: 0; }

  .perm-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .perm-tool {
    font-size: 13px;
    font-weight: 600;
    color: #c8a8f0;
    font-family: monospace;
  }

  .perm-input {
    font-size: 11px;
    color: #5a5a80;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .perm-btns {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }

  button {
    font-size: 12px;
    font-weight: 500;
    padding: 5px 14px;
    border-radius: 10px;
    border: none;
    cursor: pointer;
    transition: all 0.15s;
  }

  .btn-deny {
    background: rgba(224,85,85,0.15);
    color: #e05555;
    border: 1px solid rgba(224,85,85,0.25);
  }
  .btn-deny:hover { background: rgba(224,85,85,0.3); }

  .btn-allow {
    background: #7c6af7;
    color: white;
  }
  .btn-allow:hover { background: #9080ff; }

  .session-list {
    background: rgba(15, 15, 22, 0.92);
    border: 1px solid rgba(255,255,255,0.06);
    border-radius: 18px;
    padding: 6px 8px;
    display: flex;
    flex-direction: column;
  }

  .session-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 12px;
    transition: background 0.1s;
  }
  .session-row:hover { background: rgba(255,255,255,0.04); }

  .s-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
    animation: pulse 1.8s ease-in-out infinite;
  }

  .s-info {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .s-cwd {
    font-size: 13px;
    color: #c8c8d8;
    font-weight: 500;
  }

  .s-summary {
    font-size: 11px;
    color: #55556a;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }
</style>

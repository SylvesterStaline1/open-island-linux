<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";

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

  // Window sizes (logical px)
  const WIN_W = 600;
  const WIN_W_IDLE = 200;
  // Pill heights
  const PILL_H = 60;
  const PILL_H_IDLE = 38;
  const SESSION_H = 58;
  const PERMISSION_H = 108;
  const MAX_H = 400;

  let sessions: Session[] = $state([]);
  let permissionEvent: PermissionEvent | null = $state(null);
  let userExpanded = $state(false);
  let unlisteners: UnlistenFn[] = [];

  const activeSessions = $derived(sessions.filter(s => s.phase !== "Completed"));
  const hasPermission = $derived(permissionEvent !== null);
  const isExpanded = $derived(hasPermission || userExpanded);

  // Dot color reflects current status
  const dotColor = $derived(
    hasPermission                                                          ? "#f5a623" :
    activeSessions.some(s => s.phase === "Working")                       ? "#7c6af7" :
    activeSessions.some(s => s.phase === "AwaitingPermission")            ? "#f5a623" :
    activeSessions.length > 0                                             ? "#5040a0" :
                                                                            "#1e1e30"
  );
  const isPulsing = $derived(activeSessions.length > 0 || hasPermission);

  // Auto-expand when permission arrives
  $effect(() => {
    if (hasPermission) userExpanded = true;
  });

  // Drive window size + centering together
  $effect(() => {
    const width  = isExpanded ? WIN_W : WIN_W_IDLE;
    const pillH  = isExpanded ? PILL_H : PILL_H_IDLE;
    const sessH  = isExpanded ? Math.min(activeSessions.length, 4) * SESSION_H : 0;
    const permH  = hasPermission ? PERMISSION_H : 0;
    const height = Math.min(pillH + sessH + permH, MAX_H);
    invoke("set_window_geometry", { width, height }).catch(() => {});
  });

  onMount(async () => {
    sessions = await invoke<Session[]>("get_sessions").catch(() => []);

    unlisteners.push(
      await listen<Session[]>("sessions-changed", (e) => { sessions = e.payload; }),
      await listen<PermissionEvent>("permission-requested", (e) => { permissionEvent = e.payload; }),
      await listen<string>("permission-resolved", () => { permissionEvent = null; }),
    );
  });

  onDestroy(() => unlisteners.forEach(u => u()));

  function handlePillClick() {
    if (!hasPermission) userExpanded = !userExpanded;
  }

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
      case "Working":            return "#7c6af7";
      case "AwaitingPermission": return "#f5a623";
      case "Completed":          return "#4caf50";
      default:                   return "#44445a";
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
  <!-- Pill -->
  <div
    class="pill"
    class:expanded={isExpanded}
    role="button"
    tabindex="0"
    onclick={handlePillClick}
    onkeydown={(e) => e.key === "Enter" && handlePillClick()}
  >
    <!-- Collapsed layer: dot + name -->
    <div class="layer idle-layer">
      <span
        class="dot"
        class:pulsing={isPulsing}
        style="background:{dotColor};box-shadow:0 0 7px {dotColor}"
      ></span>
      <span class="brand-mini">Open Island</span>
      {#if activeSessions.length > 0}
        <span class="work-badge">{activeSessions.length}</span>
      {/if}
    </div>

    <!-- Expanded layer: full bar -->
    <div class="layer full-layer">
      <div class="pill-left">
        <span
          class="dot"
          class:pulsing={isPulsing}
          style="background:{dotColor};box-shadow:0 0 7px {dotColor}"
        ></span>
        <span class="brand">Open Island</span>
      </div>

      <div class="pill-center">
        {#if activeSessions.length === 0}
          <span class="no-agents">no agents</span>
        {:else}
          {#each activeSessions.slice(0, 3) as s (s.session_id)}
            <span class="chip">
              <span class="chip-dot" style="background:{phaseColor(s.phase)};box-shadow:0 0 4px {phaseColor(s.phase)}"></span>
              <span class="chip-cwd">{shortCwd(s.cwd)}</span>
            </span>
          {/each}
          {#if activeSessions.length > 3}
            <span class="more">+{activeSessions.length - 3}</span>
          {/if}
        {/if}
      </div>

      <div class="pill-right">
        {#if hasPermission}
          <span class="badge-perm">approval</span>
        {:else if activeSessions.length > 0}
          <span class="chevron">{userExpanded ? "▲" : "▼"}</span>
        {/if}
      </div>
    </div>
  </div>

  <!-- Permission panel -->
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

  <!-- Session list -->
  {#if userExpanded && !hasPermission && activeSessions.length > 0}
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
    width: 100%;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  /* ── Pill ── */
  .pill {
    position: relative;
    height: 38px;
    background: rgba(10, 10, 17, 0.97);
    backdrop-filter: blur(32px);
    border: 1px solid rgba(255,255,255,0.07);
    border-radius: 0 0 10px 10px;
    cursor: pointer;
    overflow: hidden;
    transition:
      height        0.32s cubic-bezier(0.34, 1.56, 0.64, 1),
      border-radius 0.32s ease,
      background    0.25s ease;
    -webkit-app-region: drag;
  }

  .pill.expanded {
    height: 60px;
    border-radius: 0 0 14px 14px;
    background: rgba(12, 12, 20, 0.98);
  }

  .pill:hover { background: rgba(18, 18, 26, 0.98); }
  .pill.expanded:hover { background: rgba(15, 15, 23, 0.99); }

  /* Content layers — stacked, cross-fade */
  .layer {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    transition: opacity 0.22s ease;
    -webkit-app-region: no-drag;
  }

  .idle-layer {
    justify-content: center;
    gap: 8px;
    padding: 0 14px;
    opacity: 1;
  }

  .full-layer {
    padding: 0 20px;
    gap: 12px;
    opacity: 0;
    pointer-events: none;
  }

  .pill.expanded .idle-layer {
    opacity: 0;
    pointer-events: none;
  }

  .pill.expanded .full-layer {
    opacity: 1;
    pointer-events: auto;
  }

  /* Dot */
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    flex-shrink: 0;
    transition: background 0.4s ease, box-shadow 0.4s ease;
  }
  .dot.pulsing { animation: pulse 1.9s ease-in-out infinite; }

  /* Collapsed labels */
  .brand-mini {
    font-size: 11px;
    font-weight: 600;
    color: #606080;
    letter-spacing: 0.02em;
    font-family: -apple-system, "Inter", "Segoe UI", sans-serif;
  }

  .work-badge {
    font-size: 10px;
    color: #9080ff;
    background: rgba(124,106,247,0.14);
    border-radius: 8px;
    padding: 1px 6px;
  }

  /* Expanded left */
  .pill-left {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .brand {
    font-size: 12px;
    font-weight: 600;
    color: #b8b8cc;
    letter-spacing: 0.01em;
    font-family: -apple-system, "Inter", "Segoe UI", sans-serif;
  }

  /* Expanded center */
  .pill-center {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    overflow: hidden;
  }

  .no-agents {
    font-size: 11px;
    color: #252535;
    font-family: -apple-system, "Inter", "Segoe UI", sans-serif;
  }

  .chip {
    display: flex;
    align-items: center;
    gap: 5px;
    background: rgba(255,255,255,0.05);
    border-radius: 9px;
    padding: 2px 8px;
    min-width: 0;
  }

  .chip-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    flex-shrink: 0;
    animation: pulse 1.9s ease-in-out infinite;
  }

  .chip-cwd {
    font-size: 11px;
    color: #8888a8;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 130px;
    font-family: monospace;
  }

  .more {
    font-size: 10px;
    color: #3a3a55;
  }

  /* Expanded right */
  .pill-right {
    flex-shrink: 0;
    display: flex;
    align-items: center;
  }

  .badge-perm {
    font-size: 10px;
    color: #f5a623;
    background: rgba(245,166,35,0.11);
    border-radius: 7px;
    padding: 2px 8px;
    animation: pulse 1.2s ease-in-out infinite;
    font-family: -apple-system, "Inter", "Segoe UI", sans-serif;
  }

  .chevron {
    font-size: 9px;
    color: #38384e;
  }

  /* ── Permission panel ── */
  .perm-panel {
    background: rgba(18, 10, 32, 0.95);
    border: 1px solid rgba(124,106,247,0.28);
    border-radius: 16px;
    padding: 11px 15px;
  }

  .perm-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .perm-icon { font-size: 17px; flex-shrink: 0; }

  .perm-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .perm-tool {
    font-size: 12px;
    font-weight: 600;
    color: #c0a0f0;
    font-family: monospace;
  }

  .perm-input {
    font-size: 10px;
    color: #505068;
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
    font-size: 11px;
    font-weight: 500;
    padding: 5px 13px;
    border-radius: 9px;
    border: none;
    cursor: pointer;
    transition: all 0.15s;
    font-family: -apple-system, "Inter", "Segoe UI", sans-serif;
  }

  .btn-deny {
    background: rgba(224,85,85,0.13);
    color: #e05555;
    border: 1px solid rgba(224,85,85,0.22);
  }
  .btn-deny:hover { background: rgba(224,85,85,0.28); }

  .btn-allow {
    background: #7c6af7;
    color: white;
  }
  .btn-allow:hover { background: #9080ff; }

  /* ── Session list ── */
  .session-list {
    background: rgba(13, 13, 21, 0.9);
    border: 1px solid rgba(255,255,255,0.05);
    border-radius: 16px;
    padding: 3px 8px;
    display: flex;
    flex-direction: column;
  }

  .session-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 10px;
    border-radius: 11px;
    transition: background 0.1s;
  }
  .session-row:hover { background: rgba(255,255,255,0.03); }

  .s-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
    animation: pulse 1.9s ease-in-out infinite;
  }

  .s-info {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .s-cwd {
    font-size: 12px;
    color: #b8b8cc;
    font-weight: 500;
    font-family: -apple-system, "Inter", "Segoe UI", sans-serif;
  }

  .s-summary {
    font-size: 10px;
    color: #484860;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: -apple-system, "Inter", "Segoe UI", sans-serif;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50%       { opacity: 0.3; }
  }
</style>

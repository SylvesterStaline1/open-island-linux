<script lang="ts">
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

  let { session, faded = false }: { session: Session; faded?: boolean } = $props();

  function shortCwd(cwd: string | null): string {
    if (!cwd) return "";
    const parts = cwd.split("/");
    return parts.slice(-2).join("/");
  }

  function phaseLabel(phase: SessionPhase): string {
    switch (phase) {
      case "Working": return "working";
      case "AwaitingPermission": return "needs approval";
      case "AwaitingQuestion": return "waiting";
      case "Completed": return "done";
      default: return "idle";
    }
  }

  function phaseDotClass(phase: SessionPhase): string {
    switch (phase) {
      case "Working": return "dot-working";
      case "AwaitingPermission": return "dot-permission";
      case "Completed": return "dot-done";
      default: return "dot-idle";
    }
  }
</script>

<div class="card" class:faded>
  <div class="card-header">
    <span class="phase-dot {phaseDotClass(session.phase)}"></span>
    <span class="cwd">{shortCwd(session.cwd)}</span>
    <span class="phase-label">{phaseLabel(session.phase)}</span>
  </div>

  {#if session.summary}
    <p class="summary">{session.summary}</p>
  {/if}

  {#if session.pending_permission}
    <div class="permission-badge">
      🔐 {session.pending_permission.tool_name}
    </div>
  {/if}
</div>

<style>
  .card {
    background: #15151f;
    border: 1px solid #1e1e2c;
    border-radius: 10px;
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    transition: opacity 0.2s;
  }

  .card.faded {
    opacity: 0.4;
  }

  .card-header {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .phase-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .dot-working {
    background: #7c6af7;
    box-shadow: 0 0 6px #7c6af7;
    animation: pulse 1.8s ease-in-out infinite;
  }

  .dot-permission {
    background: #f5a623;
    box-shadow: 0 0 6px #f5a623;
  }

  .dot-done {
    background: #4caf50;
  }

  .dot-idle {
    background: #44445a;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }

  .cwd {
    font-size: 13px;
    font-weight: 500;
    color: #c8c8d8;
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .phase-label {
    font-size: 11px;
    color: #55556a;
  }

  .summary {
    font-size: 12px;
    color: #6e6e85;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .permission-badge {
    font-size: 11px;
    color: #f5a623;
    background: #2a1e0a;
    border: 1px solid #3a2a10;
    border-radius: 5px;
    padding: 3px 8px;
    align-self: flex-start;
  }
</style>

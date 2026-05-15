<script lang="ts">
  interface PermissionEvent {
    sessionId: string;
    toolName: string;
    toolInput: unknown;
  }

  let {
    event,
    onAllow,
    onDeny,
  }: {
    event: PermissionEvent;
    onAllow: () => void;
    onDeny: () => void;
  } = $props();

  function formatInput(input: unknown): string {
    if (!input) return "";
    try {
      const str = JSON.stringify(input, null, 2);
      return str.length > 300 ? str.slice(0, 300) + "…" : str;
    } catch {
      return String(input);
    }
  }
</script>

<div class="dialog">
  <div class="dialog-header">
    <span class="lock">🔐</span>
    <span class="title">Permission request</span>
  </div>

  <div class="tool-name">{event.toolName}</div>

  {#if event.toolInput}
    <pre class="tool-input">{formatInput(event.toolInput)}</pre>
  {/if}

  <div class="actions">
    <button class="btn-deny" onclick={onDeny}>Deny</button>
    <button class="btn-allow" onclick={onAllow}>Allow</button>
  </div>
</div>

<style>
  .dialog {
    margin: 12px 16px;
    background: #1a1220;
    border: 1px solid #3a2a50;
    border-radius: 12px;
    padding: 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .dialog-header {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .lock {
    font-size: 16px;
  }

  .title {
    font-size: 13px;
    font-weight: 600;
    color: #c8a8f0;
  }

  .tool-name {
    font-size: 14px;
    font-weight: 600;
    color: #e2e2e8;
    font-family: monospace;
  }

  .tool-input {
    font-size: 11px;
    color: #7070a0;
    background: #0f0f18;
    border-radius: 6px;
    padding: 8px 10px;
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-all;
    max-height: 140px;
    overflow-y: auto;
  }

  .actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  button {
    font-size: 13px;
    font-weight: 500;
    padding: 7px 20px;
    border-radius: 7px;
    border: none;
    cursor: pointer;
    transition: all 0.15s;
  }

  .btn-deny {
    background: #2a1a1a;
    color: #e05555;
    border: 1px solid #3a1e1e;
  }

  .btn-deny:hover {
    background: #3a1e1e;
  }

  .btn-allow {
    background: #7c6af7;
    color: white;
  }

  .btn-allow:hover {
    background: #9080ff;
  }
</style>

<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";

  type SessionPhase = "Idle" | "Working" | "AwaitingPermission" | "AwaitingQuestion" | "Completed";

  interface PendingPermission {
    tool_name: string;
    tool_input: unknown;
  }

  interface Session {
    session_id: string;
    title: string | null;
    cwd: string | null;
    phase: SessionPhase;
    summary: string | null;
    pending_permission: PendingPermission | null;
    pending_question: string | null;
    terminal_tty: string | null;
    terminal_window_id: string | null;
    terminal_app: string | null;
    terminal_session_id: string | null;
    terminal_pid: string | null;
    started_at: number;
    updated_at: number;
  }

  interface PermissionEvent {
    sessionId: string;
    toolName: string;
    toolInput: unknown;
  }

  // ── Layout constants ─────────────────────────────────────────────────────
  const WIN_W         = 480;
  const PILL_H        = 44;
  const SLIVER_OFFSET = 30;   // translateY(-30) → 14px peeks below KDE panel
  const PANEL_GAP     = 8;    // gap between pill bottom and panel top

  // ── State ────────────────────────────────────────────────────────────────
  let sessions: Session[] = $state([]);
  let permissionEvent: PermissionEvent | null = $state(null);
  let userExpanded = $state(false);
  let isHovered = $state(false);
  let hoverLeaveTimer: ReturnType<typeof setTimeout> | null = null;
  let pillEl: HTMLDivElement | null = $state(null);
  let pillWidth = $state(220);        // estimated until measured
  let panelEl: HTMLDivElement | null = $state(null);
  let panelHeight = $state(0);
  let appliedHeight = $state(PILL_H - SLIVER_OFFSET);
  let unlisteners: UnlistenFn[] = [];

  const activeSessions = $derived(sessions.filter(s => s.phase !== "Completed"));

  const urgentSession = $derived(
    activeSessions.find(s => s.pending_permission) ??
    activeSessions.find(s => s.pending_question) ?? null
  );

  const panelVariant = $derived(
    urgentSession?.pending_permission ? "code"
    : urgentSession?.pending_question ? "question"
    : "list"
  );

  const isAwaiting = $derived(urgentSession !== null);

  // Pill rests at sliver whenever no urgent session, even with active sessions
  const isSliver = $derived(!isHovered && !userExpanded && !isAwaiting);

  // Auto-open panel when urgent
  $effect(() => { if (isAwaiting) userExpanded = true; });

  // Up to 3 tool names for pill badges: urgent first, then working sessions
  const pillTopTools = $derived((() => {
    const tools: string[] = [];
    if (urgentSession?.pending_permission) {
      tools.push(urgentSession.pending_permission.tool_name);
    } else if (urgentSession?.pending_question) {
      tools.push("question");
    }
    for (const s of activeSessions) {
      if (tools.length >= 3) break;
      if (s === urgentSession) continue;
      tools.push(currentToolOf(s));
    }
    if (tools.length === 0) tools.push("power");
    return tools.slice(0, 3);
  })());

  // ── Hover handlers ───────────────────────────────────────────────────────
  function handleMouseEnter() {
    if (hoverLeaveTimer) { clearTimeout(hoverLeaveTimer); hoverLeaveTimer = null; }
    isHovered = true;
  }
  function handleMouseLeave() {
    hoverLeaveTimer = setTimeout(() => { isHovered = false; hoverLeaveTimer = null; }, 250);
  }

  // ── Panel height measurement ─────────────────────────────────────────────
  // scrollHeight gives natural content height even when an ancestor constrains
  // the rendered box; ResizeObserver still drives reactivity. Extra re-measures
  // catch font-load reflow and post-transition settling.
  $effect(() => {
    if (!panelEl) return;
    const el = panelEl;

    const remeasure = () => {
      const h = el.scrollHeight;
      if (h > 0 && h !== panelHeight) {
        panelHeight = h;
      }
    };

    const ro = new ResizeObserver(remeasure);
    ro.observe(el);
    document.fonts?.ready.then(remeasure).catch(() => {});
    const t1 = setTimeout(remeasure, 50);
    const t2 = setTimeout(remeasure, 250);

    return () => {
      ro.disconnect();
      clearTimeout(t1);
      clearTimeout(t2);
    };
  });

  // ── Measure pill width so sliver mode can shrink window to fit ──────────
  $effect(() => {
    const el = pillEl;
    if (!el) return;
    const ro = new ResizeObserver(() => { pillWidth = el.getBoundingClientRect().width; });
    ro.observe(el);
    pillWidth = el.getBoundingClientRect().width;
    return () => ro.disconnect();
  });

  // ── Window geometry: grow fast, shrink after close animation ────────────
  $effect(() => {
    const targetH = isSliver
      ? PILL_H - SLIVER_OFFSET
      : PILL_H + (userExpanded ? PANEL_GAP + panelHeight : 0);
    const targetW = isSliver && pillWidth > 0
      ? Math.ceil(pillWidth + 8)
      : WIN_W;
    if (targetH >= appliedHeight) {
      appliedHeight = targetH;
      invoke("set_window_geometry", { width: targetW, height: targetH }).catch(() => {});
    } else {
      const id = setTimeout(() => {
        appliedHeight = targetH;
        invoke("set_window_geometry", { width: targetW, height: targetH }).catch(() => {});
      }, 470);
      return () => clearTimeout(id);
    }
  });

  // ── Mount / destroy ──────────────────────────────────────────────────────
  onMount(async () => {
    sessions = await invoke<Session[]>("get_sessions").catch(() => []);
    unlisteners.push(
      await listen<Session[]>("sessions-changed", (e) => {
        sessions = e.payload;
        if (permissionEvent) {
          const owner = sessions.find(s => s.session_id === permissionEvent!.sessionId);
          if (!owner || !owner.pending_permission) {
            permissionEvent = null;
          }
        }
      }),
      await listen<PermissionEvent>("permission-requested", (e) => {
        permissionEvent = e.payload;
      }),
      await listen<string>("permission-resolved", () => {
        permissionEvent = null;
      }),
      await listen("tauri://blur", () => {
        if (!isAwaiting) userExpanded = false;
      }),
    );
  });

  onDestroy(() => unlisteners.forEach(u => u()));

  function handlePillClick() {
    if (!isAwaiting) userExpanded = !userExpanded;
  }

  async function focusSession(sessionId: string) {
    await invoke("focus_session_terminal", { session_id: sessionId }).catch(() => {});
  }

  async function handlePermission(allow: boolean, sessionId: string) {
    console.log("[OI] handlePermission clicked:", allow, sessionId);
    try {
      await invoke("resolve_permission", { session_id: sessionId, allow });
      console.log("[OI] resolve_permission succeeded");
      permissionEvent = null;
      sessions = sessions.map(s =>
        s.session_id === sessionId ? { ...s, phase: "Working", pending_permission: null } as Session : s
      );
    } catch (e) {
      console.error("[OI] resolve_permission FAILED:", e);
    }
  }

  // ── Helpers ──────────────────────────────────────────────────────────────
  function currentToolOf(s: Session): string {
    if (s.pending_permission) return s.pending_permission.tool_name;
    if (s.summary) {
      const m = s.summary.match(/^Running (\w+)$/);
      if (m) return m[1];
    }
    return "bash";
  }

  function shortCwd(cwd: string | null): string {
    if (!cwd) return "~";
    // Normalize Windows backslashes, then strip home prefix on either platform.
    const normalized = cwd.replace(/\\/g, "/");
    const stripped = normalized
      .replace(/^[A-Za-z]:\/[Uu]sers\/[^/]+/, "~")
      .replace(/^\/home\/[^/]+/, "~");
    const parts = stripped.split("/");
    return parts.slice(-2).join("/") || "~";
  }

  function shortPath(p: string | null | undefined): string {
    if (!p) return "";
    const normalized = (p as string).replace(/\\/g, "/");
    const stripped = normalized
      .replace(/^[A-Za-z]:\/[Uu]sers\/[^/]+/, "~")
      .replace(/^\/home\/[^/]+/, "~");
    const parts = stripped.split("/");
    return parts.slice(-2).join("/") || p;
  }

  function truncate(s: string, n: number): string {
    return s.length > n ? s.slice(0, n - 1) + "…" : s;
  }

  function relTime(startedAt: number): string {
    const secs = Math.max(0, Math.floor(Date.now() / 1000 - startedAt));
    if (secs < 60) return `${secs}s`;
    const mins = Math.floor(secs / 60);
    if (mins < 60) return `${mins}m`;
    return `${Math.floor(mins / 60)}h`;
  }

  // ── Tool glyph maps ──────────────────────────────────────────────────────
  const TOOL_GLYPHS: Record<string, string> = {
    bash:
      "............" + "............" + "..#........." + "..##........" +
      "..###......." + "..####......" + "..###......." + "..##........" +
      "..#........." + "............" + ".....#####.." + "............",
    edit:
      "............" + "............" + ".........##." + "........###." +
      ".......###.." + "......###..." + ".....###...." + "....###....." +
      "...###......" + "..###......." + ".##........." + ".#..........",
    write:
      "............" + "..#######..." + "..#.....#..." + "..#.###.#..." +
      "..#.....#..." + "..#.###.#..." + "..#.....#..." + "..#.###.#..." +
      "..#.....#..." + "..#.....#..." + "..#######..." + "............",
    multiedit:
      "............" + "..######...." + "..#....#...." + "..#.######.." +
      "..######.#.." + ".......#.#.." + ".......#.#.." + ".......#.#.." +
      ".......#.#.." + ".......#.#.." + ".......###.." + "............",
    notebook:
      "............" + ".##########." + ".##.......#." + ".##.#####.#." +
      ".##.......#." + ".##.#####.#." + ".##.......#." + ".##.#####.#." +
      ".##.......#." + ".##.#####.#." + ".##########." + "............",
    webfetch:
      "............" + "....####...." + "..##.##.##.." + ".#..#.##..#." +
      ".#..#.##..#." + ".########.#." + ".########.#." + ".#..#.##..#." +
      ".#..#.##..#." + "..##.##.##.." + "....####...." + "............",
    websearch:
      "............" + "..####......" + ".##..##....." + ".#....#....." +
      ".#....#....." + ".#....#....." + ".##..##....." + "..####......" +
      "......##...." + ".......##..." + "........##.." + ".........#..",
    read:
      "............" + "............" + "....####...." + "..##....##.." +
      ".#..####..#." + ".#.######.#." + ".#.######.#." + ".#..####..#." +
      "..##....##.." + "....####...." + "............" + "............",
    check:
      "............" + "............" + "............" + "..........#." +
      ".........##." + ".#......##.." + ".##....##..." + "..##..##...." +
      "...####....." + "....##......" + "............" + "............",
    alert:
      "............" + ".....##....." + "....####...." + "....####...." +
      "...##..##..." + "...##..##..." + "..##.##.##.." + "..##.##.##.." +
      ".##..##..##." + ".###########" + ".###########" + "............",
    power:
      "............" + "......#....." + "......#....." + "...#..#..#.." +
      ".##.......##" + ".#.........#" + ".#.........#" + ".#.........#" +
      ".##.......##" + "...##...##.." + ".....###...." + "............",
    chevronDown:
      "............" + "............" + "............" + ".#........#." +
      ".##......##." + "..##....##.." + "...##..##..." + "....####...." +
      ".....##....." + "............" + "............" + "............",
    chevronUp:
      "............" + "............" + ".....##....." + "....####...." +
      "...##..##..." + "..##....##.." + ".##......##." + ".#........#." +
      "............" + "............" + "............" + "............",
    close:
      "............" + ".##......##." + ".###....###." + "..###..###.." +
      "...######..." + "....####...." + "....####...." + "...######..." +
      "..###..###.." + ".###....###." + ".##......##." + "............",
    play:
      "............" + "...##......." + "...####....." + "...######..." +
      "...########." + "...#########" + "...#########" + "...########." +
      "...######..." + "...####....." + "...##......." + "............",
    question:
      "............" + "....####...." + "...##..##..." + "..##....##.." +
      "..##....##.." + "........##.." + ".......##..." + "......##...." +
      "......##...." + "............" + "......##...." + "............",
  };

  const TOOL_GLYPH_MAP: Record<string, string> = {
    Bash: "bash", Edit: "edit", Write: "write", MultiEdit: "multiedit",
    NotebookEdit: "notebook", WebFetch: "webfetch", WebSearch: "websearch",
    Read: "read", question: "question",
  };

  const TOOL_LABEL: Record<string, string> = {
    Bash: "BASH", Edit: "EDIT", Write: "WRITE", MultiEdit: "MULTIEDIT",
    NotebookEdit: "NOTEBOOK", WebFetch: "WEB FETCH", WebSearch: "WEB SEARCH",
    Read: "READ",
  };

  function dotGlyph(key: string, size: number = 16, color: string = "currentColor"): string {
    const src = TOOL_GLYPHS[key] ?? "";
    if (src.length !== 144) return `<svg viewBox="0 0 120 120" width="${size}" height="${size}" aria-hidden="true"></svg>`;
    const cell = 10, r = (cell * 0.65) / 2, vw = 12 * cell;
    const circles: string[] = [];
    for (let y = 0; y < 12; y++)
      for (let x = 0; x < 12; x++)
        if (src[y * 12 + x] === "#")
          circles.push(`<circle cx="${x * cell + cell / 2}" cy="${y * cell + cell / 2}" r="${r}" fill="${color}"/>`);
    return `<svg viewBox="0 0 ${vw} ${vw}" width="${size}" height="${size}" style="display:block;flex-shrink:0" aria-hidden="true">${circles.join("")}</svg>`;
  }

  // ── Diff builder ─────────────────────────────────────────────────────────
  interface DiffRow { num: string | number; type: "ctx" | "add" | "del"; text: string }
  interface DiffResult { lines: DiffRow[]; added: number; removed: number; path: string; title: string }

  function buildDiff(p: PendingPermission): DiffResult {
    const inp = (p.tool_input ?? {}) as Record<string, unknown>;
    switch (p.tool_name) {
      case "Edit": {
        const oldLines = String(inp.old_string ?? "").split("\n");
        const newLines = String(inp.new_string ?? "").split("\n");
        const rows: DiffRow[] = [
          ...oldLines.map((l, i) => ({ num: i + 1, type: "del" as const, text: truncate(l, 60) })),
          ...newLines.map((l, i) => ({ num: i + 1, type: "add" as const, text: truncate(l, 60) })),
        ];
        return { lines: rows.slice(0, 8), added: newLines.length, removed: oldLines.length,
                 path: shortPath(inp.file_path as string), title: `Edit ${shortPath(inp.file_path as string)}` };
      }
      case "MultiEdit": {
        const edits = (inp.edits as Record<string, unknown>[] | undefined)?.[0] ?? {};
        const oldLines = String((edits as Record<string,unknown>).old_string ?? "").split("\n");
        const newLines = String((edits as Record<string,unknown>).new_string ?? "").split("\n");
        const rows: DiffRow[] = [
          ...oldLines.map((l, i) => ({ num: i + 1, type: "del" as const, text: truncate(l, 60) })),
          ...newLines.map((l, i) => ({ num: i + 1, type: "add" as const, text: truncate(l, 60) })),
        ];
        const fp = ((edits as Record<string,unknown>).file_path as string) ?? inp.file_path as string ?? "";
        return { lines: rows.slice(0, 8), added: newLines.length, removed: oldLines.length,
                 path: shortPath(fp), title: `MultiEdit ${shortPath(fp)}` };
      }
      case "Write": {
        const lines = String(inp.content ?? "").split("\n").slice(0, 6)
          .map((l, i) => ({ num: i + 1, type: "add" as const, text: truncate(l, 60) }));
        return { lines, added: String(inp.content ?? "").split("\n").length, removed: 0,
                 path: shortPath(inp.file_path as string), title: `Write ${shortPath(inp.file_path as string)}` };
      }
      case "NotebookEdit": {
        const lines = String(inp.new_source ?? "").split("\n").slice(0, 6)
          .map((l, i) => ({ num: i + 1, type: "add" as const, text: truncate(l, 60) }));
        return { lines, added: String(inp.new_source ?? "").split("\n").length, removed: 0,
                 path: shortPath(inp.notebook_path as string), title: `Notebook ${shortPath(inp.notebook_path as string)}` };
      }
      case "Bash":
        return { lines: [{ num: " ", type: "ctx", text: truncate(`$ ${inp.command ?? ""}`, 80) }],
                 added: 0, removed: 0, path: "BASH", title: "Run command" };
      case "WebFetch":
        return { lines: [{ num: " ", type: "ctx", text: truncate(String(inp.url ?? ""), 80) }],
                 added: 0, removed: 0, path: "WEB FETCH", title: "Fetch URL" };
      case "WebSearch":
        return { lines: [{ num: " ", type: "ctx", text: truncate(String(inp.query ?? ""), 80) }],
                 added: 0, removed: 0, path: "WEB SEARCH", title: "Web search" };
      default: {
        const j = truncate(JSON.stringify(inp), 100);
        return { lines: [{ num: " ", type: "ctx", text: j }],
                 added: 0, removed: 0, path: p.tool_name.toUpperCase(), title: p.tool_name };
      }
    }
  }

  function parseQuestion(q: string): { question: string; options: string[] } {
    // Future: parse structured format. For v1, treat the whole string as the question.
    return { question: q, options: [] };
  }

  function otherSessions(exclude: Session): Session[] {
    return activeSessions.filter(s => s.session_id !== exclude.session_id);
  }
</script>

<!-- Hover wrapper: no transform — reliable hit area for sliver hover -->
<div
  class="hover-wrapper"
  onmouseenter={handleMouseEnter}
  onmouseleave={handleMouseLeave}
  role="presentation"
>
<div class="root" class:sliver={isSliver}>

  <!-- ── Pill row (centers pill horizontally in the 480px window) ── -->
  <div class="pill-row">
    <div
      class="pill"
      bind:this={pillEl}
      role="button"
      tabindex="0"
      onclick={handlePillClick}
      onkeydown={(e) => e.key === "Enter" && handlePillClick()}
    >
      <!-- In sliver mode: show nothing, keep pill clean. Only show badges/count/chevron when expanded/hovered. -->
      {#if !isSliver}
        <!-- Overlapping tool badges (urgent first, red + pulsing ring) -->
        <div class="pill-badges">
          {#each pillTopTools as t, i (i)}
            <div class="badge-wrap" style="margin-left:{i === 0 ? 0 : -8}px; z-index:{pillTopTools.length - i};">
              <div class="tool-badge tool-badge-26" class:tool-badge-red={isAwaiting && i === 0}>
                {@html dotGlyph(TOOL_GLYPH_MAP[t] ?? "bash", 20, "#FFFFFF")}
              </div>
              {#if isAwaiting && i === 0}
                <span class="ring-pulse"></span>
              {/if}
            </div>
          {/each}
        </div>

        <!-- Agent count -->
        {#if activeSessions.length > 0}
          <span class="pill-count">{activeSessions.length}</span>
          <!-- Separator -->
          <span class="pill-sep"></span>
        {/if}

        <!-- Chevron (right side, rotates on expand) -->
        <span class="pill-chevron" class:pill-chevron-up={userExpanded}>
          {@html dotGlyph("chevronDown", 18, "var(--text-tertiary)")}
        </span>
      {/if}
    </div>
  </div>

  <!-- ── Panel (slides open below pill) ── -->
  <div class="panel-clip" class:panel-clip-open={userExpanded}>
    <div class="panel" bind:this={panelEl}>

      {#if panelVariant === "code" && urgentSession?.pending_permission}
        <!-- Code approval panel -->
        {@const perm = urgentSession.pending_permission}
        {@const diff = buildDiff(perm)}

        <!-- Awaiting header -->
        <div class="aw-header">
          <div class="badge-wrap">
            <div class="tool-badge tool-badge-26 tool-badge-red">
              {@html dotGlyph(TOOL_GLYPH_MAP[perm.tool_name] ?? "alert", 20, "#FFFFFF")}
            </div>
            <span class="ring-pulse ring-pulse-sm"></span>
          </div>
          <span class="aw-title">{diff.title}</span>
          <span class="tag tag-red">AWAITING</span>
          <span class="cond-time">{relTime(urgentSession.updated_at)}</span>
        </div>

        <!-- Diff block -->
        {#if diff.lines.length > 0}
          <div class="diff-block">
            {#each diff.lines as l}
              <div class="diff-line" class:diff-line-add={l.type === "add"} class:diff-line-del={l.type === "del"}>
                <span class="diff-num">{l.num}</span>
                <span class="diff-mark"
                  class:diff-mark-add={l.type === "add"}
                  class:diff-mark-del={l.type === "del"}
                  class:diff-mark-ctx={l.type === "ctx"}>
                  {l.type === "add" ? "+" : l.type === "del" ? "−" : " "}
                </span>
                <span class="diff-text"
                  class:diff-text-add={l.type === "add"}
                  class:diff-text-del={l.type === "del"}
                  class:diff-text-ctx={l.type === "ctx"}>
                  {l.text}
                </span>
              </div>
            {/each}
          </div>
          <div class="diff-meta">
            {#if diff.added > 0}<span class="diff-meta-add">+{diff.added}</span>{/if}
            {#if diff.removed > 0}<span class="diff-meta-del">−{diff.removed}</span>{/if}
            <span class="diff-meta-path">{diff.path}</span>
          </div>
        {/if}

        <!-- Allow/Deny pill buttons + Open Terminal secondary -->
        <div class="action-row">
          <button class="btn-deny" onclick={() => handlePermission(false, urgentSession!.session_id)}>
            Deny <span class="kbd">2</span>
          </button>
          <button class="btn-allow" onclick={() => handlePermission(true, urgentSession!.session_id)}>
            Allow <span class="kbd">1</span>
          </button>
        </div>
        <div class="action-row action-row-secondary">
          <button
            class="btn-open-terminal"
            onclick={() => focusSession(urgentSession!.session_id)}
          >
            OPEN TERMINAL
          </button>
        </div>

        <!-- Other sessions (dim) -->
        {#if otherSessions(urgentSession).length > 0}
          <div class="hairline"></div>
          {#each otherSessions(urgentSession) as s, i (s.session_id)}
            {@render condensedRow(s, i === 0, true)}
          {/each}
        {/if}

      {:else if panelVariant === "question" && urgentSession?.pending_question}
        <!-- Question panel (dormant for v1 — backend doesn't populate pending_question yet) -->
        {@const parsed = parseQuestion(urgentSession.pending_question)}

        <div class="aw-header">
          <div class="badge-wrap">
            <div class="tool-badge tool-badge-26 tool-badge-red">
              {@html dotGlyph("question", 20, "#FFFFFF")}
            </div>
            <span class="ring-pulse ring-pulse-sm"></span>
          </div>
          <span class="aw-title">Claude asks</span>
          <span class="tag tag-red">ASKS</span>
          <span class="cond-time">{relTime(urgentSession.updated_at)}</span>
        </div>

        <div class="q-block">
          <p class="q-text">{parsed.question}</p>
          {#if parsed.options.length > 0}
            <div class="q-opts">
              {#each parsed.options as opt, i (i)}
                <button class="q-opt" onclick={() => focusSession(urgentSession!.session_id)}>
                  <span class="kbd">{i + 1}</span>
                  <span class="q-opt-text">{opt}</span>
                </button>
              {/each}
            </div>
          {:else}
            <button class="btn-open-terminal" onclick={() => focusSession(urgentSession!.session_id)}>
              OPEN TERMINAL
            </button>
          {/if}
        </div>

        {#if otherSessions(urgentSession).length > 0}
          <div class="hairline"></div>
          {#each otherSessions(urgentSession) as s, i (s.session_id)}
            {@render condensedRow(s, i === 0, true)}
          {/each}
        {/if}

      {:else}
        <!-- Session list panel (quiet) -->
        {#if activeSessions.length === 0}
          <div class="cond-row cond-row-first">
            <div class="tool-badge tool-badge-22">
              {@html dotGlyph("power", 16, "var(--text-disabled)")}
            </div>
            <span class="cond-project" style="color:var(--text-tertiary)">No active agents</span>
          </div>
        {:else}
          {#each activeSessions as s, i (s.session_id)}
            {@render condensedRow(s, i === 0, false)}
          {/each}
        {/if}
      {/if}

    </div>
  </div>

</div>
</div>

<!-- ── Snippet: CondensedRow ─────────────────────────────────────────────── -->
{#snippet condensedRow(s: Session, first: boolean, dim: boolean)}
  {@const tool = currentToolOf(s)}
  {@const label = TOOL_LABEL[tool] ?? tool.toUpperCase()}
  <div
    class="cond-row"
    class:cond-row-first={first}
    class:cond-row-dim={dim}
    role="button"
    tabindex="0"
    onclick={() => focusSession(s.session_id)}
    onkeydown={(e) => e.key === "Enter" && focusSession(s.session_id)}
  >
    <div class="tool-badge tool-badge-22">
      {@html dotGlyph(TOOL_GLYPH_MAP[tool] ?? "bash", 16, "#FFFFFF")}
    </div>
    <span class="cond-project">{shortCwd(s.cwd)}</span>
    <span class="tag">{label}</span>
    <span class="cond-time">{relTime(s.started_at)}</span>
  </div>
{/snippet}

<style>
  /* ── Nothing DS foundation tokens ──────────────────────────────────────── */
  :global(:root) {
    --nothing-red:         #D81F26;
    --nothing-red-hover:   #B81920;
    --nothing-white:       #FFFFFF;
    --nothing-black:       #000000;
    --surface-0:           #000000;
    --surface-1:           #121212;
    --surface-2:           #1F1F1F;
    --surface-3:           #2B2B2B;
    --text-primary:        #FFFFFF;
    --text-secondary:      #BFBFBF;
    --text-tertiary:       #7A7A7A;
    --text-disabled:       #3D3D3D;
    --divider:             #2B2B2B;
    --font-body:           "NType 82", system-ui, -apple-system, "Helvetica Neue", sans-serif;
    --font-mono:           "JetBrains Mono", "IBM Plex Mono", monospace;
    --font-display:        "Ndot-57", "JetBrains Mono", monospace;
    --ease:                cubic-bezier(0.2, 0, 0, 1);
  }

  :global(*) { box-sizing: border-box; margin: 0; padding: 0; }

  :global(html, body) {
    background: transparent !important;
    overflow: hidden;
    user-select: none;
    -webkit-user-select: none;
  }

  /* ── Keyframes ──────────────────────────────────────────────────────────── */
  @keyframes oi-pulse {
    0%, 100% { opacity: 0.2; }
    40%      { opacity: 1;   }
  }

  @keyframes oi-ring {
    0%   { box-shadow: 0 0 0 2px rgba(216,31,38,1);   opacity: 1; transform: scale(1);    }
    100% { box-shadow: 0 0 0 6px rgba(216,31,38,0);   opacity: 0; transform: scale(1.15); }
  }

  /* ── Layout shell ───────────────────────────────────────────────────────── */
  .hover-wrapper {
    width: 100%;
    height: 100%;
  }

  .root {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 100%;
    transform: translateY(0);
    transition: transform 500ms var(--ease);
  }

  .root.sliver {
    transform: translateY(-30px);
  }

  /* ── Pill row + pill ────────────────────────────────────────────────────── */
  .pill-row {
    display: flex;
    justify-content: center;
    width: 100%;
  }

  .pill {
    display: inline-flex;
    align-items: center;
    gap: 14px;
    height: 44px;
    padding: 0 16px;
    background: #0A0A0A;
    color: var(--text-primary);
    border-radius: 0 0 12px 12px;
    box-shadow: 0 6px 20px rgba(0,0,0,0.55), 0 0 0 1px rgba(255,255,255,0.04) inset;
    font-family: var(--font-body);
    cursor: pointer;
    -webkit-app-region: drag;
    transition: background 150ms linear;
  }

  .pill:hover { background: #121212; }

  /* ── Pill badge cluster ─────────────────────────────────────────────────── */
  .pill-badges {
    display: inline-flex;
    align-items: center;
    -webkit-app-region: no-drag;
  }

  .badge-wrap {
    position: relative;
    display: inline-flex;
    flex-shrink: 0;
  }

  /* ── Tool badges ────────────────────────────────────────────────────────── */
  .tool-badge {
    background: var(--surface-2);
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    border-radius: 8px;
  }

  .tool-badge-26 { width: 26px; height: 26px; }
  .tool-badge-22 { width: 22px; height: 22px; }
  .tool-badge-red { background: var(--nothing-red); }

  /* ── Pulsing ring around urgent badge ──────────────────────────────────── */
  .ring-pulse {
    position: absolute;
    inset: -5px;
    border-radius: 14px;
    box-shadow: 0 0 0 2px var(--nothing-red);
    animation: oi-ring 1400ms var(--ease) infinite;
    pointer-events: none;
  }

  .ring-pulse-sm {
    inset: -4px;
    border-radius: 12px;
  }

  /* ── Pill count ─────────────────────────────────────────────────────────── */
  .pill-count {
    font-family: var(--font-mono);
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    letter-spacing: 0.04em;
    flex-shrink: 0;
    -webkit-app-region: no-drag;
  }

  /* ── Separator ──────────────────────────────────────────────────────────── */
  .pill-sep {
    width: 1px;
    height: 18px;
    background: var(--divider);
    flex-shrink: 0;
  }

  /* ── Chevron (right side) ───────────────────────────────────────────────── */
  .pill-chevron {
    display: inline-flex;
    align-items: center;
    flex-shrink: 0;
    -webkit-app-region: no-drag;
    transform: rotate(0deg);
    transition: transform 350ms var(--ease);
  }

  .pill-chevron-up {
    transform: rotate(180deg);
  }

  /* ── Panel clip (fade + slide, no max-height) ───────────────────────────── */
  /* max-height animation was removed: in WebKit2GTK it constrained .panel's
     flex-stretched height to intermediate animation values, causing the
     ResizeObserver to under-report panelHeight and the window to be sized
     short. The window resize via set_window_geometry IS the reveal. */
  .panel-clip {
    opacity: 0;
    transform: translateY(-8px);
    transition:
      opacity    300ms var(--ease),
      transform  400ms var(--ease);
    margin-top: 8px;
    width: 100%;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    pointer-events: none;
  }

  .panel-clip-open {
    opacity: 1;
    transform: translateY(0);
    pointer-events: auto;
  }

  /* ── Panel shell ────────────────────────────────────────────────────────── */
  .panel {
    width: 480px;
    background: #0A0A0A;
    border-radius: 18px;
    overflow: hidden;
    box-shadow: 0 12px 32px rgba(0,0,0,0.65), 0 0 0 1px rgba(255,255,255,0.04) inset;
    padding-bottom: 4px;
  }

  /* ── Condensed row ──────────────────────────────────────────────────────── */
  .cond-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border-top: 1px solid var(--divider);
    cursor: pointer;
    transition: background 150ms var(--ease);
  }

  .cond-row:hover { background: rgba(255,255,255,0.03); }
  .cond-row-first { border-top: none; }
  .cond-row-dim   { opacity: 0.85; }

  .cond-project {
    font-family: var(--font-body);
    font-size: 12.5px;
    font-weight: 500;
    color: var(--text-primary);
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cond-time {
    font-family: var(--font-mono);
    font-size: 10.5px;
    color: var(--text-tertiary);
    letter-spacing: 0.02em;
    min-width: 22px;
    text-align: right;
    flex-shrink: 0;
  }

  /* ── Tag ────────────────────────────────────────────────────────────────── */
  .tag {
    display: inline-flex;
    align-items: center;
    padding: 2px 7px;
    border-radius: 6px;
    background: var(--surface-2);
    font-family: var(--font-mono);
    font-size: 10px;
    letter-spacing: 0.04em;
    color: var(--text-secondary);
    font-weight: 500;
    line-height: 1.5;
    flex-shrink: 0;
  }

  .tag-red {
    background: rgba(216,31,38,0.18);
    color: var(--nothing-red);
  }

  /* ── Kbd ────────────────────────────────────────────────────────────────── */
  .kbd {
    display: inline-flex;
    align-items: center;
    padding: 1px 5px;
    border-radius: 4px;
    background: rgba(255,255,255,0.08);
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--text-secondary);
    font-weight: 500;
    line-height: 1.4;
  }

  /* ── Awaiting header ────────────────────────────────────────────────────── */
  .aw-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 14px 10px;
  }

  .aw-title {
    font-family: var(--font-body);
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Diff block ─────────────────────────────────────────────────────────── */
  .diff-block {
    margin: 0 14px;
    background: #000;
    border-radius: 10px;
    padding: 8px 0;
    font-family: var(--font-mono);
    font-size: 11.5px;
    line-height: 1.5;
    overflow: hidden;
  }

  .diff-line {
    display: flex;
    align-items: center;
    padding: 2px 0;
  }

  .diff-line-add { background: rgba(70,180,90,0.10); }
  .diff-line-del { background: rgba(216,31,38,0.14); }

  .diff-num {
    width: 30px;
    color: var(--text-tertiary);
    text-align: right;
    padding-right: 8px;
    font-size: 10.5px;
    flex-shrink: 0;
  }

  .diff-mark {
    width: 14px;
    text-align: center;
    flex-shrink: 0;
  }

  .diff-mark-add { color: rgb(120,210,140); }
  .diff-mark-del { color: rgb(240,120,125); }
  .diff-mark-ctx { color: var(--text-tertiary); }

  .diff-text      { flex: 1; padding-right: 12px; }
  .diff-text-add  { color: rgb(170,225,185); }
  .diff-text-del  { color: rgb(240,150,155); }
  .diff-text-ctx  { color: var(--text-secondary); }

  .diff-meta {
    padding: 8px 14px 12px;
    display: flex;
    align-items: center;
    gap: 10px;
    font-family: var(--font-mono);
    font-size: 10.5px;
  }

  .diff-meta-add  { color: rgb(140,220,160); font-weight: 500; }
  .diff-meta-del  { color: rgb(240,150,155); font-weight: 500; }
  .diff-meta-path {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--text-tertiary);
    letter-spacing: 0.06em;
    margin-left: auto;
    text-transform: uppercase;
  }

  /* ── Action row ─────────────────────────────────────────────────────────── */
  .action-row {
    padding: 8px 12px 12px;
    display: flex;
    gap: 8px;
  }

  .btn-open-terminal {
    flex: 1;
    height: 36px;
    border-radius: 10px;
    border: 0;
    cursor: pointer;
    background: var(--nothing-red);
    color: var(--nothing-white);
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    font-family: var(--font-mono);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    transition: background 150ms var(--ease);
  }

  .btn-open-terminal:hover { background: var(--nothing-red-hover); }

  .btn-deny {
    flex: 1; height: 36px; border-radius: 10px; cursor: pointer;
    background: rgba(255,255,255,0.06); color: var(--text-primary);
    border: 1px solid var(--surface-3);
    display: flex; align-items: center; justify-content: center; gap: 8px;
    font-family: var(--font-body); font-size: 12.5px; font-weight: 500;
    transition: background 150ms var(--ease);
  }
  .btn-deny:hover { background: rgba(255,255,255,0.10); }

  .btn-allow {
    flex: 1; height: 36px; border-radius: 10px; border: 0; cursor: pointer;
    background: var(--nothing-red); color: var(--nothing-white);
    display: flex; align-items: center; justify-content: center; gap: 8px;
    font-family: var(--font-body); font-size: 12.5px; font-weight: 600;
    transition: background 150ms var(--ease);
  }
  .btn-allow:hover { background: var(--nothing-red-hover); }

  .action-row-secondary {
    padding-top: 0;
  }
  .action-row-secondary .btn-open-terminal {
    background: rgba(255,255,255,0.04);
    color: var(--text-secondary);
    font-size: 10.5px;
    height: 30px;
  }
  .action-row-secondary .btn-open-terminal:hover {
    background: rgba(255,255,255,0.08);
  }

  /* ── Hairline divider ───────────────────────────────────────────────────── */
  .hairline {
    height: 1px;
    background: var(--divider);
  }

  /* ── Question block ─────────────────────────────────────────────────────── */
  .q-block {
    padding: 2px 14px 12px;
  }

  .q-text {
    margin: 0 0 10px;
    font-size: 13px;
    color: var(--text-primary);
    line-height: 1.45;
  }

  .q-opts {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .q-opt {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 11px;
    border-radius: 9px;
    background: rgba(255,255,255,0.04);
    border: 1px solid var(--surface-2);
    color: var(--text-primary);
    cursor: pointer;
    text-align: left;
    font-family: var(--font-body);
    transition: background 150ms var(--ease);
  }

  .q-opt:hover { background: rgba(255,255,255,0.07); }

  .q-opt-text {
    font-size: 12.5px;
    font-weight: 500;
  }
</style>

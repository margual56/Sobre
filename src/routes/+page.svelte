<script lang="ts">
  import { onMount } from "svelte";
  import { actions, app, refreshStatus, startCompose, startListening, step, toast } from "$lib/app.svelte";
  import { errorText } from "$lib/format";
  import TrayWarning from "$lib/components/TrayWarning.svelte";
  import AskDialog from "$lib/components/AskDialog.svelte";
  import AccountSetup from "$lib/components/AccountSetup.svelte";
  import ActionBar from "$lib/components/ActionBar.svelte";
  import Compose from "$lib/components/Compose.svelte";
  import Gate from "$lib/components/Gate.svelte";
  import MessageList from "$lib/components/MessageList.svelte";
  import Reader from "$lib/components/Reader.svelte";
  import StatsDialog from "$lib/components/StatsDialog.svelte";
  import SettingsDialog from "$lib/components/SettingsDialog.svelte";

  let listWidth = $state(380);
  let dragging = $state(false);

  onMount(async () => {
    try {
      await startListening();
      await refreshStatus();
    } catch (e) {
      toast(errorText(e));
    }
  });

  function drag(e: PointerEvent) {
    if (dragging) listWidth = Math.min(Math.max(e.clientX, 260), window.innerWidth - 360);
  }

  function keys(e: KeyboardEvent) {
    if (app.stage !== "ready" || app.compose || app.dialog || app.question) return;
    const target = e.target as HTMLElement;
    if (target.matches("input, textarea, select")) {
      if (e.key === "Escape") target.blur();
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const handlers: Record<string, () => unknown> = {
      j: () => step(1),
      ArrowDown: () => step(1),
      k: () => step(-1),
      ArrowUp: () => step(-1),
      c: () => startCompose("new"),
      r: () => app.detail && startCompose("reply"),
      a: () => app.detail && startCompose("replyAll"),
      f: () => app.detail && startCompose("forward"),
      u: actions.toggleRead,
      s: actions.toggleStar,
      e: actions.archive,
      Delete: actions.remove,
      "!": actions.spam,
      "/": () => document.getElementById("search")?.focus(),
      Escape: () => (app.link = null),
    };
    const handler = handlers[e.key];
    if (handler) {
      e.preventDefault();
      void handler();
    }
  }
</script>

<svelte:window onkeydown={keys} onpointermove={drag} onpointerup={() => (dragging = false)} />

{#if app.stage === "loading"}
  <div class="center muted">Starting…</div>
{:else if app.stage !== "ready"}
  <Gate />
{:else}
  <div class="shell" class:dragging>
    <ActionBar />
    <div class="panes">
      <div class="left" style:width="{listWidth}px"><MessageList /></div>
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div class="divider" onpointerdown={() => (dragging = true)}></div>
      <div class="right"><Reader /></div>
    </div>
  </div>
  {#if app.compose}{#key app.compose}<Compose init={app.compose} onclose={() => (app.compose = null)} />{/key}{/if}
  {#if app.dialog === "settings"}<SettingsDialog />{/if}
  {#if app.dialog === "stats"}<StatsDialog />{/if}
  {#if app.dialog === "account"}<AccountSetup onclose={() => (app.dialog = null)} />{/if}
{/if}

<AskDialog />
{#if app.stage !== "loading"}<TrayWarning />{/if}

{#if app.toast}<div class="toast selectable" role="status">{app.toast}</div>{/if}

<style>
  .center { height: 100vh; display: grid; place-items: center; }
  .shell { height: 100vh; display: flex; flex-direction: column; }
  .panes { flex: 1; display: flex; min-height: 0; }
  .left { flex: none; min-width: 0; }
  .right { flex: 1; min-width: 0; }
  .divider { width: 5px; margin: 0 -2px; cursor: col-resize; z-index: 1; border-left: 1px solid var(--line); margin-left: 0; }
  .divider:hover, .dragging .divider { background: var(--accent-soft); }
  /* While resizing, the iframe must not swallow the pointer. */
  .dragging :global(iframe) { pointer-events: none; }
  .toast {
    position: fixed; bottom: 18px; left: 50%; transform: translateX(-50%); z-index: 50; max-width: min(640px, 90vw);
    background: var(--text); color: var(--bg); padding: 9px 16px; border-radius: 9px; box-shadow: var(--shadow); font-size: 13.5px;
  }
</style>

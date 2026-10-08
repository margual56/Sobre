<script lang="ts">
  import { TriangleAlert, X } from "@lucide/svelte";
  import { app } from "$lib/app.svelte";

  // Dismissed for this run only; it comes back on the next start.
  let dismissed = $state(false);
  const missing = $derived(app.trayProblem === "missing-library");
</script>

{#if app.trayProblem && !dismissed}
  <aside class="warning" role="alert">
    <TriangleAlert size={18} />
    <div>
      <b>No tray icon</b>
      {#if missing}
        <p>A system library is missing, so Sobre cannot sit in the tray. It will quit when you close this window and will not check mail in the background.</p>
        <p>Install it and restart Sobre:</p>
        <ul class="selectable">
          <li>Arch: <code>sudo pacman -S libayatana-appindicator</code></li>
          <li>Debian, Ubuntu: <code>sudo apt install libayatana-appindicator3-1</code></li>
          <li>Fedora: <code>sudo dnf install libayatana-appindicator-gtk3</code></li>
        </ul>
      {:else}
        <p>The tray icon could not be created, so Sobre will quit when you close this window and will not check mail in the background.</p>
        <p class="selectable muted">{app.trayProblem}</p>
      {/if}
    </div>
    <button class="x" title="Dismiss" onclick={() => (dismissed = true)}><X size={15} /></button>
  </aside>
{/if}

<style>
  .warning {
    position: fixed; left: 16px; bottom: 16px; z-index: 30; width: min(430px, calc(100vw - 32px)); display: flex; gap: 10px;
    background: var(--warn-soft); color: var(--text); border: 1px solid var(--warn); border-radius: 11px;
    box-shadow: var(--shadow); padding: 13px 12px 11px 14px; font-size: 13px;
  }
  .warning > :global(svg) { flex: none; color: var(--warn); margin-top: 1px; }
  .warning div { flex: 1; min-width: 0; }
  p { margin: 4px 0; }
  ul { margin: 4px 0 0; padding-left: 18px; }
  li { margin: 3px 0; }
  .x { border: 0; background: none; border-radius: 5px; width: 24px; height: 24px; display: grid; place-items: center; flex: none; }
  .x:hover { background: rgb(128 128 128 / 0.2); }
</style>

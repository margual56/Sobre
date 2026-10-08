<script lang="ts">
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import { Download, X } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { errorText, fileSize } from "$lib/format";

  let update = $state<{ version: string; notes: string; size: number } | null>(null);
  let phase = $state<"offer" | "downloading" | "done" | "failed">("offer");
  let percent = $state(0);
  let error = $state("");

  onMount(() => {
    // A failed check is not worth interrupting anyone for.
    api.checkUpdate().then((found) => (update = found)).catch(() => {});
    const stop = listen<number>("update-progress", ({ payload }) => (percent = payload));
    return () => void stop.then((off) => off());
  });

  async function install() {
    phase = "downloading";
    percent = 0;
    try {
      await api.installUpdate();
      phase = "done";
    } catch (e) {
      error = errorText(e);
      phase = "failed";
    }
  }
</script>

{#if update}
  <aside class="popup" role="status">
    <button class="x" title="Later" disabled={phase === "downloading"} onclick={() => (update = null)}><X size={15} /></button>
    {#if phase === "offer"}
      <b>Sobre {update.version} is available</b>
      <span class="muted">{fileSize(update.size)} download</span>
      <div class="row">
        <button class="btn small primary" onclick={install}><Download size={14} />Update</button>
        <button class="btn small" onclick={() => (update = null)}>Later</button>
      </div>
    {:else if phase === "downloading"}
      <b>Downloading Sobre {update.version}…</b>
      <div class="track"><div class="fill" style:width="{percent}%"></div></div>
      <span class="muted">{percent}%</span>
    {:else if phase === "done"}
      <b>Sobre {update.version} is installed</b>
      <span class="muted">Restart to start using it.</span>
      <div class="row">
        <button class="btn small primary" onclick={() => api.restart()}>Restart now</button>
        <button class="btn small" onclick={() => (update = null)}>Later</button>
      </div>
    {:else}
      <b>The update did not install</b>
      <span class="error selectable">{error}</span>
      <div class="row"><button class="btn small" onclick={install}>Try again</button></div>
    {/if}
  </aside>
{/if}

<style>
  .popup {
    position: fixed; right: 16px; bottom: 16px; z-index: 30; width: 290px; display: flex; flex-direction: column; gap: 6px;
    background: var(--raised); border: 1px solid var(--line); border-radius: 11px; box-shadow: var(--shadow); padding: 13px 14px;
  }
  .x { position: absolute; top: 7px; right: 7px; border: 0; background: none; border-radius: 5px; width: 24px; height: 24px; display: grid; place-items: center; color: var(--muted); }
  .x:hover:not(:disabled) { background: var(--hover); }
  .row { margin-top: 4px; gap: 7px; }
  .muted, .error { font-size: 12.5px; margin: 0; }
  .track { height: 5px; border-radius: 3px; background: var(--hover); overflow: hidden; margin-top: 4px; }
  .fill { height: 100%; background: var(--accent); transition: width 0.2s; }
</style>

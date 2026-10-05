<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { api, type BackendEvent } from "$lib/api";
  import { app, loadTheme, type ComposeInit } from "$lib/app.svelte";
  import { errorText } from "$lib/format";
  import AskDialog from "$lib/components/AskDialog.svelte";
  import Compose from "$lib/components/Compose.svelte";

  let init = $state<ComposeInit | null>(null);
  let error = $state("");

  onMount(async () => {
    try {
      const id = Number(new URLSearchParams(location.search).get("draft"));
      await loadTheme();
      app.accounts = await api.accounts();
      init = await api.takeComposeDraft<ComposeInit>(id);
      if (!init) error = "This draft is no longer available.";
      // The window shows decrypted text; it must not outlive a lock.
      await listen<BackendEvent>("backend", ({ payload }) => {
        if (payload.type === "locked") void api.closeWindow();
        if (payload.type === "theme_changed") void loadTheme();
      });
    } catch (e) {
      error = errorText(e);
    }
  });
</script>

{#if init}
  <Compose {init} standalone onclose={() => api.closeWindow()} />
{:else if error}
  <p class="error">{error}</p>
{/if}
<AskDialog />

<style>
  .error { padding: 24px; }
</style>

<script lang="ts">
  import { api, type ServerConfig } from "$lib/api";
  import { app, loadEverything, toast } from "$lib/app.svelte";
  import { errorText } from "$lib/format";

  let { onclose }: { onclose: () => void } = $props();

  let email = $state("");
  let name = $state("");
  let config = $state<ServerConfig | null>(null);
  let password = $state("");
  let username = $state("");
  let usePassword = $state(false);
  let advanced = $state(false);
  let clientId = $state("");
  let clientSecret = $state("");
  let clientSaved = $state(false);
  let busy = $state("");
  let error = $state("");

  const provider = $derived(config?.oauth_provider ?? null);
  const oauth = $derived(provider !== null && !usePassword);
  const providerName = $derived(provider === "google" ? "Google" : provider === "microsoft" ? "Microsoft" : "");

  async function lookUp() {
    error = "";
    busy = "Looking up your provider…";
    try {
      config = await api.discover(email.trim());
      advanced = config.source === "guess";
      if (config.oauth_provider) {
        const client = await api.getOAuthClient(config.oauth_provider);
        clientId = client.client_id;
        clientSecret = client.client_secret;
        clientSaved = client.client_id !== "";
      }
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = "";
    }
  }

  // The backend announces the switch and the app reloads itself around it.
  async function openDemo() {
    error = "";
    try {
      await api.enterDemo();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function add() {
    if (!config) return;
    error = "";
    try {
      if (oauth) {
        if (!clientId.trim()) throw `Paste your ${providerName} OAuth client ID first.`;
        await api.setOAuthClient({ provider: provider!, client_id: clientId, client_secret: clientSecret });
        busy = `Waiting for you to sign in with ${providerName} in your browser…`;
      } else {
        busy = "Signing in…";
      }
      await api.addAccount({
        email: email.trim(),
        display_name: name.trim(),
        username: username.trim() || null,
        config: $state.snapshot(config),
        password: oauth ? null : password,
      });
      await loadEverything();
      toast(`${email.trim()} added. Downloading mail…`);
      onclose();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = "";
    }
  }
</script>

<div class="overlay">
  <div class="modal">
    <h2>Add an email account</h2>
    {#if !config}
      <form onsubmit={(e) => { e.preventDefault(); void lookUp(); }}>
        <label class="field"><span>Email address</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input type="email" bind:value={email} autofocus required placeholder="you@example.com" />
        </label>
        <label class="field"><span>Your name, as recipients will see it</span><input bind:value={name} /></label>
        {#if error}<p class="error selectable">{error}</p>{/if}
        <div class="row end">
          {#if app.accounts.length}
            <button type="button" class="btn" onclick={onclose}>Cancel</button>
          {:else if !app.demo}
            <button type="button" class="skip" disabled={!!busy} onclick={openDemo}>Skip for now, open the demo account</button>
          {/if}
          <button class="btn primary" disabled={!!busy}>{busy || "Continue"}</button>
        </div>
      </form>
    {:else}
      <p class="muted selectable">{email} · {config.imap_host}</p>

      {#if oauth}
        <p>{providerName} accounts sign in through your browser. The app never sees your password.</p>
        {#if !clientSaved}
          <div class="box">
            <p><b>One-time setup.</b> {providerName} requires each mail app to identify itself with an OAuth client that you create (free, about five minutes). The steps are in <code>docs/google-oauth.md</code> on the project page.</p>
            <label class="field"><span>Client ID</span><input bind:value={clientId} spellcheck="false" /></label>
            <label class="field"><span>Client secret {provider === "microsoft" ? "(leave empty)" : ""}</span><input bind:value={clientSecret} spellcheck="false" /></label>
          </div>
        {/if}
        <button class="linkish" onclick={() => (usePassword = true)}>Use an app password instead</button>
      {:else}
        <label class="field"><span>{provider ? "App password" : "Password"}</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input type="password" bind:value={password} autofocus />
        </label>
        {#if provider}<button class="linkish" onclick={() => (usePassword = false)}>Sign in with {providerName} instead</button>{/if}
      {/if}

      <button class="linkish" onclick={() => (advanced = !advanced)}>{advanced ? "Hide" : "Show"} server settings</button>
      {#if advanced}
        <div class="box">
          {#if config.source === "guess"}<p class="muted">These servers are a guess from your address. Check them against your provider's instructions.</p>{/if}
          <div class="grid">
            <label class="field"><span>IMAP server (TLS)</span><input bind:value={config.imap_host} spellcheck="false" /></label>
            <label class="field"><span>Port</span><input type="number" bind:value={config.imap_port} /></label>
            <label class="field"><span>SMTP server</span><input bind:value={config.smtp_host} spellcheck="false" /></label>
            <label class="field"><span>Port</span><input type="number" bind:value={config.smtp_port} /></label>
          </div>
          <label class="check"><input type="checkbox" bind:checked={config.smtp_starttls} /> SMTP uses STARTTLS (usually port 587)</label>
          <label class="field"><span>Username, if different from the address</span><input bind:value={username} spellcheck="false" /></label>
        </div>
      {/if}

      {#if error}<p class="error selectable">{error}</p>{/if}
      {#if busy}<p class="muted">{busy}</p>{/if}
      <div class="row end">
        <button class="btn" disabled={!!busy} onclick={() => { config = null; error = ""; }}>Back</button>
        <button class="btn primary" disabled={!!busy || (!oauth && !password)} onclick={add}>{oauth ? `Sign in with ${providerName}` : "Add account"}</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .end { justify-content: flex-end; margin-top: 16px; }
  .skip { border: 0; background: transparent; color: var(--accent); padding: 7px 0; border-radius: 7px; margin-right: auto; }
  .skip:hover:not(:disabled) { text-decoration: underline; }
  .skip:disabled { opacity: 0.5; cursor: default; }
  .box { border: 1px solid var(--line); border-radius: 9px; padding: 12px 12px 2px; margin: 10px 0; }
  .box p { margin: 0 0 10px; }
  .grid { display: grid; grid-template-columns: 1fr 90px; gap: 0 10px; }
  .linkish { display: block; border: 0; background: none; color: var(--accent); padding: 4px 0; font-size: 13px; }
  .check { display: flex; gap: 7px; align-items: center; margin-bottom: 12px; font-size: 13px; }
</style>

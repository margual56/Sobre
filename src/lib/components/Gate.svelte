<script lang="ts">
  import { KeyRound, Lock, Wallet } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { app, refreshStatus } from "$lib/app.svelte";
  import { errorText } from "$lib/format";

  let choice = $state<"wallet" | "passphrase">("wallet");
  let passphrase = $state("");
  let repeat = $state("");
  let busy = $state(false);
  let error = $state("");

  async function run(work: () => Promise<void>) {
    error = "";
    busy = true;
    try {
      await work();
      passphrase = repeat = "";
      await refreshStatus();
    } catch (e) {
      error = errorText(e);
    } finally {
      busy = false;
    }
  }

  function create() {
    if (choice === "passphrase" && passphrase !== repeat) return (error = "The two passphrases do not match.");
    void run(() => api.createStore(choice === "passphrase" ? passphrase : null));
  }
</script>

<main>
  {#if app.stage === "new"}
    <div class="card">
      <h1>Welcome</h1>
      <p class="muted">Your mail is stored on this computer in an encrypted database. Choose how its key is kept. You can change this later.</p>
      <label class="option" class:on={choice === "wallet"}>
        <input type="radio" bind:group={choice} value="wallet" />
        <Wallet size={22} />
        <span><b>System wallet</b> (recommended)<br /><span class="muted">Opens by itself and checks mail in the background. Protects against a stolen disk and other users; a program running as you could still ask the wallet for the key.</span></span>
      </label>
      <label class="option" class:on={choice === "passphrase"}>
        <input type="radio" bind:group={choice} value="passphrase" />
        <KeyRound size={22} />
        <span><b>Passphrase</b><br /><span class="muted">Only this app, with your passphrase, can read your mail. You type it every time the app starts, and mail is not checked until you do.</span></span>
      </label>
      {#if choice === "passphrase"}
        <div class="row">
          <input type="password" bind:value={passphrase} placeholder="Passphrase (8+ characters)" style="flex:1" />
          <input type="password" bind:value={repeat} placeholder="Repeat it" style="flex:1" />
        </div>
      {/if}
      {#if error}<p class="error selectable">{error}</p>{/if}
      <button class="btn primary" disabled={busy || (choice === "passphrase" && passphrase.length < 8)} onclick={create}>{busy ? "Setting up…" : "Continue"}</button>
    </div>
  {:else if app.stage === "locked"}
    <form class="card narrow" onsubmit={(e) => { e.preventDefault(); void run(() => api.unlock(app.keyMode === "passphrase" ? passphrase : null)); }}>
      <Lock size={28} />
      <h1>Locked</h1>
      {#if app.keyMode === "passphrase"}
        <!-- svelte-ignore a11y_autofocus -->
        <input type="password" bind:value={passphrase} placeholder="Passphrase" autofocus />
      {:else}
        <p class="muted">The key could not be read from the system wallet. Unlock your wallet and try again.</p>
      {/if}
      {#if error}<p class="error selectable">{error}</p>{/if}
      <button class="btn primary" disabled={busy || (app.keyMode === "passphrase" && !passphrase)}>{busy ? "Unlocking…" : app.keyMode === "passphrase" ? "Unlock" : "Try again"}</button>
    </form>
  {/if}
</main>

<style>
  main { height: 100vh; display: grid; place-items: center; padding: 20px; overflow: auto; }
  .card { background: var(--panel); border: 1px solid var(--line); border-radius: 14px; padding: 28px; width: min(540px, 100%); display: flex; flex-direction: column; gap: 14px; }
  .card.narrow { width: min(340px, 100%); align-items: stretch; text-align: center; }
  .card.narrow :global(svg) { margin: 0 auto; color: var(--muted); }
  h1 { margin: 0; font-size: 22px; }
  p { margin: 0; }
  .option { display: flex; gap: 12px; align-items: flex-start; border: 1px solid var(--line); border-radius: 10px; padding: 13px; cursor: pointer; }
  .option.on { border-color: var(--accent); background: var(--accent-soft); }
  .option input { display: none; }
  .option :global(svg) { flex: none; margin-top: 2px; }
  .btn { justify-content: center; }
</style>

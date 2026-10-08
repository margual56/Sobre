<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { ChartColumn, Trash2, X } from "@lucide/svelte";
  import { api, type Settings } from "$lib/api";
  import { app, applyTheme, ask, attempt, loadEverything, refreshStatus, toast } from "$lib/app.svelte";

  let settings = $state<Settings | null>(null);
  let blocked = $state<string[]>([]);
  let newBlock = $state("");
  let passphrase = $state("");
  let passphrase2 = $state("");
  let working = $state(false);
  let version = $state("");
  void getVersion().then((v) => (version = v)).catch(() => {});

  void attempt(async () => {
    settings = await api.settings();
    blocked = await api.blocked();
  });

  const save = () => attempt(() => api.setSettings($state.snapshot(settings!)));

  async function removeAccount(id: number, email: string) {
    if (!(await ask(`Remove ${email} from this computer? Mail stays on the server.`, "Remove"))) return;
    await attempt(() => api.removeAccount(id));
    await loadEverything();
  }
  async function addBlock() {
    const added = await attempt(() => api.block(newBlock));
    if (added) {
      newBlock = "";
      blocked = await api.blocked();
    }
  }
  async function unblock(pattern: string) {
    await attempt(() => api.unblock(pattern));
    blocked = await api.blocked();
  }
  async function switchMode(mode: "wallet" | "passphrase") {
    if (mode === "passphrase" && passphrase !== passphrase2) return toast("The two passphrases do not match.");
    working = true;
    const ok = await attempt(async () => {
      await api.setKeyMode(mode, mode === "passphrase" ? passphrase : null);
      return true;
    });
    working = false;
    if (ok) {
      passphrase = passphrase2 = "";
      await refreshStatus();
      toast(mode === "passphrase" ? "The mail store is now protected by your passphrase." : "The key is now kept in the system wallet.");
    }
  }
</script>

<div class="overlay">
  <div class="modal">
    <div class="title"><h2>Settings</h2><button class="x" onclick={() => (app.dialog = null)}><X size={18} /></button></div>

    <h3>Accounts</h3>
    {#each app.accounts as a}
      <div class="item">
        <span class="selectable">{a.email} <span class="muted">· {a.auth_kind === "oauth" ? "browser sign-in" : "password"}{#if app.sync[a.id]} · {app.sync[a.id].state}{/if}</span></span>
        <button class="btn small danger" onclick={() => removeAccount(a.id, a.email)}><Trash2 size={14} />Remove</button>
      </div>
      {#if app.sync[a.id]?.state === "error"}<p class="error selectable">{app.sync[a.id].detail}</p>{/if}
    {/each}
    <button class="btn small" onclick={() => (app.dialog = "account")}>Add account…</button>

    {#if settings}
      <h3>General</h3>
      <div class="theme">
        <span>Appearance</span>
        <div class="seg">
          {#each [["system", "System"], ["light", "Light"], ["dark", "Dark"]] as const as [value, label]}
            <button class:on={settings.theme === value} onclick={() => { settings!.theme = value; applyTheme(value); void save(); }}>{label}</button>
          {/each}
        </div>
      </div>
      <label class="check"><input type="checkbox" bind:checked={settings.notifications} onchange={save} /> Notify me about new mail</label>
      <label class="check"><input type="checkbox" bind:checked={settings.autostart} onchange={save} /> Start in the background when I log in</label>
      <label class="check"><input type="checkbox" bind:checked={settings.check_updates} onchange={save} /> Check for a new version when the app starts (AppImage only)</label>
      <label class="check"><input type="checkbox" bind:checked={settings.fetch_icons} onchange={save} /> Fetch sender icons from their websites</label>
      <label class="check"><input type="checkbox" bind:checked={settings.auto_junk_failed} onchange={save} /> Move mail that fails verification to Spam automatically</label>
      {#if !app.hasTray}
        <p class="muted">No tray icon: install <code>libayatana-appindicator</code> and restart. Until then, closing the window quits the app.</p>
      {/if}
    {/if}

    <h3>Blocked senders</h3>
    {#each blocked as pattern}
      <div class="item"><span class="selectable">{pattern}</span><button class="btn small" onclick={() => unblock(pattern)}>Unblock</button></div>
    {:else}
      <p class="muted">Nobody is blocked.</p>
    {/each}
    <form class="row" onsubmit={(e) => { e.preventDefault(); void addBlock(); }}>
      <input bind:value={newBlock} placeholder="address or domain" style="flex:1" spellcheck="false" />
      <button class="btn small" disabled={!newBlock.trim()}>Block</button>
    </form>

    <h3>Encryption</h3>
    <p>Everything this app stores is in one encrypted database.
      {#if app.keyMode === "wallet"}
        Its key is kept in your system wallet, so the app opens by itself. Other programs running as you could ask the wallet for that key.
      {:else}
        Its key comes from your passphrase and is stored nowhere. Nothing else on this computer can read your mail.
      {/if}
    </p>
    {#if app.keyMode === "wallet"}
      <div class="row">
        <input type="password" bind:value={passphrase} placeholder="New passphrase (8+ characters)" style="flex:1" />
        <input type="password" bind:value={passphrase2} placeholder="Repeat it" style="flex:1" />
      </div>
      <p><button class="btn small" disabled={working || passphrase.length < 8} onclick={() => switchMode("passphrase")}>{working ? "Re-encrypting…" : "Protect with a passphrase"}</button></p>
      <p class="muted">A forgotten passphrase cannot be recovered; you would add your accounts again and re-download your mail.</p>
    {:else}
      <div class="row">
        <button class="btn small" onclick={() => attempt(api.lock)}>Lock now</button>
        <button class="btn small" disabled={working} onclick={() => switchMode("wallet")}>Keep the key in the system wallet instead</button>
      </div>
    {/if}

    <div class="about">
      <span class="muted">Sobre {version}</span>
      <button class="btn small" onclick={() => (app.dialog = "stats")}><ChartColumn size={14} />Statistics and storage…</button>
    </div>
  </div>
</div>

<style>
  .theme { display: flex; align-items: center; justify-content: space-between; padding: 2px 0 8px; }
  .seg { display: flex; border: 1px solid var(--line); border-radius: 7px; overflow: hidden; }
  .seg button { border: 0; background: none; padding: 5px 14px; color: var(--muted); }
  .seg button + button { border-left: 1px solid var(--line); }
  .seg button.on { background: var(--accent-soft); color: var(--text); font-weight: 600; }
  .about { display: flex; justify-content: space-between; align-items: center; margin-top: 22px; padding-top: 14px; border-top: 1px solid var(--line); }
  .title { display: flex; justify-content: space-between; align-items: flex-start; }
  .x { border: 0; background: none; border-radius: 6px; width: 30px; height: 30px; display: grid; place-items: center; }
  .x:hover { background: var(--hover); }
  .item { display: flex; justify-content: space-between; align-items: center; gap: 10px; padding: 6px 0; border-bottom: 1px solid var(--line); margin-bottom: 8px; }
  .item span { min-width: 0; overflow-wrap: anywhere; }
  .check { display: flex; gap: 8px; align-items: center; padding: 4px 0; }
  p { margin: 8px 0; }
</style>

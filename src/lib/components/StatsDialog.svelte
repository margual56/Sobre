<script lang="ts">
  import { ArrowLeft, X } from "@lucide/svelte";
  import { api, type Stats } from "$lib/api";
  import { app, ask, attempt, toast } from "$lib/app.svelte";
  import { fileSize } from "$lib/format";

  let stats = $state<Stats | null>(null);
  let clearing = $state("");

  const load = () => attempt(async () => (stats = await api.stats()));
  void load();

  const count = (n: number) => n.toLocaleString();
  const verdictLabel: Record<string, string> = { verified: "Verified", unverified: "Unverified", failed: "Failed verification", "not checked": "Not checked yet" };

  async function clear(what: "images" | "mail") {
    const question =
      what === "images"
        ? "Clear cached images and sender icons? They are fetched again when needed."
        : "Clear the local copy of your mail? Nothing is deleted from the server. Recent mail downloads again right away; older messages you had loaded come back with \"Load older messages\".";
    if (!(await ask(question, "Clear"))) return;
    clearing = what;
    const freed = await attempt(() => api.clearStorage(what));
    clearing = "";
    if (freed !== undefined) {
      toast(freed > 0 ? `Freed ${fileSize(freed)}.` : "Cleared.");
      if (what === "mail") {
        app.selectedId = null;
        app.detail = null;
      }
    }
    await load();
  }
</script>

<div class="overlay">
  <div class="modal">
    <div class="title">
      <h2><button class="x" title="Back to settings" onclick={() => (app.dialog = "settings")}><ArrowLeft size={18} /></button>Statistics and storage</h2>
      <button class="x" onclick={() => (app.dialog = null)}><X size={18} /></button>
    </div>

    {#if !stats}
      <p class="muted">Loading…</p>
    {:else}
      <h3>Storage on this computer</h3>
      <p class="big">{fileSize(stats.disk_bytes)} <span class="muted">used by the encrypted database</span></p>
      <table>
        <tbody>
          <tr><th>Downloaded mail</th><td>{count(stats.downloaded)} messages</td><td class="num">{fileSize(stats.mail_bytes)}</td></tr>
          <tr><th>Cached remote images</th><td>{count(stats.image_count)}</td><td class="num">{fileSize(stats.image_bytes)}</td></tr>
          <tr><th>Sender icons</th><td>{count(stats.icon_count)}</td><td class="num">{fileSize(stats.icon_bytes)}</td></tr>
        </tbody>
      </table>
      <div class="row buttons">
        <button class="btn small" disabled={!!clearing} onclick={() => clear("images")}>{clearing === "images" ? "Clearing…" : "Clear images and icons"}</button>
        <button class="btn small danger" disabled={!!clearing} onclick={() => clear("mail")}>{clearing === "mail" ? "Clearing…" : "Clear local mail copy"}</button>
      </div>

      <h3>Mail</h3>
      <table>
        <thead><tr><th></th><th class="num">Received</th><th class="num">Sent</th><th class="num">Spam</th></tr></thead>
        <tbody>
          {#each stats.periods as p}
            <tr><th>{p.label}</th><td class="num">{count(p.received)}</td><td class="num">{count(p.sent)}</td><td class="num">{count(p.spam)}</td></tr>
          {/each}
        </tbody>
      </table>
      <p class="muted small">Counts cover the mail stored on this computer: {count(stats.messages)} messages, {count(stats.unread)} unread in the inbox. Each folder starts with its newest 500.</p>

      {#if stats.accounts.length > 1}
        <h3>By account</h3>
        <table><tbody>{#each stats.accounts as [email, n]}<tr><th class="selectable">{email}</th><td class="num">{count(n)}</td></tr>{/each}</tbody></table>
      {/if}

      {#if stats.top_senders.length}
        <h3>Most mail from, last 30 days</h3>
        <table>
          <tbody>
            {#each stats.top_senders as [addr, name, n]}
              <tr><th class="selectable">{name || addr}{#if name}<span class="muted"> · {addr}</span>{/if}</th><td class="num">{count(n)}</td></tr>
            {/each}
          </tbody>
        </table>
      {/if}

      <h3>Sender verification, received mail</h3>
      <table>
        <tbody>
          {#each stats.verdicts as [verdict, n]}<tr><th>{verdictLabel[verdict] ?? verdict}</th><td class="num">{count(n)}</td></tr>{/each}
          <tr><th>Blocked senders</th><td class="num">{count(stats.blocked)}</td></tr>
        </tbody>
      </table>

      <p class="muted small version">Sobre {stats.version}</p>
    {/if}
  </div>
</div>

<style>
  .title { display: flex; justify-content: space-between; align-items: flex-start; }
  h2 { display: flex; align-items: center; gap: 6px; }
  .x { border: 0; background: none; border-radius: 6px; width: 30px; height: 30px; display: grid; place-items: center; }
  .x:hover { background: var(--hover); }
  .big { font-size: 20px; font-weight: 650; margin: 0 0 8px; }
  .big span { font-size: 13px; font-weight: 400; }
  table { width: 100%; border-collapse: collapse; }
  th, td { text-align: left; padding: 5px 0; border-bottom: 1px solid var(--line); font-weight: 400; overflow-wrap: anywhere; }
  thead th { color: var(--muted); font-size: 12.5px; }
  .num { text-align: right; font-variant-numeric: tabular-nums; white-space: nowrap; padding-left: 14px; }
  .buttons { margin-top: 12px; }
  .small { font-size: 12.5px; }
  p { margin: 8px 0; }
  .version { margin-top: 18px; }
</style>

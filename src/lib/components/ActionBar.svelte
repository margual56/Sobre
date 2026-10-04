<script lang="ts">
  import {
    Archive, Ban, Forward, Inbox, Mail, MailOpen, PenLine, RefreshCw, Reply, ReplyAll, Search, Settings,
    ShieldAlert, Star, Trash2,
  } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { actions, app, attempt, loadMessages, selectedRow, startCompose } from "$lib/app.svelte";

  const roles = [
    ["inbox", "Inbox"], ["sent", "Sent"], ["drafts", "Drafts"], ["archive", "Archive"], ["junk", "Spam"], ["trash", "Trash"],
  ] as const;

  const row = $derived(selectedRow());
  const inJunk = $derived(row?.folder_role === "junk");
  const busy = $derived(Object.values(app.sync).some((s) => s.state === "syncing" || s.state === "connecting"));
  const problem = $derived(Object.entries(app.sync).find(([, s]) => s.state === "error"));
  const otherFolders = $derived(app.folders.filter((f) => f.role === "other" && (app.filter.accountId === null || f.account_id === app.filter.accountId)));
  const where = $derived(app.filter.folderId !== null ? `f:${app.filter.folderId}` : `r:${app.filter.role}`);

  let blockMenu = $state(false);
  let searchTimer: ReturnType<typeof setTimeout>;

  function pickAccount(e: Event) {
    const v = (e.currentTarget as HTMLSelectElement).value;
    app.filter.accountId = v === "" ? null : Number(v);
    app.filter.folderId = null;
    void loadMessages();
  }
  function pickFolder(e: Event) {
    const v = (e.currentTarget as HTMLSelectElement).value;
    if (v.startsWith("f:")) app.filter.folderId = Number(v.slice(2));
    else {
      app.filter.folderId = null;
      app.filter.role = v.slice(2);
    }
    void loadMessages();
  }
  function onSearch() {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(loadMessages, 200);
  }
  function accountName(id: string) {
    return app.accounts.find((a) => a.id === Number(id))?.email ?? "account";
  }
</script>

<header class="bar">
  <div class="group">
    <select value={app.filter.accountId ?? ""} onchange={pickAccount} title="Account">
      <option value="">All accounts</option>
      {#each app.accounts as a}<option value={a.id}>{a.email}</option>{/each}
    </select>
    <select value={where} onchange={pickFolder} title="Folder">
      {#each roles as [role, label]}<option value="r:{role}">{label}</option>{/each}
      {#if otherFolders.length}
        <optgroup label="Folders">
          {#each otherFolders as f}<option value="f:{f.id}">{f.name}</option>{/each}
        </optgroup>
      {/if}
    </select>
  </div>

  <div class="group tools">
    <button class="tool accent" title="New message (c)" onclick={() => startCompose("new")}><PenLine size={17} /></button>
    <span class="sep"></span>
    <button class="tool" title="Reply (r)" disabled={!app.detail} onclick={() => startCompose("reply")}><Reply size={17} /></button>
    <button class="tool" title="Reply all (a)" disabled={!app.detail} onclick={() => startCompose("replyAll")}><ReplyAll size={17} /></button>
    <button class="tool" title="Forward (f)" disabled={!app.detail} onclick={() => startCompose("forward")}><Forward size={17} /></button>
    <span class="sep"></span>
    <button class="tool" title={row?.seen ? "Mark as unread (u)" : "Mark as read (u)"} disabled={!row} onclick={actions.toggleRead}>
      {#if row?.seen}<Mail size={17} />{:else}<MailOpen size={17} />{/if}
    </button>
    <button class="tool" class:on={row?.flagged} title="Star (s)" disabled={!row} onclick={actions.toggleStar}><Star size={17} /></button>
    <button class="tool" title="Archive (e)" disabled={!row || row.folder_role === "archive"} onclick={actions.archive}><Archive size={17} /></button>
    <button class="tool" title={row?.folder_role === "trash" ? "Delete forever (Del)" : "Delete (Del)"} disabled={!row} onclick={actions.remove}><Trash2 size={17} /></button>
    <span class="sep"></span>
    {#if inJunk}
      <button class="tool" title="Not spam: move to Inbox" onclick={actions.notSpam}><Inbox size={17} /></button>
    {:else}
      <button class="tool" title="Report as spam (!)" disabled={!row} onclick={actions.spam}><ShieldAlert size={17} /></button>
    {/if}
    <span class="menu-anchor">
      <button class="tool" title="Block sender" disabled={!row} onclick={() => (blockMenu = !blockMenu)}><Ban size={17} /></button>
      {#if blockMenu && row}
        <button class="backdrop" aria-label="Close menu" onclick={() => (blockMenu = false)}></button>
        <div class="menu">
          <button onclick={() => { blockMenu = false; void actions.block(false); }}>Block <b>{row.from_addr}</b></button>
          <button onclick={() => { blockMenu = false; void actions.block(true); }}>Block everyone at <b>{row.from_addr.split("@")[1]}</b></button>
        </div>
      {/if}
    </span>
  </div>

  <div class="group right">
    {#if problem}
      <span class="problem selectable" title={problem[1].detail}>Cannot reach {accountName(problem[0])}</span>
    {/if}
    <label class="search">
      <Search size={15} />
      <input id="search" type="search" placeholder="Search" bind:value={app.filter.search} oninput={onSearch} />
    </label>
    <button class="tool" class:spin={busy} title="Check for new mail" onclick={() => attempt(api.syncNow)}><RefreshCw size={17} /></button>
    <button class="tool" title="Settings" onclick={() => (app.dialog = "settings")}><Settings size={17} /></button>
  </div>
</header>

<style>
  .bar {
    display: flex; align-items: center; gap: 14px; padding: 8px 12px; background: var(--panel);
    border-bottom: 1px solid var(--line); min-height: 50px;
  }
  .group { display: flex; align-items: center; gap: 6px; }
  .tools { gap: 2px; }
  .right { margin-left: auto; }
  select { padding: 6px 8px; max-width: 190px; }
  .tool {
    border: 0; background: none; border-radius: 7px; width: 34px; height: 34px; display: grid; place-items: center;
    color: var(--text);
  }
  .tool:hover:not(:disabled) { background: var(--hover); }
  .tool:disabled { color: var(--faint); cursor: default; opacity: 0.55; }
  .tool.accent { background: var(--accent); color: #fff; }
  .tool.accent:hover { filter: brightness(1.08); background: var(--accent); }
  .tool.on { color: #e0a000; }
  .tool.on :global(svg) { fill: currentColor; }
  .tool.spin :global(svg) { animation: spin 1s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .sep { width: 1px; height: 20px; background: var(--line); margin: 0 6px; }
  .search {
    display: flex; align-items: center; gap: 6px; background: var(--bg); border: 1px solid var(--line);
    border-radius: 8px; padding: 0 9px; color: var(--muted);
  }
  .search:focus-within { border-color: var(--accent); }
  .search input { border: 0; background: none; padding: 6px 0; width: 180px; }
  .problem { color: var(--bad); font-size: 12.5px; max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .menu-anchor { position: relative; }
  .backdrop { position: fixed; inset: 0; background: none; border: 0; z-index: 9; cursor: default; }
  .menu {
    position: absolute; top: 38px; left: 0; z-index: 10; background: var(--raised); border: 1px solid var(--line);
    border-radius: 9px; box-shadow: var(--shadow); padding: 5px; min-width: 260px;
  }
  .menu button { display: block; width: 100%; text-align: left; border: 0; background: none; padding: 8px 10px; border-radius: 6px; }
  .menu button:hover { background: var(--hover); }
</style>

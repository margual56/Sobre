<script lang="ts">
  import { Paperclip, ShieldX, Star } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { app, attempt, select } from "$lib/app.svelte";
  import { senderLabel, shortDate } from "$lib/format";
  import Avatar from "./Avatar.svelte";

  // Rows are fixed height, so only the visible slice is rendered.
  const ROW = 76;
  let viewport = $state<HTMLDivElement>();
  let scrollTop = $state(0);
  let height = $state(600);

  const first = $derived(Math.max(0, Math.floor(scrollTop / ROW) - 6));
  const last = $derived(Math.min(app.messages.length, Math.ceil((scrollTop + height) / ROW) + 6));
  const visible = $derived(app.messages.slice(first, last));

  // Older mail is only fetched for one concrete folder.
  const olderFolder = $derived.by(() => {
    if (app.filter.search.trim()) return null;
    if (app.filter.folderId !== null) return app.filter.folderId;
    const matching = app.folders.filter((f) => f.role === app.filter.role && (app.filter.accountId === null || f.account_id === app.filter.accountId));
    return matching.length === 1 ? matching[0].id : null;
  });

  $effect(() => {
    // Keep the keyboard selection in view.
    const i = app.messages.findIndex((m) => m.id === app.selectedId);
    if (i < 0 || !viewport) return;
    const top = i * ROW;
    if (top < viewport.scrollTop) viewport.scrollTop = top;
    else if (top + ROW > viewport.scrollTop + viewport.clientHeight) viewport.scrollTop = top + ROW - viewport.clientHeight;
  });
</script>

<div class="list" bind:this={viewport} bind:clientHeight={height} onscroll={(e) => (scrollTop = e.currentTarget.scrollTop)}>
  {#if app.messages.length === 0}
    <p class="empty">{app.filter.search.trim() ? "No messages match." : app.accounts.length ? "Nothing here." : "Add an account to get started."}</p>
  {:else}
    <div style:height="{first * ROW}px"></div>
    {#each visible as m (m.id)}
      <button class="row" class:selected={m.id === app.selectedId} class:unread={!m.seen} onclick={() => select(m.id)}>
        <Avatar name={m.from_name} addr={m.from_addr} verified={m.auth_verdict === "verified"} />
        <span class="text">
          <span class="top">
            <span class="name">{senderLabel(m.from_name, m.from_addr)}</span>
            {#if m.auth_verdict === "failed"}<span class="bad" title="This message failed authenticity checks"><ShieldX size={14} /></span>{/if}
            {#if m.flagged}<span class="star"><Star size={13} /></span>{/if}
            {#if m.has_attachments}<span class="clip"><Paperclip size={13} /></span>{/if}
            <span class="date">{shortDate(m.date)}</span>
          </span>
          <span class="addr">{m.from_name ? m.from_addr : ""}</span>
          <span class="subject"><b>{m.subject || "(no subject)"}</b>{#if m.preview}<span class="preview"> · {m.preview}</span>{/if}</span>
        </span>
      </button>
    {/each}
    <div style:height="{(app.messages.length - last) * ROW}px"></div>
    {#if olderFolder !== null}
      <div class="more"><button class="btn small" onclick={() => attempt(() => api.loadOlder(olderFolder!))}>Load older messages</button></div>
    {/if}
  {/if}
</div>

<style>
  .list { height: 100%; overflow-y: auto; background: var(--panel); }
  .empty { color: var(--muted); text-align: center; margin-top: 60px; }
  .row {
    display: flex; gap: 10px; align-items: center; width: 100%; height: 76px; padding: 0 12px; text-align: left;
    border: 0; border-bottom: 1px solid var(--line); background: none; border-left: 3px solid transparent;
  }
  .row:hover { background: var(--hover); }
  .row.selected { background: var(--accent-soft); border-left-color: var(--accent); }
  .text { min-width: 0; flex: 1; display: flex; flex-direction: column; gap: 1px; }
  .top { display: flex; align-items: center; gap: 5px; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 0 1 auto; color: var(--muted); }
  .unread .name { font-weight: 650; color: var(--text); }
  .date { margin-left: auto; padding-left: 8px; font-size: 12px; color: var(--faint); flex: none; }
  .unread .date { color: var(--accent); font-weight: 600; }
  .addr { font-size: 12px; color: var(--faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-height: 16px; }
  .subject { font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--muted); }
  .subject b { font-weight: 500; color: var(--muted); }
  .unread .subject b { font-weight: 650; color: var(--text); }
  .preview { color: var(--faint); }
  .bad { color: var(--bad); display: inline-flex; }
  .star { color: #e0a000; display: inline-flex; }
  .star :global(svg) { fill: currentColor; }
  .clip { color: var(--faint); display: inline-flex; }
  .more { padding: 14px; text-align: center; }
</style>

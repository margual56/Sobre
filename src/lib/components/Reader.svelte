<script lang="ts">
  import { BellOff, Download, ExternalLink, ImageOff, Paperclip, TriangleAlert } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { app, ask, attempt, toast } from "$lib/app.svelte";
  import { fileSize, longDate, senderLabel } from "$lib/format";
  import AuthBadge from "./AuthBadge.svelte";
  import Avatar from "./Avatar.svelte";

  type View = "auto" | "plain" | "source";
  let view = $state<View>("auto");
  let images = $state(false);

  const detail = $derived(app.detail);
  // Mail you sent never went through inbound checks; there is no verdict to show.
  const outgoing = $derived(detail?.row.folder_role === "sent" || detail?.row.folder_role === "drafts");

  // A different message starts from its own defaults.
  let shownId = -1;
  $effect(() => {
    if (detail && detail.row.id !== shownId) {
      shownId = detail.row.id;
      view = "auto";
      images = detail.images_trusted;
    }
  });

  const src = $derived(detail ? `${detail.body_url}?view=${view}&images=${images ? 1 : 0}&theme=${app.dark ? "dark" : "light"}` : "");
  const kindLabel = $derived(detail ? { html: "HTML", markdown: "Markdown", plain: "Text" }[detail.kind] : "");
  const linkHost = $derived.by(() => {
    try {
      return app.link ? new URL(app.link).host || app.link : "";
    } catch {
      return "";
    }
  });
  const linkSuspicious = $derived(!!app.link && !!detail?.deceptive_links.some((l) => l === app.link || app.link!.startsWith(l)));

  let unsubscribing = $state(false);
  async function unsubscribe() {
    const offer = detail?.unsubscribe;
    if (!detail || !offer) return;
    const question = {
      one_click: `Unsubscribe from this sender? Sobre will send the request to ${offer.target}.`,
      mail: `Unsubscribe from this sender? Sobre will send an email from your address to ${offer.target}.`,
      link: `This sender only offers an unsubscribe page at ${offer.target}. Open it in your browser?`,
    }[offer.method];
    if (!(await ask(question, offer.method === "link" ? "Open page" : "Unsubscribe"))) return;
    unsubscribing = true;
    await attempt(async () => {
      const url = await api.unsubscribe(detail.row.id);
      if (url) await api.openLink(url);
      else toast(`Unsubscribe request sent to ${offer.target}.`);
    });
    unsubscribing = false;
  }
  async function alwaysImages() {
    if (!detail) return;
    await attempt(() => api.trustImages(detail.row.from_addr, true));
    detail.images_trusted = true;
    images = true;
  }
  async function save(index: number) {
    const path = await attempt(() => api.saveAttachment(detail!.row.id, index));
    if (path) toast(`Saved to ${path}`);
  }
  function people(list: { name: string; addr: string }[]) {
    return list.map((c) => (c.name ? `${c.name} <${c.addr}>` : c.addr)).join(", ");
  }
</script>

<section class="reader">
  {#if !detail}
    <div class="blank">
      {#if app.detailError}<p class="error selectable">{app.detailError}</p>
      {:else if app.loadingDetail}<p>Loading…</p>
      {:else}<p>Select a message to read it.</p>{/if}
    </div>
  {:else}
    <header class="head">
      <h1 class="selectable">{detail.row.subject || "(no subject)"}</h1>
      <div class="from">
        <Avatar name={detail.row.from_name} addr={detail.row.from_addr} verified={detail.auth?.verdict === "verified"} size={42} />
        <div class="who selectable">
          <div><b>{senderLabel(detail.row.from_name, detail.row.from_addr)}</b> <span class="muted">&lt;{detail.row.from_addr}&gt;</span></div>
          <div class="muted small">To {people(detail.extra.to) || "undisclosed recipients"}{#if detail.extra.cc.length} · Cc {people(detail.extra.cc)}{/if}</div>
          <div class="muted small">{longDate(detail.row.date)}</div>
        </div>
        <div class="side">
          {#if !outgoing}<AuthBadge auth={detail.auth} firstTime={detail.first_time_sender} />{/if}
          {#if detail.unsubscribe}
            <button class="btn small" disabled={unsubscribing} onclick={unsubscribe} title="Stop receiving mail from this list"><BellOff size={14} />{unsubscribing ? "Unsubscribing…" : "Unsubscribe"}</button>
          {/if}
          <div class="views">
            <button class:on={view === "auto"} onclick={() => (view = "auto")}>{kindLabel}</button>
            {#if detail.kind !== "plain"}<button class:on={view === "plain"} onclick={() => (view = "plain")}>Plain</button>{/if}
            <button class:on={view === "source"} onclick={() => (view = "source")}>Source</button>
          </div>
        </div>
      </div>

      {#if detail.auth?.verdict === "failed"}
        <div class="note bad"><TriangleAlert size={16} /><span><b>This message may be forged.</b> {detail.auth.summary} Do not follow its links or open its attachments unless you can confirm it another way.</span></div>
      {/if}
      {#each detail.auth?.warnings ?? [] as warning}
        <div class="note warn"><TriangleAlert size={16} /><span>{warning}</span></div>
      {/each}
      {#if detail.deceptive_links.length}
        <div class="note warn"><TriangleAlert size={16} /><span>This message has {detail.deceptive_links.length === 1 ? "a link" : "links"} whose text names a different site than the one it opens.</span></div>
      {/if}
      {#if detail.blocked_images > 0 && !images && view === "auto"}
        <div class="note">
          <ImageOff size={16} /><span>Remote images are hidden so the sender cannot tell that you opened this.</span>
          <button class="btn small" onclick={() => (images = true)}>Show images</button>
          <button class="btn small" onclick={alwaysImages}>Always for {detail.row.from_addr}</button>
        </div>
      {/if}
      {#if detail.attachments.length}
        <div class="attachments">
          {#each detail.attachments as a}
            <span class="chip">
              <Paperclip size={14} />
              <button class="name" title="Open with the default program" onclick={() => attempt(() => api.openAttachment(detail.row.id, a.index))}>{a.name}</button>
              <span class="muted">{fileSize(a.size)}</span>
              <button class="icon" title="Save as…" onclick={() => save(a.index)}><Download size={14} /></button>
            </span>
          {/each}
        </div>
      {/if}
    </header>

    {#key src}
      <iframe class="body" title="Message" sandbox="" {src} referrerpolicy="no-referrer"></iframe>
    {/key}

    {#if app.link}
      <footer class="link" class:suspicious={linkSuspicious}>
        {#if linkSuspicious}<TriangleAlert size={16} />{:else}<ExternalLink size={16} />{/if}
        <span class="url selectable">
          {#if linkSuspicious}<b>The link text names another site.</b> It really opens{:else}Open{/if}
          <b>{linkHost}</b><span class="muted"> · {app.link}</span>
        </span>
        <button class="btn small" onclick={() => { void navigator.clipboard.writeText(app.link!); app.link = null; }}>Copy</button>
        <button class="btn small" onclick={() => (app.link = null)}>Cancel</button>
        <button class="btn small primary" onclick={() => { const url = app.link!; app.link = null; void attempt(() => api.openLink(url)); }}>Open in browser</button>
      </footer>
    {/if}
  {/if}
</section>

<style>
  .reader { height: 100%; display: flex; flex-direction: column; min-width: 0; background: var(--panel); }
  .blank { margin: auto; color: var(--muted); padding: 24px; text-align: center; }
  .head { padding: 16px 20px 10px; border-bottom: 1px solid var(--line); }
  h1 { font-size: 19px; margin: 0 0 12px; line-height: 1.3; overflow-wrap: anywhere; }
  .from { display: flex; gap: 12px; align-items: flex-start; }
  .who { min-width: 0; flex: 1; overflow-wrap: anywhere; }
  .small { font-size: 12.5px; }
  .side { display: flex; flex-direction: column; align-items: flex-end; gap: 8px; flex: none; }
  .views { display: flex; border: 1px solid var(--line); border-radius: 7px; overflow: hidden; }
  .views button { border: 0; background: none; padding: 3px 9px; font-size: 12px; color: var(--muted); }
  .views button + button { border-left: 1px solid var(--line); }
  .views button.on { background: var(--hover); color: var(--text); font-weight: 600; }
  .note {
    display: flex; align-items: center; gap: 9px; margin-top: 10px; padding: 8px 11px; border-radius: 8px;
    background: var(--hover); font-size: 13px;
  }
  .note span { flex: 1; }
  .note :global(svg) { flex: none; }
  .note.bad { background: var(--bad-soft); color: var(--bad); }
  .note.warn { background: var(--warn-soft); color: var(--warn); }
  .attachments { display: flex; flex-wrap: wrap; gap: 7px; margin-top: 10px; }
  .chip { display: inline-flex; align-items: center; gap: 6px; border: 1px solid var(--line); border-radius: 8px; padding: 4px 6px 4px 9px; font-size: 13px; }
  .chip .name { border: 0; background: none; padding: 0; max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chip .name:hover { text-decoration: underline; }
  .chip .icon { border: 0; background: none; display: grid; place-items: center; width: 24px; height: 24px; border-radius: 5px; color: var(--muted); }
  .chip .icon:hover { background: var(--hover); }
  .body { flex: 1; width: 100%; border: 0; background: #fff; min-height: 0; }
  .link { display: flex; align-items: center; gap: 9px; padding: 9px 14px; border-top: 1px solid var(--line); background: var(--raised); font-size: 13px; }
  .link.suspicious { background: var(--bad-soft); color: var(--bad); }
  .link :global(svg) { flex: none; }
  .url { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>

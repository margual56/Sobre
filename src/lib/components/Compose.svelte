<script lang="ts">
  import { ExternalLink, Paperclip, Send, X } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { app, ask, type ComposeInit } from "$lib/app.svelte";
  import { untrack } from "svelte";
  import { errorText } from "$lib/format";
  import RecipientInput from "./RecipientInput.svelte";

  // `standalone` is the compose window: it fills the window and closes it when done.
  let props: { init: ComposeInit; standalone?: boolean; onclose: () => void } = $props();
  const standalone = untrack(() => props.standalone ?? false);
  // The parent re-creates this component per draft, so the first value is the only one.
  const init = untrack(() => props.init);

  let accountId = $state(init.accountId ?? app.accounts[0]?.id ?? 0);
  // Addresses arrive as one string; "Name <address>" keeps its spaces.
  const list = (text: string | undefined) => (text ?? "").split(/[,;\n]/).map((x) => x.trim()).filter(Boolean);
  let to = $state(list(init.to));
  let cc = $state(list(init.cc));
  let bcc = $state(list(init.bcc));
  let showCopies = $state(init.cc !== "" || !!init.bcc);
  let subject = $state(init.subject);
  let body = $state(init.body);
  let markdown = $state(init.markdown ?? false);
  let files = $state<string[]>(init.files ?? []);
  let sending = $state(false);
  let error = $state("");
  let bodyBox = $state<HTMLTextAreaElement>();

  $effect(() => {
    // Replies start typing above the quote; new mail starts at the recipient.
    if (init.to && bodyBox) {
      bodyBox.focus();
      bodyBox.setSelectionRange(0, 0);
    }
  });

  const dirty = () => body.trim() !== init.body.trim() || to.join() !== list(init.to).join() || subject !== init.subject || files.length > 0;

  async function close() {
    if (sending) return;
    if (dirty() && !(await ask("Discard this message?", "Discard"))) return;
    props.onclose();
  }

  /** Continue this draft in a window of its own. */
  async function popOut() {
    try {
      await api.openComposeWindow({ ...init, accountId, to: to.join(", "), cc: cc.join(", "), bcc: bcc.join(", "), subject, body, markdown, files: $state.snapshot(files) });
      props.onclose();
    } catch (e) {
      error = errorText(e);
    }
  }

  async function attach() {
    try {
      files = [...new Set([...files, ...(await api.pickFiles())])];
    } catch (e) {
      error = errorText(e);
    }
  }

  async function send() {
    error = "";
    if (to.length + cc.length + bcc.length === 0) return void (error = "Add at least one recipient.");
    if (!subject.trim() && !(await ask("Send without a subject?", "Send"))) return;
    sending = true;
    try {
      await api.send({
        account_id: accountId,
        to: $state.snapshot(to),
        cc: $state.snapshot(cc),
        bcc: $state.snapshot(bcc),
        subject,
        body,
        markdown,
        in_reply_to: init.inReplyTo,
        references: init.references,
        attachments: files,
        reply_to_id: init.replyToId,
      });
      props.onclose();
    } catch (e) {
      error = errorText(e);
    } finally {
      sending = false;
    }
  }

  function keys(e: KeyboardEvent) {
    if (app.question) return;
    if (e.key === "Escape" && !standalone) void close();
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) void send();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class={standalone ? "window" : "overlay"} onkeydown={keys}>
  <div class="compose" class:standalone>
    {#if !standalone}
      <header>
        <b>{init.replyToId ? "Reply" : init.subject.startsWith("Fwd:") ? "Forward" : "New message"}</b>
        <span class="row">
          <button class="x" title="Open in a new window" onclick={popOut}><ExternalLink size={16} /></button>
          <button class="x" title="Close (Esc)" onclick={close}><X size={18} /></button>
        </span>
      </header>
    {/if}
    <div class="line">
      <span>From</span>
      <select bind:value={accountId}>
        {#each app.accounts as a}<option value={a.id}>{a.display_name ? `${a.display_name} <${a.email}>` : a.email}</option>{/each}
      </select>
    </div>
    <div class="line">
      <span>To</span>
      <!-- svelte-ignore a11y_autofocus -->
      <RecipientInput bind:value={to} autofocus={!init.to} placeholder="name@example.com" />
      {#if !showCopies}<button class="link" onclick={() => (showCopies = true)}>Cc/Bcc</button>{/if}
    </div>
    {#if showCopies}
      <div class="line"><span>Cc</span><RecipientInput bind:value={cc} /></div>
      <div class="line"><span>Bcc</span><RecipientInput bind:value={bcc} /></div>
    {/if}
    <div class="line"><span>Subject</span><input bind:value={subject} /></div>
    <textarea bind:this={bodyBox} bind:value={body} placeholder="Write your message…" spellcheck="true"></textarea>
    {#if files.length}
      <div class="files">
        {#each files as f}
          <span class="chip">{f.split("/").pop()}<button title="Remove" onclick={() => (files = files.filter((x) => x !== f))}><X size={12} /></button></span>
        {/each}
      </div>
    {/if}
    {#if error}<p class="error selectable">{error}</p>{/if}
    <footer>
      <button class="btn primary" disabled={sending || !accountId} onclick={send}><Send size={15} />{sending ? "Sending…" : "Send"}</button>
      <button class="btn" onclick={attach}><Paperclip size={15} />Attach</button>
      <label class="md" title="Also sends a formatted HTML version rendered from the markdown you typed">
        <input type="checkbox" bind:checked={markdown} /> Format as markdown
      </label>
      <span class="muted hint">Ctrl+Enter to send</span>
    </footer>
  </div>
</div>

<style>
  .compose {
    background: var(--panel); border: 1px solid var(--line); border-radius: 12px; box-shadow: var(--shadow);
    width: min(760px, calc(100vw - 32px)); height: min(640px, calc(100vh - 48px)); display: flex; flex-direction: column;
  }
  .window { height: 100vh; }
  .compose.standalone { width: 100%; height: 100%; border: 0; border-radius: 0; box-shadow: none; }
  header .row { gap: 2px; }
  header { display: flex; align-items: center; justify-content: space-between; padding: 10px 10px 10px 16px; border-bottom: 1px solid var(--line); }
  .x { border: 0; background: none; border-radius: 6px; width: 30px; height: 30px; display: grid; place-items: center; }
  .x:hover { background: var(--hover); }
  .line { display: flex; align-items: center; min-height: 39px; gap: 8px; padding: 0 16px; border-bottom: 1px solid var(--line); }
  .line > span { width: 56px; color: var(--muted); font-size: 13px; flex: none; }
  .line > input, .line select { flex: 1; border: 0; background: none; padding: 9px 0; border-radius: 0; min-width: 0; }
  .link { border: 0; background: none; color: var(--accent); font-size: 12.5px; }
  textarea { flex: 1; border: 0; border-radius: 0; resize: none; padding: 14px 16px; line-height: 1.5; min-height: 0; background: none; }
  .files { display: flex; flex-wrap: wrap; gap: 6px; padding: 8px 16px 0; }
  .chip { display: inline-flex; align-items: center; gap: 5px; border: 1px solid var(--line); border-radius: 7px; padding: 3px 5px 3px 9px; font-size: 13px; }
  .chip button { border: 0; background: none; display: grid; place-items: center; padding: 2px; border-radius: 4px; }
  .chip button:hover { background: var(--hover); }
  .error { padding: 0 16px; }
  footer { display: flex; align-items: center; gap: 10px; padding: 12px 16px; border-top: 1px solid var(--line); }
  .md { display: flex; align-items: center; gap: 6px; font-size: 13px; }
  .hint { margin-left: auto; font-size: 12px; }
</style>

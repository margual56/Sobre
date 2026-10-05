<script lang="ts">
  import { X } from "@lucide/svelte";

  let { value = $bindable([]), autofocus = false, placeholder = "" }: { value: string[]; autofocus?: boolean; placeholder?: string } = $props();

  let text = $state("");
  let input = $state<HTMLInputElement>();

  export const looksValid = (addr: string) => /^[^\s@<>,;]+@[^\s@<>,;]+\.[^\s@<>,;]+$/.test(addr);

  function add(addresses: string[]) {
    const fresh = addresses.map((a) => a.trim()).filter((a) => a && !value.some((v) => v.toLowerCase() === a.toLowerCase()));
    if (fresh.length) value = [...value, ...fresh];
  }

  /** Turn whatever is typed so far into a bubble. */
  function commit() {
    add([text.replace(/[,;\s]+$/, "")]);
    text = "";
  }

  function keydown(e: KeyboardEvent) {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      // Sending: keep the half-typed address, then let the form handle the key.
      commit();
    } else if ([",", ";", " ", "Enter"].includes(e.key)) {
      e.preventDefault();
      commit();
    } else if (e.key === "Tab" && text.trim()) {
      commit();
    } else if (e.key === "Backspace" && text === "" && value.length) {
      // Pull the last bubble back into the field to correct it.
      e.preventDefault();
      text = value[value.length - 1];
      value = value.slice(0, -1);
    }
  }

  function paste(e: ClipboardEvent) {
    const pasted = e.clipboardData?.getData("text") ?? "";
    // Handles lists and "Name <address>" forms copied from elsewhere.
    const found = pasted.match(/[^\s<>,;"']+@[^\s<>,;"']+/g);
    if (found && (found.length > 1 || /[\s,;<]/.test(pasted.trim()))) {
      e.preventDefault();
      add(found);
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="recipients" onclick={() => input?.focus()}>
  {#each value as addr (addr)}
    <span class="bubble" class:invalid={!looksValid(addr)} title={looksValid(addr) ? addr : "This does not look like an email address"}>
      {addr}
      <button type="button" title="Remove" tabindex="-1" onclick={(e) => { e.stopPropagation(); value = value.filter((v) => v !== addr); }}><X size={12} /></button>
    </span>
  {/each}
  <!-- svelte-ignore a11y_autofocus -->
  <input bind:this={input} bind:value={text} {autofocus} placeholder={value.length ? "" : placeholder} spellcheck="false" autocomplete="off" onkeydown={keydown} onpaste={paste} onblur={commit} />
</div>

<style>
  .recipients { flex: 1; min-width: 0; display: flex; flex-wrap: wrap; align-items: center; gap: 5px; padding: 6px 0; cursor: text; }
  .bubble {
    display: inline-flex; align-items: center; gap: 3px; background: var(--accent-soft); color: var(--text);
    border-radius: 99px; padding: 2px 4px 2px 10px; font-size: 13px; max-width: 100%; overflow-wrap: anywhere;
    user-select: text; -webkit-user-select: text;
  }
  .bubble.invalid { background: var(--bad-soft); color: var(--bad); }
  .bubble button { border: 0; background: none; display: grid; place-items: center; width: 18px; height: 18px; border-radius: 50%; color: inherit; opacity: 0.7; }
  .bubble button:hover { background: rgb(128 128 128 / 0.25); opacity: 1; }
  input { flex: 1; min-width: 140px; border: 0; background: none; padding: 3px 0; border-radius: 0; }
</style>

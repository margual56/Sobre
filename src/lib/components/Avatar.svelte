<script lang="ts">
  import { api } from "$lib/api";
  import { hueFor, initials } from "$lib/format";
  import { domainOf, registrable, serviceIcon } from "$lib/services";

  let { name, addr, verified = false, size = 36 }: { name: string; addr: string; verified?: boolean; size?: number } = $props();

  // One lookup per domain per session; the backend caches across sessions.
  const cache: Map<string, Promise<string | null>> = ((globalThis as any).__iconCache ??= new Map());

  const brand = $derived(serviceIcon(addr));
  let fetched = $state<string | null>(null);

  $effect(() => {
    fetched = null;
    const domain = registrable(domainOf(addr));
    if (brand || !domain) return;
    const key = `${domain}|${verified}`;
    if (!cache.has(key)) cache.set(key, api.senderIcon(domain, verified).catch(() => null));
    let current = true;
    cache.get(key)!.then((url) => current && (fetched = url));
    return () => (current = false);
  });
</script>

<span class="avatar" style:width="{size}px" style:height="{size}px" style:font-size="{size * 0.38}px">
  {#if brand}
    <span class="brand">
      <svg viewBox="0 0 24 24" width={size * 0.56} height={size * 0.56} aria-hidden="true">
        <path d={brand.path} fill={brand.hex === "000000" || brand.hex === "FFFFFF" ? "currentColor" : `#${brand.hex}`} />
      </svg>
    </span>
  {:else if fetched}
    <span class="brand"><img src={fetched} alt="" width={size * 0.62} height={size * 0.62} /></span>
  {:else}
    <span class="letters" style:background="hsl({hueFor(addr || name)} 45% 46%)">{initials(name, addr)}</span>
  {/if}
</span>

<style>
  .avatar { flex: none; display: inline-grid; border-radius: 50%; overflow: hidden; }
  .brand, .letters { display: grid; place-items: center; width: 100%; height: 100%; }
  .brand { background: var(--hover); border: 1px solid var(--line); border-radius: 50%; }
  .brand img { object-fit: contain; border-radius: 3px; }
  .letters { color: #fff; font-weight: 600; }
</style>

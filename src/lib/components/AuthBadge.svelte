<script lang="ts">
  import { ShieldCheck, ShieldQuestion, ShieldX } from "@lucide/svelte";
  import type { AuthReport } from "$lib/api";

  let { auth, firstTime }: { auth: AuthReport | null; firstTime: boolean } = $props();
  let open = $state(false);

  const label = $derived(!auth ? "Checking…" : auth.verdict === "verified" ? "Verified sender" : auth.verdict === "failed" ? "Failed verification" : "Unverified");
</script>

<span class="wrap">
  <button class="badge {auth?.verdict ?? 'pending'}" onclick={() => (open = !open)} title="Sender authenticity">
    {#if auth?.verdict === "verified"}<ShieldCheck size={14} />{:else if auth?.verdict === "failed"}<ShieldX size={14} />{:else}<ShieldQuestion size={14} />{/if}
    {label}
  </button>
  {#if open}
    <button class="backdrop" aria-label="Close" onclick={() => (open = false)}></button>
    <div class="pop selectable">
      {#if !auth}
        <p>The signature check has not finished yet.</p>
      {:else}
        <p class="summary">{auth.summary}</p>
        <table>
          <tbody>
            <tr><th>From domain</th><td>{auth.from_domain || "(none)"}</td></tr>
            {#each auth.dkim as d}
              <tr>
                <th>DKIM signature</th>
                <td>
                  <span class="res {d.result}">{d.result}</span> {d.domain}
                  {#if !d.aligned}<span class="muted"> (not the From domain)</span>{/if}
                  {#if d.detail}<div class="muted">{d.detail}</div>{/if}
                </td>
              </tr>
            {:else}
              <tr><th>DKIM signature</th><td class="muted">none</td></tr>
            {/each}
            <tr><th>Domain policy (DMARC)</th><td>{auth.dmarc_policy ?? "not published"}</td></tr>
            {#if auth.provider}
              <tr>
                <th>Your provider says</th>
                <td>SPF {auth.provider.spf ?? "n/a"} · DKIM {auth.provider.dkim ?? "n/a"} · DMARC {auth.provider.dmarc ?? "n/a"}<div class="muted">{auth.provider.authserv_id}</div></td>
              </tr>
            {/if}
          </tbody>
        </table>
        {#if firstTime}<p class="muted">First message you have received from this address.</p>{/if}
      {/if}
    </div>
  {/if}
</span>

<style>
  .wrap { position: relative; display: inline-block; }
  .badge {
    display: inline-flex; align-items: center; gap: 5px; border: 0; border-radius: 99px; padding: 3px 10px 3px 8px;
    font-size: 12.5px; background: var(--hover); color: var(--muted);
  }
  .badge.verified { background: var(--ok-soft); color: var(--ok); }
  .badge.failed { background: var(--bad-soft); color: var(--bad); }
  .backdrop { position: fixed; inset: 0; background: none; border: 0; z-index: 9; cursor: default; }
  .pop {
    position: absolute; z-index: 10; top: 30px; right: 0; width: 420px; background: var(--raised);
    border: 1px solid var(--line); border-radius: 10px; box-shadow: var(--shadow); padding: 14px; font-size: 13px;
  }
  .summary { margin: 0 0 10px; font-weight: 600; }
  p { margin: 8px 0 0; }
  table { border-collapse: collapse; width: 100%; }
  th { text-align: left; font-weight: 500; color: var(--muted); padding: 4px 12px 4px 0; vertical-align: top; white-space: nowrap; }
  td { padding: 4px 0; overflow-wrap: anywhere; }
  .res { font-weight: 600; }
  .res.pass { color: var(--ok); }
  .res.fail { color: var(--bad); }
  .res.error { color: var(--warn); }
</style>

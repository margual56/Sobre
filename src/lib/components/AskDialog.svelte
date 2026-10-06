<script lang="ts">
  import { app } from "$lib/app.svelte";
</script>

{#if app.question}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="overlay top" onkeydown={(e) => e.key === "Escape" && app.question?.answer(false)}>
    <div class="modal ask">
      <p class="selectable">{app.question.text}</p>
      <div class="row end">
        <button class="btn" onclick={() => app.question?.answer(false)}>Cancel</button>
        <!-- svelte-ignore a11y_autofocus -->
        <button class="btn primary" autofocus onclick={() => app.question?.answer(true)}>{app.question.ok}</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .top { z-index: 40; }
  .ask { width: min(420px, calc(100vw - 32px)); }
  .ask p { margin: 0 0 18px; line-height: 1.5; }
  .end { justify-content: flex-end; }
</style>

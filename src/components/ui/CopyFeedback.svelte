<script lang="ts">
  import { Check } from 'lucide-svelte';
  let { label = '复制', copied = false }: { label?: string; copied?: boolean } = $props();
</script>

<span class="copy-feedback" data-copy-feedback data-copied={copied} role="status" aria-live="polite" aria-atomic="true">
  <span class="copy-label" class:concealed={copied} aria-hidden={copied}>{label}</span>
  <span class="copy-success" class:revealed={copied} aria-hidden={!copied}><Check size={12} />已复制</span>
</span>

<style>
  .copy-feedback { display: inline-grid; vertical-align: middle; }
  .copy-label, .copy-success { grid-area: 1 / 1; white-space: nowrap; justify-self: center; align-self: center; }
  .copy-label.concealed { visibility: hidden; }
  .copy-success { display: inline-flex; align-items: center; gap: 3px; visibility: hidden; color: var(--primary); }
  .copy-success.revealed { visibility: visible; animation: copy-confirm 140ms ease-out; }
  @keyframes copy-confirm { from { opacity: 0; } to { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) {
    .copy-success.revealed { animation: none; }
  }
</style>

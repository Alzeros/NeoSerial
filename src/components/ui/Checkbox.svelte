<script lang="ts">
  let {
    checked = $bindable(false),
    label,
    disabled = false,
    onchange,
  }: {
    checked?: boolean;
    label?: string;
    disabled?: boolean;
    onchange?: (val: boolean) => void;
  } = $props();
</script>

<label class="flex items-center gap-2 text-sm text-foreground cursor-pointer select-none">
  <input
    type="checkbox"
    class="h-4 w-4 rounded border border-input accent-primary"
    bind:checked
    {disabled}
    onchange={() => onchange?.(checked)}
  />
  {#if label}
    <span>{label}</span>
  {/if}
</label>

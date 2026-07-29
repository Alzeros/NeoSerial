<script lang="ts">
  let {
    value = $bindable(''),
    options,
    disabled = false,
    class: className = '',
    onchange,
  }: {
    value?: string;
    options: { label: string; value: string }[];
    disabled?: boolean;
    class?: string;
    onchange?: (val: string) => void;
  } = $props();
</script>

<select
  class="flex h-8 w-full rounded border border-input bg-background px-2 text-sm text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 {className}"
  bind:value
  {disabled}
  onchange={() => onchange?.(value)}
>
  {#each options as opt}
    <option value={opt.value}>{opt.label}</option>
  {/each}
</select>

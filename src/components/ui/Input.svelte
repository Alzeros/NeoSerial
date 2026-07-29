<script lang="ts">
  let {
    value = $bindable(''),
    placeholder = '',
    type = 'text',
    disabled = false,
    readonly = false,
    class: className = '',
    onkeydown,
  }: {
    value?: string;
    placeholder?: string;
    type?: string;
    disabled?: boolean;
    readonly?: boolean;
    class?: string;
    onkeydown?: (e: KeyboardEvent) => void;
  } = $props();
</script>

<input
  class="flex h-8 w-full rounded border border-input bg-background px-2.5 text-sm text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 {className}"
  bind:value
  {type}
  {placeholder}
  {disabled}
  {readonly}
  {onkeydown}
/>

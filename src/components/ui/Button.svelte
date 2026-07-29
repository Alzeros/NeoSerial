<script lang="ts">
  let {
    variant = 'default',
    size = 'default',
    disabled = false,
    onclick,
    children,
  }: {
    variant?: 'default' | 'primary' | 'destructive' | 'ghost' | 'outline';
    size?: 'default' | 'sm' | 'lg' | 'icon';
    disabled?: boolean;
    onclick?: () => void;
    children: any;
  } = $props();

  const base = 'inline-flex items-center justify-center rounded text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50 cursor-pointer';

  const variants = {
    default: 'bg-secondary text-secondary-foreground hover:bg-secondary/80',
    primary: 'bg-primary text-primary-foreground hover:bg-primary/90',
    destructive: 'bg-destructive text-destructive-foreground hover:bg-destructive/90',
    ghost: 'hover:bg-accent hover:text-accent-foreground',
    outline: 'border border-input bg-background hover:bg-accent hover:text-accent-foreground',
  };

  const sizes = {
    default: 'h-8 px-3 py-1.5',
    sm: 'h-7 px-2.5 text-xs',
    lg: 'h-10 px-5',
    icon: 'h-8 w-8',
  };
</script>

<button
  class="{base} {variants[variant]} {sizes[size]}"
  {disabled}
  {onclick}
>
  {@render children?.()}
</button>

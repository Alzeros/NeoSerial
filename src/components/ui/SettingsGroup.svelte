<script lang="ts">
  import type { Snippet } from 'svelte';
  import SettingsHelp from './SettingsHelp.svelte';

  let {
    label,
    help = '',
    variant = 'rows',
    description,
    children,
  }: {
    label: string;
    help?: string;
    /** rows:框里放 Collapsible 行,相邻行之间画实线;padded:框里直接放内容;bare:只有组名,不画框 */
    variant?: 'rows' | 'padded' | 'bare';
    /** 组名下方常驻的一行(如指令联想的刷新状态):组里的行全收起也看得见 */
    description?: Snippet;
    children: Snippet;
  } = $props();
</script>

<!-- 设置页唯一的分组容器:组名在框外,框里是可展开的行或直接放内容。
     层级:组名(12px/600/灰) > 节标题(13px/500) > 选项(13px) > 说明(12px/灰)。 -->
<section data-settings-group class="settings-group">
  <div data-settings-group-label class="settings-group-label">
    {label}{#if help}<SettingsHelp {label} text={help} />{/if}
  </div>
  {#if description}
    <div class="settings-group-description">{@render description()}</div>
  {/if}
  {#if variant === 'bare'}
    {@render children()}
  {:else}
    <div data-settings-group-box class="settings-group-box" class:padded={variant === 'padded'}>
      {@render children()}
    </div>
  {/if}
</section>

<style>
  .settings-group { margin-top: 12px; }
  .settings-group:first-child { margin-top: 0; }
  .settings-group-label {
    display: flex;
    align-items: center;
    gap: 4px;
    margin: 0 2px 6px;
    font-size: 12px;
    font-weight: 600;
    line-height: 16px;
    color: var(--muted-foreground);
  }
  .settings-group-description {
    margin: -2px 2px 6px;
    font-size: 12px;
    line-height: 18px;
    color: var(--muted-foreground);
  }
  .settings-group-box {
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }
  .settings-group-box.padded { padding: 14px 12px; }
  /* rows:相邻两行之间一条实线;{#if} 条件渲染出来的行同样适用 */
  .settings-group-box:not(.padded) > :global(:not(:first-child)) {
    border-top: 1px solid var(--border);
  }
</style>

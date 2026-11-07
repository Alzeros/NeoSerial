<script lang="ts">
  import { onDestroy, tick } from 'svelte';
  import { Replayer } from '@rrweb/replay';
  import '@rrweb/replay/dist/style.css';
  import type { eventWithTime } from '@rrweb/types';
  import { sanitizeReplayEvent } from '$lib/replayPrivacy';

  let { onclose }: { onclose: () => void } = $props();
  type Segment = { window: string; session: string; segment: number; events: eventWithTime[] };
  let segments = $state.raw<Segment[]>([]);
  let selected = $state(0);
  let error = $state('');
  let status = $state('选择导出的 JSON 诊断文件，查看各窗口的界面变化和事件。');
  let logs = $state.raw<{ time: number; kind: string; window: string; detail: string }[]>([]);
  let host: HTMLDivElement;
  let container: HTMLDivElement;
  let player: Replayer | null = null;
  let playing = $state(false);
  let time = $state(0);
  let duration = $state(0);
  let scale = $state(1);
  let frameWidth = $state(800);
  let frameHeight = $state(600);
  let fit = $state(true);
  let fileInput: HTMLInputElement;

  function resize() { scale = fit && container ? Math.min(1, Math.max(0.1, (container.clientWidth - 16) / frameWidth)) : 1; }
  async function chooseFile(event: Event) {
    const file = (event.target as HTMLInputElement).files?.[0];
    if (!file) return;
    error = '';
    try {
      if (file.size > 256 * 1024 * 1024) throw new Error('诊断文件超过 256 MiB，请缩小导出范围');
      const report = JSON.parse(await file.text());
      if (report.schema !== 'neoserial.frontend-diagnostics.v1') throw new Error('不是 NeoSerial 前端诊断文件');
      const raw = report.replay?.segments;
      if (!Array.isArray(raw)) throw new Error('此文件未包含界面回放，可能来自较早版本');
      segments = raw.filter(segment => Array.isArray(segment.events) && segment.events.some((event: eventWithTime) => event.type === 2))
        .map(segment => ({ window: String(segment.window), session: String(segment.session), segment: Number(segment.segment),
          events: segment.events.filter((event: eventWithTime) => Number.isFinite(event.timestamp) && Number.isInteger(event.type) && event.type >= 0 && event.type <= 6).map((event: eventWithTime) => sanitizeReplayEvent(event, true)) }))
        .filter(segment => segment.events.length >= 2).sort((a, b) => a.events[0].timestamp - b.events[0].timestamp);
      logs = (Array.isArray(report.records) ? report.records : []).slice(-2000).map((row: Record<string, any>) => ({
        time: Date.parse(row.record?.ts ?? row.received_at ?? ''), kind: String(row.record?.kind ?? row.kind ?? ''),
        window: String(row.window ?? ''), detail: JSON.stringify(row.record?.data ?? row).slice(0, 1200),
      })).sort((a: {time:number}, b: {time:number}) => a.time - b.time);
      selected = Math.max(0, segments.length - 1);
      const missing = report.replay?.incomplete_segments ?? 0;
      status = `${segments.length} 个界面片段；${logs.length} 条近期事件。${missing ? `有 ${missing} 个片段不完整。` : ''}输入和业务文字已脱敏。`;
      if (!segments.length) throw new Error('没有完整的界面检查点；事件日志仍可查看');
      await openSegment();
    } catch (cause) { error = String(cause instanceof Error ? cause.message : cause); }
  }
  async function openSegment() {
    player?.destroy(); player = null; playing = false; time = 0;
    await tick(); host.replaceChildren();
    const segment = segments[selected];
    if (!segment) return;
    try {
      const meta = segment.events.find(event => event.type === 4);
      frameWidth = meta?.type === 4 ? meta.data.width : 800;
      frameHeight = meta?.type === 4 ? meta.data.height : 600;
      player = new Replayer(segment.events, { root: host, showWarning: false, showDebug: false, mouseTail: false, UNSAFE_replayCanvas: false });
      duration = Math.max(1, player.getMetaData().totalTime);
      player.pause(0); resize();
    } catch (cause) { error = `此片段无法回放：${cause}`; }
  }
  function togglePlay() { if (!player) return; if (playing) player.pause(); else player.play(time); playing = !playing; }
  const timer = setInterval(() => { if (player && playing) { time = Math.min(duration, player.getCurrentTime()); if (time >= duration) playing = false; } }, 100);
  onDestroy(() => { clearInterval(timer); player?.destroy(); });
  const currentEpoch = $derived((segments[selected]?.events[0]?.timestamp ?? 0) + time);
  const nearby = $derived(logs.filter(log => !currentEpoch || Math.abs(log.time - currentEpoch) <= 5000).slice(-30));
</script>

<svelte:window onresize={resize} />
<div class="fixed inset-0 z-[500] flex flex-col p-3 gap-2" role="dialog" aria-label="诊断回放" aria-modal="true" tabindex="-1" data-diagnostic-private data-replay-viewer style="background:var(--background);color:var(--foreground);">
  <div class="flex gap-2 items-center text-[12px]">
    <strong class="mr-auto">诊断回放</strong>
    <input bind:this={fileInput} type="file" accept=".json,application/json" class="hidden" onchange={chooseFile} data-replay-file />
    <button class="btn btn-secondary" onclick={() => fileInput.click()}>打开诊断文件</button>
    <button class="btn btn-secondary" onclick={onclose}>关闭回放</button>
  </div>
  <p class="text-[12px] text-[var(--muted-foreground)]">{status}</p>
  {#if error}<p class="text-[12px] select-text" role="alert" style="color:var(--error)">{error}</p>{/if}
  {#if segments.length}
    <select class="text-[12px] border rounded p-1" style="background:var(--background-input);border-color:var(--border);" value={selected} onchange={event => { selected = Number(event.currentTarget.value); void openSegment(); }} aria-label="回放窗口片段">
      {#each segments as segment, index}<option value={index}>{segment.window} · {new Date(segment.events[0].timestamp).toLocaleString()} · #{segment.segment}</option>{/each}
    </select>
    <div class="flex gap-2 items-center text-[12px]">
      <button class="btn btn-secondary" onclick={togglePlay}>{playing ? '暂停' : '播放'}</button>
      <input type="range" min="0" max={duration} step="10" value={time} oninput={event => { time = Number(event.currentTarget.value); player?.pause(time); playing = false; }} class="flex-1" aria-label="回放进度" />
      <span>{(time / 1000).toFixed(1)} / {(duration / 1000).toFixed(1)}s</span>
      <button class="btn btn-secondary" onclick={() => { fit = !fit; resize(); }}>{fit ? '原始尺寸' : '适应宽度'}</button>
    </div>
  {/if}
  <div bind:this={container} class="flex-1 min-h-0 overflow-auto border rounded" style="border-color:var(--border);background:#eee;">
    <div style:width={`${frameWidth * scale}px`} style:height={`${frameHeight * scale}px`}>
      <div bind:this={host} style:width={`${frameWidth}px`} style:height={`${frameHeight}px`} style:transform={`scale(${scale})`} style="transform-origin:top left;"></div>
    </div>
  </div>
  <div class="max-h-32 overflow-auto border rounded p-2 text-[11px] font-mono select-text" style="border-color:var(--border);">
    {#each nearby as log}<div class="mb-1 break-all">{new Date(log.time).toLocaleTimeString()} [{log.window}] {log.kind} {log.detail}</div>{:else}<span>当前时刻附近没有已保留的事件。</span>{/each}
  </div>
</div>

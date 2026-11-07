import { record } from '@rrweb/record';
import type { eventWithTime } from '@rrweb/types';
import { maskReplayText as mask, maskReplayInput, sanitizeReplayEvent } from './replayPrivacy';
import type { DiagnosticLevel } from './frontendDiagnosticsModel';

type RecordEvent = (kind: string, data?: unknown, level?: DiagnosticLevel, urgent?: boolean) => void;
export function startFrontendReplay(
  session: string,
  write: (segment: number, events: eventWithTime[]) => Promise<void>,
  log: RecordEvent,
) {
  let segment = 0;
  let queue: { segment: number; event: eventWithTime; bytes: number }[] = [];
  let queuedBytes = 0;
  let inFlight: Promise<void> | null = null;
  let stopped = false;
  let checkpointPending = false;
  let lastCheckpoint = 0;
  let errors = 0;
  let writeFailed = false;
  const profile = { build: __NEOSERIAL_BUILD__, user_agent: navigator.userAgent, platform: navigator.platform,
    language: navigator.language, logical_cpus: navigator.hardwareConcurrency, text_policy: 'static_template_labels_only', input_masked: true };
  const checkpoint = () => {
    if (stopped || checkpointPending) return;
    checkpointPending = true;
    const delay = Math.max(0, 1000 - (Date.now() - lastCheckpoint));
    setTimeout(() => {
      checkpointPending = false; lastCheckpoint = Date.now();
      if (!stopped) { try { record.takeFullSnapshot(); } catch (error) { log('replay.error', { error }, 'error', true); } }
    }, delay);
  };
  const stop = record({
    recordAfter: 'DOMContentLoaded',
    checkoutEveryNms: 30000,
    checkoutEveryNth: 1000,
    maskAllInputs: true,
    maskTextSelector: '*',
    maskTextFn: mask,
    maskInputFn: maskReplayInput,
    blockSelector: '[data-diagnostic-private], iframe, canvas, video, audio',
    slimDOMOptions: 'all',
    inlineStylesheet: true,
    recordCanvas: false,
    inlineImages: false,
    collectFonts: false,
    sampling: { mousemove: 100, scroll: 150, input: 'last' },
    errorHandler(error) {
      log('replay.error', { error }, 'error', true);
      if (++errors >= 3) { stopped = true; setTimeout(() => stop?.(), 0); log('replay.disabled', { reason: 'repeated_recorder_errors' }, 'warn', true); }
      return true;
    },
    emit(raw) {
      if (stopped) return;
      if (raw.type === 4) segment++;
      const source = raw.type === 4 ? { ...raw, data: { ...raw.data, diagnostics: profile } } : raw;
      const event = sanitizeReplayEvent(source);
      const bytes = new TextEncoder().encode(JSON.stringify(event)).length;
      if (bytes > 4 * 1024 * 1024) {
        stopped = true; setTimeout(() => stop?.(), 0);
        log('replay.disabled', { reason: 'single_event_limit', bytes }, 'warn', true); return;
      }
      if (queuedBytes + bytes > 8 * 1024 * 1024 || queue.length >= 4000) {
        log('replay.gap', { session, reason: 'queue_limit', events_lost: queue.length + 1, bytes: queuedBytes + bytes }, 'warn', true);
        queue = []; queuedBytes = 0; checkpoint(); return;
      }
      queue.push({ segment, event, bytes }); queuedBytes += bytes;
    },
  });
  log('replay.started', { session, active: Boolean(stop) && !stopped, format: 'rrweb-2.2.1', checkpoint_ms: 30000, mask_inputs: true, text_policy: 'static_template_labels_only', canvas: false, private_regions_blocked: true }, 'info', true);
  const flush = (): Promise<void> => {
    if (inFlight) return inFlight;
    const run = async () => {
      while (queue.length) {
        const part = queue[0].segment;
        const batch: typeof queue = [];
        let bytes = 0;
        while (queue.length && queue[0].segment === part && (bytes < 256 * 1024 || batch.length === 0)) {
          const entry = queue.shift()!; batch.push(entry); bytes += entry.bytes; queuedBytes -= entry.bytes;
        }
        try {
          await write(part, batch.map(entry => entry.event));
          if (writeFailed) { log('replay.write_resumed', { session, segment: part }, 'info', true); writeFailed = false; }
        }
        catch (error) {
          if (!writeFailed) { writeFailed = true; log('replay.write_failed', { session, segment: part, error }, 'error', true); }
          // A bounded segment needs a new self-contained checkpoint; do not retry invalid deltas forever.
          if (String(error).includes('replay_segment_limit') || String(error).includes('replay_partial_segment')) {
            queue = queue.filter(entry => entry.segment !== part);
            queuedBytes = queue.reduce((sum, entry) => sum + entry.bytes, 0);
            log('replay.gap', { session, segment: part, reason: String(error).includes('partial') ? 'partial_write' : 'segment_limit' }, 'warn', true); checkpoint();
          } else {
            if (queuedBytes + bytes > 8 * 1024 * 1024 || queue.length + batch.length > 4000) {
              const lost = batch.length + queue.filter(entry => entry.segment === part).length;
              queue = queue.filter(entry => entry.segment !== part);
              queuedBytes = queue.reduce((sum, entry) => sum + entry.bytes, 0);
              log('replay.gap', { session, segment: part, reason: 'retry_queue_limit', events_lost: lost }, 'warn', true);
              checkpoint();
            } else { queue.unshift(...batch); queuedBytes += bytes; }
            throw error;
          }
        }
      }
    };
    inFlight = run().finally(() => { inFlight = null; }); return inFlight;
  };
  const timer = setInterval(() => { void flush().catch(() => {}); }, 1000);
  return { flush, checkpoint, nodeId(node: Node) { return record.mirror.getId(node); }, stop() { stopped = true; clearInterval(timer); stop?.(); } };
}

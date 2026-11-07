import { diagnosticSummary } from './diagnosticSummary';
import type { DiagnosticLevel } from './frontendDiagnosticsModel';
type RecordEvent = (kind: string, data?: unknown, level?: DiagnosticLevel, urgent?: boolean) => void;

/** New exports automatically enter the state inventory; no per-bug state field list. */
export function observeDiagnosticState(name: string, module: Record<string, unknown>, record: RecordEvent) {
  const previous = new Map<string, string>();
  const sample = () => {
    for (const [field, value] of Object.entries(module)) {
      if (!value || typeof value !== 'object') continue;
      const current = diagnosticSummary(value, 0, field);
      const serialized = JSON.stringify(current);
      if (previous.get(field) !== serialized) {
        record('state.changed', { module: name, field, current, initial: !previous.has(field) });
        previous.set(field, serialized);
      }
    }
  };
  const timer = setInterval(sample, 1000);
  const afterInput = () => setTimeout(sample, 0);
  window.addEventListener('click', afterInput, true);
  window.addEventListener('change', afterInput, true);
  sample();
  return { sample, stop() { clearInterval(timer); window.removeEventListener('click', afterInput, true); window.removeEventListener('change', afterInput, true); } };
}

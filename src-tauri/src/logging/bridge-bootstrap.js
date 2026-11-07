// Runs after Tauri creates its internals object and before core.js defines its
// read-only methods. Preserve those descriptors and restore Object immediately.
;(function () {
  const bridge = window.__TAURI_INTERNALS__;
  const define = Object.defineProperty;
  const descriptor = Object.getOwnPropertyDescriptor(Object, 'defineProperty');
  const remaining = new Set(['invoke', 'transformCallback', 'unregisterCallback']);
  const raw = Object.create(null);
  let client = null;
  let timer;
  const restore = () => {
    if (Object.defineProperty === intercept) define(Object, 'defineProperty', descriptor);
    clearTimeout(timer);
  };
  define(bridge, '__neoserialDiagnostics', { value: Object.freeze({
    raw,
    get ready() { return remaining.size === 0; },
    connect(next) {
      if (remaining.size) throw new Error('Tauri diagnostic bootstrap did not complete');
      for (const key of Object.keys(raw)) if (typeof next[key] !== 'function') throw new Error('Invalid diagnostic bridge');
      client = next;
      return () => { if (client === next) client = null; };
    },
  }) });
  function intercept(target, key, property) {
    if (target !== bridge || !remaining.has(key) || typeof property.value !== 'function') {
      return define(target, key, property);
    }
    const original = property.value;
    const wrapped = function (...args) {
      return Reflect.apply(client ? client[key] : original, client || this, args);
    };
    // Finish the native definition before marking this method as ready.
    const result = define(target, key, { ...property, value: wrapped });
    raw[key] = original.bind(bridge);
    remaining.delete(key);
    if (!remaining.size) { Object.freeze(raw); restore(); }
    return result;
  }
  Object.defineProperty = intercept;
  // A changed/failed Tauri bootstrap must never leave the interception installed.
  timer = setTimeout(restore, 0);
})()

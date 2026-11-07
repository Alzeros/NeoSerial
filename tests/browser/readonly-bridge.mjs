import source from '/src-tauri/src/logging/bridge-bootstrap.js?raw';

/** Model the native bootstrap before locking the SDK mock's methods. */
export function installReadonlyBridge() {
  const bridge=window.__TAURI_INTERNALS__;
  const define=Object.defineProperty;
  new Function(source)();
  for(const key of ['transformCallback','unregisterCallback','invoke']) {
    Object.defineProperty(bridge,key,{value:bridge[key],writable:false,configurable:false});
  }
  if(Object.defineProperty!==define||!bridge.__neoserialDiagnostics.ready) throw new Error('Incomplete diagnostic bootstrap');
  Object.defineProperty(window,'__TAURI_INTERNALS__',{value:bridge,writable:false,configurable:false});
}

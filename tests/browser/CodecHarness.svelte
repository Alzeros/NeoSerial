<script>
  import CodecTool from '/src/components/CodecTool.svelte';
  import { suggestFillRequest, hexSend } from '/src/lib/stores.svelte.ts';
  let config = $state({ operation: 'text_to_hex', input: '', format: 'text' });
  window.codecTest = {
    config: () => JSON.parse(JSON.stringify(config)),
    update: patch => { config = { ...config, ...patch }; },
    fill: () => ({ text: suggestFillRequest.text, hex: hexSend.value }),
  };
</script>
<div style="width:500px;height:620px;display:flex;flex-direction:column;border:1px solid var(--border);">
  <CodecTool {config} onconfigchange={next => { config = next; }} />
</div>

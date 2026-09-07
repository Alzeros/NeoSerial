import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';
import { preloadBeforeMount } from '$lib/startup';

// 先把设置和快捷指令取回来再挂载:首帧直接按用户配置画,不先画默认值再跳一次(见 startup.ts)
preloadBeforeMount().finally(() => {
  mount(App, {
    target: document.getElementById('app')!,
  });
});

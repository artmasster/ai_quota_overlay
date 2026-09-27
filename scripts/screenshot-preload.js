// Stand-in for src/preload.js when taking README screenshots: same bridge, driven by demo data from main.
const { contextBridge, ipcRenderer } = require('electron');

const handlers = {};
const on = (name) => (cb) => { handlers[name] = cb; };
ipcRenderer.on('demo', (_e, name, payload) => handlers[name]?.(payload));
const ignore = () => {};

contextBridge.exposeInMainWorld('overlay', {
  onQuota: on('quota'),
  onLoading: on('loading'),
  onVertical: on('vertical'),
  onSelected: on('selected'),
  onReveal: on('reveal'),
  size: ignore, select: ignore, refresh: ignore, menu: ignore, dragStart: ignore, dragMove: ignore, dragEnd: ignore,
});

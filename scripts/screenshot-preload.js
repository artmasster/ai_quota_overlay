// Stand-in for src/preload.js when taking README screenshots: same bridge, driven by demo data from main.
const { contextBridge, ipcRenderer } = require('electron');

const handlers = {};
const on = (name) => (cb) => { handlers[name] = cb; };
ipcRenderer.on('demo', (_e, name, payload) => handlers[name]?.(payload));
const ignore = () => {};

contextBridge.exposeInMainWorld('overlay', {
  onQuota: on('quota'),
  onLoading: on('loading'),
  onNetwork: on('network'),
  onSettings: on('settings'),
  onSelected: on('selected'),
  onReveal: on('reveal'),
  onUpdate: on('update'),
  size: ignore, select: ignore, refresh: ignore, openSettings: ignore, set: ignore, snap: ignore, restart: ignore, quit: ignore, update: ignore,
  dragStart: ignore, dragMove: ignore, dragEnd: ignore,
});

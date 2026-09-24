// Stand-in for src/preload.js when taking README screenshots: same bridge, driven by demo data from main.
const { contextBridge, ipcRenderer } = require('electron');

const handlers = {};
const on = (name) => (cb) => { handlers[name] = cb; };
ipcRenderer.on('demo', (_e, name, payload) => handlers[name]?.(payload));

contextBridge.exposeInMainWorld('overlay', {
  onQuota: on('quota'),
  onLoading: on('loading'),
  onAnchor: on('anchor'),
  onLayout: on('layout'),
  cardSize() {}, interactive() {}, refresh() {}, menu() {}, dragStart() {}, dragMove() {}, dragEnd() {},
});

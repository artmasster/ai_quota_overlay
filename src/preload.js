const { contextBridge, ipcRenderer } = require('electron');

const on = (channel) => (cb) => ipcRenderer.on(channel, (_e, value) => cb(value));
const send = (channel) => (value) => ipcRenderer.send(channel, value);

contextBridge.exposeInMainWorld('overlay', {
  onQuota: on('quota'),
  onLoading: on('loading'),
  onNetwork: on('network'),
  onSettings: on('settings'),
  onSelected: on('selected'),
  onReveal: on('reveal'),
  onUpdate: on('update'),
  size: send('size'),
  select: send('select'),
  refresh: send('refresh'),
  openSettings: send('open-settings'),
  set: (key, value) => ipcRenderer.send('set', { key, value }),
  snap: send('snap'),
  restart: send('restart'),
  quit: send('quit'),
  update: send('update'),
  dragStart: send('drag-start'),
  dragMove: send('drag-move'),
  dragEnd: send('drag-end'),
});

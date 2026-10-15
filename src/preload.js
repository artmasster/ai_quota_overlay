const { contextBridge, ipcRenderer } = require('electron');

const on = (channel) => (cb) => ipcRenderer.on(channel, (_e, value) => cb(value));
const send = (channel) => (value) => ipcRenderer.send(channel, value);

contextBridge.exposeInMainWorld('overlay', {
  onQuota: on('quota'),
  onLoading: on('loading'),
  onNetwork: on('network'),
  onVertical: on('vertical'),
  onSelected: on('selected'),
  onReveal: on('reveal'),
  size: send('size'),
  select: send('select'),
  refresh: send('refresh'),
  menu: send('menu'),
  dragStart: send('drag-start'),
  dragMove: send('drag-move'),
  dragEnd: send('drag-end'),
});

const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('overlay', {
  onQuota: (cb) => ipcRenderer.on('quota', (_e, data) => cb(data)),
  onLoading: (cb) => ipcRenderer.on('loading', (_e, v) => cb(v)),
  onAnchor: (cb) => ipcRenderer.on('anchor', (_e, a) => cb(a)),
  onLayout: (cb) => ipcRenderer.on('layout', (_e, l) => cb(l)),
  cardSize: (size) => ipcRenderer.send('card-size', size),
  interactive: (on) => ipcRenderer.send('interactive', on),
  refresh: () => ipcRenderer.send('refresh'),
  menu: () => ipcRenderer.send('menu'),
  dragStart: () => ipcRenderer.send('drag-start'),
  dragMove: (d) => ipcRenderer.send('drag-move', d),
  dragEnd: () => ipcRenderer.send('drag-end'),
});

// `npm run check` — print what the overlay would show, without opening a window.
const { fetchAll } = require('./quota');

fetchAll().then((r) => console.log(JSON.stringify(r, null, 2)));

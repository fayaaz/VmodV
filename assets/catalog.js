// Static gallery for the generated VmodV module catalogue.
// Data comes from modules.json; WGSL is fetched from previews.json on demand.

const dataUrl = new URL('../modules/modules.json', import.meta.url);
const previewsUrl = new URL('../modules/previews.json', import.meta.url);

const els = {
  grid: document.getElementById('grid'),
  kinds: document.getElementById('kinds'),
  search: document.getElementById('search'),
  sort: document.getElementById('sort'),
  count: document.getElementById('count'),
  backdrop: document.getElementById('backdrop'),
  detail: document.getElementById('detail'),
  title: document.getElementById('detail-title'),
  badges: document.getElementById('detail-badges'),
  body: document.getElementById('detail-body'),
  close: document.getElementById('close'),
};

const KINDS = [
  ['all', 'All'],
  ['isf', 'ISF'],
  ['shader', 'Shaders'],
  ['point10', 'Point10'],
  ['2d', '2D'],
];

let catalog;
let previews;
let previewsPromise;
let activeStop = null;
let activeToken = 0;
let state = { query: '', kind: 'all', sort: 'name' };

function element(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = String(text);
  return node;
}

function badge(module, extraClass) {
  const node = element('span', `badge ${module.kind}${extraClass ? ` ${extraClass}` : ''}`, module.kind);
  node.dataset.kind = module.kind;
  return node;
}

function description(module) {
  if (module.description) return module.description;
  if (module.kind === 'point10') return 'Ported Point10 scene — its own GPU target, post chain and audio levers.';
  if (module.kind === '2d') return '2D canvas facade module from the built-in module set.';
  if (module.kind === 'shader') return 'Built-in WGSL effect driven by the shared shader uniform block.';
  return 'ISF shader from the bundled corpus.';
}

function matches(module) {
  if (state.kind !== 'all' && module.kind !== state.kind) return false;
  if (!state.query) return true;
  const needle = state.query.toLowerCase();
  return [module.name, module.author, module.category, module.description, module.kind]
    .filter(Boolean)
    .some((value) => String(value).toLowerCase().includes(needle));
}

function render() {
  const modules = catalog.modules.filter(matches);
  modules.sort((a, b) => {
    if (state.sort === 'kind') return a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name);
    if (state.sort === 'category') return a.category.localeCompare(b.category) || a.name.localeCompare(b.name);
    return a.name.localeCompare(b.name);
  });

  els.count.textContent = `${modules.length} of ${catalog.modules.length} modules`;
  const fragment = document.createDocumentFragment();
  for (const module of modules) {
    const card = element('button', 'card');
    card.type = 'button';
    card.setAttribute('aria-label', `Details for ${module.name}`);
    card.append(element('h3', null, module.name));
    card.append(element('p', null, description(module)));

    const meta = element('div', 'card-meta');
    meta.append(badge(module));
    if (module.preview) meta.append(element('span', 'badge live', 'live preview'));
    meta.append(element('span', 'category', module.category));
    card.append(meta);
    card.addEventListener('click', () => openDetail(module));
    fragment.append(card);
  }
  els.grid.replaceChildren(fragment);
}

function buildChips(counts) {
  const fragment = document.createDocumentFragment();
  for (const [kind, label] of KINDS) {
    const count = kind === 'all' ? catalog.modules.length : (counts[kind] || 0);
    const chip = element('button', 'chip', `${label} ${count}`);
    chip.type = 'button';
    chip.setAttribute('aria-pressed', String(state.kind === kind));
    chip.addEventListener('click', () => {
      state.kind = kind;
      for (const other of els.kinds.children) {
        other.setAttribute('aria-pressed', String(other === chip));
      }
      render();
    });
    fragment.append(chip);
  }
  els.kinds.replaceChildren(fragment);
}

function detailField(label, value) {
  const wrapper = element('div');
  wrapper.append(element('dt', null, label));
  wrapper.append(element('dd', null, value || '—'));
  return wrapper;
}

function propsTable(props) {
  const table = element('table', 'props');
  const head = element('thead');
  const row = element('tr');
  for (const label of ['Input', 'Type', 'Default', 'Range']) row.append(element('th', null, label));
  head.append(row);
  table.append(head);
  const body = element('tbody');
  for (const prop of props) {
    const tr = element('tr');
    tr.append(element('td', null, prop.label || prop.name));
    const type = element('td');
    type.append(element('code', null, prop.type));
    tr.append(type);
    const value = element('td');
    value.append(element('code', null, JSON.stringify(prop.default ?? null)));
    tr.append(value);
    const range = prop.min !== null && prop.max !== null
      ? `${prop.min} – ${prop.max}`
      : '—';
    tr.append(element('td', null, range));
    body.append(tr);
  }
  table.append(body);
  return table;
}

async function loadPreviews() {
  if (!previewsPromise) {
    previewsPromise = fetch(previewsUrl)
      .then((response) => {
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        return response.json();
      })
      .catch((error) => {
        previewsPromise = null;
        throw error;
      });
  }
  return previewsPromise;
}

function stopPreview() {
  if (activeStop) {
    try { activeStop(); } catch (error) { console.warn(error); }
    activeStop = null;
  }
}

async function startPreview(module, canvas, status, token) {
  status.textContent = 'Loading WebAssembly runtime and WGSL…';
  try {
    const all = await loadPreviews();
    if (token !== activeToken) return;
    const shader = all[module.id];
    if (!shader) throw new Error('preview data missing');
    const { startPreview } = await import('./preview.js');
    if (token !== activeToken) return;
    const stop = await startPreview({ canvas, module, shader, status });
    if (token !== activeToken) {
      stop();
      return;
    }
    activeStop = stop;
  } catch (error) {
    status.textContent = `Preview unavailable: ${error.message || error}`;
  }
}

function openDetail(module) {
  stopPreview();
  const token = ++activeToken;
  els.title.textContent = module.name;

  const badges = element('div');
  badges.append(badge(module, module.preview ? 'live' : ''));
  if (module.preview) badges.append(element('span', 'badge live', 'live preview'));
  if (module.passes > 1) badges.append(element('span', 'badge', `${module.passes} passes`));
  els.badges.replaceChildren(badges);

  const body = document.createDocumentFragment();

  if (module.preview) {
    const box = element('div', 'preview-box');
    const canvas = element('canvas');
    canvas.setAttribute('aria-label', `Live preview of ${module.name}`);
    const status = element('p', 'preview-status', '');
    box.append(canvas, status);
    body.append(box);
    queueMicrotask(() => startPreview(module, canvas, status, token));
  } else {
    const note = element('p', 'note');
    note.textContent = `${module.kind === '2d' ? '2D canvas modules' : 'Point10 scenes'} render through the native engine and have no portable browser preview. Open the app to see them in motion.`;
    body.append(note);
  }

  const grid = element('dl', 'detail-grid');
  grid.append(detailField('Author', module.author));
  grid.append(detailField('Version', module.version));
  grid.append(detailField('Category', module.category));
  grid.append(detailField('Inputs', module.props.length));
  body.append(grid);

  if (module.description) {
    const p = element('p', null, module.description);
    body.append(p);
  }
  body.append(element('h3', 'section-title', 'Inputs'));
  body.append(propsTable(module.props.length ? module.props : [{ name: '—', label: 'None declared', type: '—', default: null }]));

  const footer = element('div', 'detail-footer');
  const source = module.source
    ? `https://github.com/fayaaz/VmodV/blob/main/${encodeURI(module.source)}`
    : `https://github.com/fayaaz/VmodV/search?q=${encodeURIComponent(module.name)}`;
  const sourceLink = element('a', null, 'View source ↗');
  sourceLink.href = source;
  sourceLink.target = '_blank';
  sourceLink.rel = 'noopener noreferrer';
  const notices = element('a', null, 'Licences ↗');
  notices.href = 'https://github.com/fayaaz/VmodV/blob/main/THIRD_PARTY_NOTICES.md';
  notices.target = '_blank';
  notices.rel = 'noopener noreferrer';
  footer.append(sourceLink, notices);
  body.append(footer);

  els.body.replaceChildren(body);
  els.detail.classList.add('open');
  els.backdrop.classList.add('open');
  document.body.style.overflow = 'hidden';
  els.close.focus();
}

function closeDetail() {
  stopPreview();
  ++activeToken;
  els.detail.classList.remove('open');
  els.backdrop.classList.remove('open');
  document.body.style.overflow = '';
}

els.close.addEventListener('click', closeDetail);
els.backdrop.addEventListener('click', closeDetail);
document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape' && els.detail.classList.contains('open')) closeDetail();
});
els.search.addEventListener('input', (event) => {
  state.query = event.target.value.trim();
  render();
});
els.sort.addEventListener('change', (event) => {
  state.sort = event.target.value;
  render();
});

fetch(dataUrl)
  .then((response) => {
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    return response.json();
  })
  .then((loaded) => {
    catalog = loaded;
    const counts = loaded.modules.reduce((count, module) => {
      count[module.kind] = (count[module.kind] || 0) + 1;
      return count;
    }, {});
    els.search.placeholder = `Search ${loaded.modules.length} modules by name, author, category…`;
    buildChips(counts);
    render();
  })
  .catch((error) => {
    els.grid.append(element('p', 'note', `Could not load the catalogue: ${error.message || error}`));
  });

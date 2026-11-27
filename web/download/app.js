'use strict';

// The page has no build step and no server: it asks GitHub which release is
// current, then links directly to the assets attached to that release. The
// `/releases/latest` URL is the fallback everywhere, so a new version needs no
// edit to this site.
const API_URL = 'https://api.github.com/repos/fayaaz/VmodV/releases/latest';
const LATEST_URL = 'https://github.com/fayaaz/VmodV/releases/latest';
const CACHE_KEY = 'vmodv-download-latest';
const CACHE_TTL_MS = 10 * 60 * 1000; // be polite to the unauthenticated API

// Each slot describes one download button. Only assets present on the latest
// release become direct links; the rest fall back to the releases page.
const LINUX_APPIMAGE = [
  {
    label: 'x86_64 AppImage',
    detail: 'Intel / AMD — most Linux desktops',
    match: name => name.endsWith('-x86_64.AppImage'),
  },
  {
    label: 'aarch64 AppImage',
    detail: 'ARM64 — ARM laptops and single-board computers',
    match: name => name.endsWith('-aarch64.AppImage'),
  },
];

const LINUX_DEB = [
  {
    label: 'Ubuntu 24.04 (.deb)',
    detail: 'Noble — amd64',
    match: name => name.includes('-amd64-ubuntu24.04.deb'),
  },
  {
    label: 'Ubuntu 26.04 (.deb)',
    detail: 'amd64',
    match: name => name.includes('-amd64-ubuntu26.04.deb'),
  },
  {
    label: 'Ubuntu 26.10 (.deb)',
    detail: 'amd64',
    match: name => name.includes('-amd64-ubuntu26.10.deb'),
  },
  {
    label: 'Ubuntu devel (.deb)',
    detail: 'amd64 — development release',
    match: name => name.includes('-amd64-ubuntudevel.deb'),
  },
];

const WINDOWS = [
  {
    label: 'Windows x86_64 portable',
    detail: 'ZIP — unzip and run VmodV.cmd',
    match: name => name.includes('-Windows-x86_64-portable-') && name.endsWith('.zip'),
  },
];

const MACOS = [
  {
    label: 'macOS arm64 portable',
    detail: 'Apple Silicon — ZIP app bundle',
    match: name => name.includes('-macOS-arm64-portable-') && name.endsWith('.zip'),
  },
  {
    label: 'macOS x86_64 portable',
    detail: 'Intel — ZIP app bundle',
    match: name => name.includes('-macOS-x86_64-portable-') && name.endsWith('.zip'),
  },
];

function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return '';
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = bytes;
  let i = 0;
  while (value >= 1024 && i < units.length - 1) {
    value /= 1024;
    i += 1;
  }
  const rounded = value >= 10 || i === 0 ? Math.round(value) : value.toFixed(1);
  return `${rounded} ${units[i]}`;
}

function assetRow(asset, slot) {
  const link = document.createElement('a');
  link.className = asset ? 'asset' : 'asset missing';
  link.href = asset ? asset.browser_download_url || LATEST_URL : LATEST_URL;

  const left = document.createElement('span');
  const label = document.createElement('span');
  label.className = 'asset-label';
  label.textContent = slot.label;
  const detail = document.createElement('span');
  detail.className = 'asset-detail';
  detail.textContent = asset
    ? `${slot.detail} · ${asset.name}`
    : 'Not attached to this release — open all releases';
  left.append(label, detail);

  const right = document.createElement('span');
  right.className = 'asset-right';
  if (asset) {
    const size = document.createElement('span');
    size.className = 'asset-size';
    size.textContent = formatBytes(asset.size);
    right.append(size);
  }
  const arrow = document.createElement('span');
  arrow.className = 'asset-arrow';
  arrow.setAttribute('aria-hidden', 'true');
  arrow.textContent = asset ? '↓' : '↗';
  right.append(arrow);

  link.append(left, right);
  return link;
}

function render(containerId, slots, assets) {
  const container = document.getElementById(containerId);
  container.replaceChildren();
  for (const slot of slots) {
    container.append(assetRow(assets.find(asset => slot.match(asset.name)), slot));
  }
}

function renderAll(assets) {
  render('assets-linux-appimage', LINUX_APPIMAGE, assets);
  render('assets-linux-deb', LINUX_DEB, assets);
  render('assets-windows', WINDOWS, assets);
  render('assets-macos', MACOS, assets);
}

function setStatus(message, error = false) {
  const status = document.getElementById('status');
  status.textContent = message;
  status.classList.toggle('error', error);
}

function showRelease(release) {
  const version = document.getElementById('version');
  version.textContent = release.tag_name || 'latest';
  document.getElementById('release-notes').href = release.html_url || LATEST_URL;

  const published = document.getElementById('published');
  published.textContent = release.published_at
    ? `published ${new Date(release.published_at).toLocaleDateString(undefined, {
        year: 'numeric',
        month: 'short',
        day: 'numeric',
      })}`
    : '';

  const assets = Array.isArray(release.assets) ? release.assets : [];
  renderAll(assets);
  setStatus(
    `${assets.length} build${assets.length === 1 ? '' : 's'} attached to this release. ` +
      'Buttons link straight to the asset files.'
  );
}

function showFailure(error) {
  console.error('Could not load the latest VmodV release:', error);
  document.getElementById('version').textContent = 'latest';
  renderAll([]);
  setStatus(
    `Could not reach the GitHub API (${error.message}). ` +
      'Open the latest release page to download manually.',
    true
  );
}

async function fetchLatest() {
  try {
    const cached = JSON.parse(localStorage.getItem(CACHE_KEY) || 'null');
    if (
      cached &&
      cached.release &&
      Number.isFinite(cached.at) &&
      Date.now() - cached.at < CACHE_TTL_MS
    ) {
      return cached.release;
    }
  } catch (_error) {
    // Private mode / disabled storage: just skip the cache.
  }

  const response = await fetch(API_URL, {
    headers: { Accept: 'application/vnd.github+json' },
  });
  if (!response.ok) throw new Error(`GitHub returned ${response.status}`);
  const release = await response.json();
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify({ at: Date.now(), release }));
  } catch (_error) {
    // Caching is an optimisation, not a requirement.
  }
  return release;
}

function initLinuxTabs() {
  const tabs = Array.from(document.querySelectorAll('[data-linux-tab]'));
  const panelFor = tab => document.getElementById(`panel-${tab.dataset.linuxTab}`);

  function select(selected) {
    for (const tab of tabs) {
      const active = tab === selected;
      tab.classList.toggle('is-active', active);
      tab.setAttribute('aria-selected', String(active));
      panelFor(tab).hidden = !active;
    }
  }

  tabs.forEach((tab, index) => {
    tab.addEventListener('click', () => select(tab));
    tab.addEventListener('keydown', event => {
      if (event.key !== 'ArrowRight' && event.key !== 'ArrowLeft') return;
      event.preventDefault();
      const step = event.key === 'ArrowRight' ? 1 : tabs.length - 1;
      const next = tabs[(index + step) % tabs.length];
      select(next);
      next.focus();
    });
  });
}

initLinuxTabs();
fetchLatest().then(showRelease).catch(showFailure);

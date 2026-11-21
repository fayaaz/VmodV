# Legacy Electron archive

The Electron/Vue implementation was removed from the working tree in the
VmodV 2.0 cleanup. It is preserved locally and in the repository history:

| Ref | Points at |
| --- | --- |
| branch `legacy-electron` | commit `6c74947e`, the complete pre-cleanup Electron tree |
| tag `v1.x-final` | the same commit (`6c74947e`) |
| commit `4a2a4164` | this branch's HEAD before the deletion commits (includes the final asset relocation) |

Do not delete or move those refs; they are the only supported way to retrieve
the old application, its Playwright/Vitest suites and its packaging pipeline.

## Retrieve a copy

Read a file without checking the branch out:

```bash
git show legacy-electron:package.json
git show legacy-electron:src/renderer/src/application/worker/index.worker.js
```

Create a worktree for a local build or investigation:

```bash
git worktree add ../modV-legacy-electron legacy-electron
cd ../modV-legacy-electron
```

The archived tree needs its original toolchain (Docker with the Node/Yarn
image, `make install`, `make dev`). None of that tooling exists in this tree,
and the Electron app is no longer maintained or released.

## What stays in this tree

`resources/isf`, `resources/point10/textures`, the VmodV installer assets and
the Rust ports of the Electron behaviour remain current. The legacy archive is
for reference only; fixes and features land in `crates/`.

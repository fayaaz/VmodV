# MilkDrop presets in VmodV

VmodV can load MilkDrop-style `.mlk` presets as modules, but it does **not**
ship a preset collection of its own. This guide explains where your own files
go, what happens to them, and what you are responsible for when you share a
build or a preset.

## What are MilkDrop presets

MilkDrop presets are plain-text files with a `.mlk` extension. Each file holds
one preset in a `[preset00]` section: `key=value` lines configure warp, wave,
shape and video settings, while `warp=` and `comp=` carry the EEL equations
that animate the visuals. VmodV registers a loaded preset as a `milkdrop`
module and renders it with its own MilkDrop runtime. Because they are ordinary
text files, presets are read from disk when VmodV builds its module registry —
none are baked into the app or the installer.

## Where to put MilkDrop files

Put your `.mlk` files in the `milkdrop` folder of your media tree:

- the shared `<userData>/media/milkdrop` folder, used by every profile;
- the active profile's `<userData>/media/<profile>/milkdrop` folder; or
- any extra MilkDrop folder you have configured.

Folders are scanned recursively and `.mlk` is matched case-insensitively, so
subfolders and names such as `MyPreset.MLK` are found too; hidden and temporary
files are skipped. Only the active profile's folder counts.

`MODV_USER_MILKDROP_DIR` overrides all of those with a single directory, which
is useful for pointing VmodV at an existing collection without copying files
into its media tree. VmodV watches these folders for changes; a burst of edits
is coalesced after **500 ms** of quiet before the presets are reloaded, so a
save is picked up without restarting the app.

## User files are read in place, never redistributed

VmodV reads your MilkDrop presets from the folders above in place. It does not
copy them into the source tree, the AppImage, the `.deb`, or any release
archive. Your collection stays on your machine under your control; only you
decide whether to share a particular preset with someone else.

## Why no presets are bundled

MilkDrop presets are third-party creative works with widely varying licenses —
many are non-commercial and/or share-alike, and the author and terms are not
always clear. Bundling a corpus would mean redistributing those files and
taking on their attribution and license obligations. VmodV therefore ships no
presets at all: the only `.mlk` file in this repository is a test fixture used
to exercise the notices guard, not a preset that is installed for users.

## Your responsibilities

A preset you load keeps its original license. Many MilkDrop presets are
CC BY-NC-SA or otherwise restricted, so check the preset's own header and
source before using it commercially, remixing it, or passing it on. Keep the
original author credit and any share-alike terms intact. The licenses of the
content VmodV *does* bundle, and of the test fixture above, are recorded in
[THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md); user presets are not part
of that bundled set.

## Troubleshooting

- **Preset not listed.** Check that the file ends in `.mlk`, sits in one of the
  watched `milkdrop` folders, and contains a `[preset00]` section — a file with
  no preset block is ignored.
- **Edits not showing up.** The watcher waits for 500 ms of quiet, so pause
  briefly after saving. If you set `MODV_USER_MILKDROP_DIR`, only that folder is
  scanned; unset it (or include your other folders) to see them again.
- **Preset renders incorrectly or not at all.** Not every EEL function or
  MilkDrop extension is supported. A preset that fails to compile is skipped
  for the rest of that session rather than breaking the frame; reloading after
  a fix retries it.
- **License questions.** The license and credit belong to the preset's author,
  not to VmodV. Consult the preset's header/source and
  [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md) before redistributing.

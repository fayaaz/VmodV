# VmodV — Rust-only build entry points.
#
# Every command runs in Docker: the host needs only Docker (with Buildx) and
# make. There is no Node, Yarn or Electron in this tree.
#
# Common targets:
#   make build            VmodV AppImage + .deb into dist/ (clean first)
#   make appimage-vmodv  VmodV AppImage only (runs vmodv-appimage-check.sh)
#   make vmodv-deb       VmodV .deb (runs vmodv-deb-check.sh)
#   make vmodv-deb-check re-check an existing .deb
#   make vmodv-appimage-check re-check an existing AppImage's fat bundle
#   make dev              VmodV shell (release; MODV_UI is gone)
#   make vmodv-debug     VmodV shell, debug profile
#   make engine           release workspace build
#   make engine-test      cargo test (pass ARGS="..." to narrow)
#   make engine-lint      clippy -D warnings + rustfmt --check
#   make test-e2e-vmodv  the real-binary e2e harness and parity tests
#   make shell            interactive shell in the Rust toolchain
#   make docker-package   build both installers via Dockerfile `artifacts`
#   make disk-clean-targets  drop compiled artifacts from the cargo volume

# Local builds keep Cargo's target dir in the modv-cargo Docker volume instead
# of the bind-mounted crates/ tree, which otherwise grows by gigabytes with
# stale build-script outputs. Opt in by setting CARGO_TARGET_DIR in the
# gitignored .env (e.g. /cargo/target); CI leaves it unset so its crates/target
# cache keeps working.
-include .env
CARGO_TARGET_ENV := $(if $(CARGO_TARGET_DIR),-e CARGO_TARGET_DIR="$(CARGO_TARGET_DIR)")

# The version baked into the installers is not kept in the tree. It comes
# from $VMODV_VERSION when set, otherwise from the git tags via
# tools/version.sh: the tag on HEAD when this checkout is tagged, else the
# highest vX.Y.Z tag. `make version` prints the value in effect.
VMODV_VERSION ?= $(shell "$(CURDIR)/tools/version.sh")
# Dev-only asset tooling: the wordcodec is a small Node script run in a
# throwaway container (no Node is needed for the app build/tests).
NODE_IMAGE ?= node:24-slim
DOCKER_BUILD_ARGS ?=
DIST_DIR ?= dist
# Build image base and .deb artifact suffix. The release workflow overrides
# both per Ubuntu leg (--build-arg BASE_IMAGE=ubuntu:..., DEB_SUFFIX=-ubuntu...);
# local builds keep Debian trixie and the unsuffixed artifact.
BASE_IMAGE ?= debian:trixie-slim
DEB_SUFFIX ?=
# appimagetool ships one AppImage per CPU, so derive its token from the host:
# amd64/x86_64 -> x86_64 and arm64/aarch64 -> aarch64. The bundle checker looks
# for the matching native artifact instead of assuming x86_64.
APPIMAGE_ARCH ?= $(shell uname -m | sed -e 's/^amd64$$/x86_64/' -e 's/^arm64$$/aarch64/')

DOCKER ?= docker
RUST_IMAGE ?= modv-rust:local
CARGO_VOLUME ?= modv-cargo
HOST_UID ?= $(shell id -u)
HOST_GID ?= $(shell id -g)
DOCKER_RUN_ARGS ?=
XAUTHORITY ?= $(HOME)/.Xauthority
PULSE_SOCKET ?= $(XDG_RUNTIME_DIR)/pulse/native
WAYLAND_DISPLAY ?= wayland-0
WAYLAND_SOCKET ?= $(XDG_RUNTIME_DIR)/$(WAYLAND_DISPLAY)
# Containers run as the host UID/GID and do not inherit supplementary groups,
# so the GPU device nodes stay unreadable unless their GIDs are added. The
# VmodV shell needs the render node for Vulkan (and falls back to lavapipe
# when no hardware adapter is available).
DRI_GIDS ?= $(shell stat -c '%g' /dev/dri/card0 2>/dev/null) $(shell stat -c '%g' /dev/dri/renderD128 2>/dev/null)
DRI_GROUPS ?= $(foreach gid,$(sort $(DRI_GIDS)),--group-add $(gid))
DRI_ARGS ?= $(if $(wildcard /dev/dri/renderD* /dev/dri/card*),--device /dev/dri $(DRI_GROUPS))

# Prefer a Wayland socket when the host has one, otherwise fall back to X11.
DISPLAY_BACKEND ?= auto
ifeq ($(DISPLAY_BACKEND),auto)
  ifneq ($(wildcard $(WAYLAND_SOCKET)),)
    DISPLAY_BACKEND := wayland
  else
    DISPLAY_BACKEND := x11
  endif
endif

ifeq ($(DISPLAY_BACKEND),wayland)
  GUI_ENV := -e XDG_RUNTIME_DIR="$(XDG_RUNTIME_DIR)" \
	-e WAYLAND_DISPLAY="$(WAYLAND_DISPLAY)" \
	-e XDG_SESSION_TYPE=wayland
  GUI_MOUNTS := -v "$(XDG_RUNTIME_DIR)":"$(XDG_RUNTIME_DIR)"
  GUI_AUDIO :=
else
  GUI_ENV := -e DISPLAY="$(DISPLAY)" -e XDG_SESSION_TYPE=x11
  GUI_MOUNTS := -v /tmp/.X11-unix:/tmp/.X11-unix \
	$(if $(wildcard $(XAUTHORITY)),-e XAUTHORITY=/tmp/.Xauthority -v "$(XAUTHORITY):/tmp/.Xauthority:ro")
  GUI_AUDIO := $(if $(wildcard $(PULSE_SOCKET)),-e PULSE_SERVER=unix:/tmp/pulse-native -v "$(PULSE_SOCKET):/tmp/pulse-native")
endif

GUI_RUN_ARGS := --net=host --shm-size=1g --ulimit core=0 $(DRI_ARGS) $(GUI_ENV) $(GUI_MOUNTS) $(GUI_AUDIO)

# The process harness prefers the host Wayland compositor and falls back to
# X11/XWayland when there is no Wayland socket. Force the fallback with
# `DISPLAY_BACKEND=x11`. With no display at all (CI), the harness starts Xvfb.

# Building the binary and both installers needs several GB of BuildKit cache.
# Fail before a build starts when the filesystem is nearly full instead of
# letting Docker fill the disk.
MIN_FREE_GB ?= 20

.DEFAULT_GOAL := build
.PHONY: build appimage appimage-vmodv vmodv-appimage-check vmodv-deb vmodv-deb-check clean dev vmodv vmodv-debug lint format test-e2e test-e2e-vmodv engine engine-test engine-lint engine-run rave-reencode shell docker-rust docker-cargo-volume docker-package docker-reset disk-guard disk-clean disk-clean-targets version

disk-guard:
	@free=$$(df -Pk "$(CURDIR)" | awk 'NR==2 {print int($$4/1048576)}'); \
	if [ "$$free" -lt "$(MIN_FREE_GB)" ]; then \
		echo "error: only $${free} GB free on the build filesystem (need $(MIN_FREE_GB) GB)."; \
		echo "       free host space first — e.g. 'make disk-clean-targets' (drops compiled artifacts,"; \
		echo "       keeps the registry) or 'make docker-reset' (clears the whole cargo cache volume)."; \
		echo "       'make disk-clean' only drops cache unused for a week; avoid 'docker builder prune -af'."; \
		exit 1; \
	fi

# Keeps the BuildKit cache (image layers only need rebuilding when the
# Dockerfile changes); pruning it forces every `make engine*` to rebuild the
# toolchain image from apt up. Only cache unused for a week is dropped, plus
# dangling images/containers. For a truly cold cache run
# `docker builder prune -af` by hand.
disk-clean:
	-$(DOCKER) builder prune -f --filter until=168h
	-$(DOCKER) image prune -f
	-$(DOCKER) container prune -f

# Drop compiled artifacts from the cargo volume without losing the registry
# cache. The next make target recompiles from scratch but keeps downloaded
# crates; 'make docker-reset' clears the registry too.
#
# Deliberately does not depend on `docker-cargo-volume`/`docker-rust`: those
# need `disk-guard`, and this is the escape hatch the guard tells you to run
# when the disk is already full. The volume is created by the bind below if it
# does not exist, and an existing toolchain image is enough to delete from it.
disk-clean-targets:
	@target="$(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),/cargo/target)"; \
	case "$$target" in \
		/cargo/?*) ;; \
		*) echo "error: refusing to clean unsafe CARGO_TARGET_DIR: $$target" >&2; exit 1 ;; \
	esac; \
	$(DOCKER) run --rm -v "$(CARGO_VOLUME)":/cargo "$(RUST_IMAGE)" rm -rf "$$target"

clean:
	rm -rf "$(DIST_DIR)"

# Print the version the installers would be built with.
version:
	@"$(CURDIR)/tools/version.sh"

# Build the VmodV AppImage and Debian package. This is the release entry
# point; there is no Electron fallback build.
build: clean appimage-vmodv vmodv-deb

# VmodV AppImage: reusable Rust toolchain image + cargo cache, no Electron.
# tools/vmodv-appimage.sh verifies the fat bundle with
# tools/vmodv-appimage-check.sh on the image it just produced, so `make build`
# (and this target) fails on a thin bundle just like `vmodv-deb` does.
appimage-vmodv: disk-guard docker-cargo-volume
	mkdir -p "$(DIST_DIR)"
	$(DOCKER) run --rm \
		--ulimit core=0 \
		--user "$(HOST_UID):$(HOST_GID)" \
		-e HOME=/tmp \
		-e CARGO_HOME=/cargo \
		$(CARGO_TARGET_ENV) \
		-e APP_VERSION=$(VMODV_VERSION) \
		-v "$(CURDIR)":/app \
		-v "$(CARGO_VOLUME)":/cargo \
		-w /app \
		$(RUST_IMAGE) \
		bash /app/tools/vmodv-appimage.sh

# Backwards-compatible alias for the only AppImage build that exists.
appimage: appimage-vmodv

# Re-run the fat-bundle content checks on an already-built AppImage. Mirrors
# vmodv-deb-check: the checker runs in the toolchain container because it uses
# readelf to verify the $ORIGIN-relative RUNPATHs.
vmodv-appimage-check: docker-rust
	mkdir -p "$(DIST_DIR)"
	$(DOCKER) run --rm \
		--ulimit core=0 \
		--user "$(HOST_UID):$(HOST_GID)" \
		-e APP_VERSION=$(VMODV_VERSION) \
		-e DIST_DIR=$(DIST_DIR) \
		-v "$(CURDIR)":/app \
		-w /app \
		$(DOCKER_RUN_ARGS) \
		"$(RUST_IMAGE)" \
		bash /app/tools/vmodv-appimage-check.sh /app/$(DIST_DIR)/VmodV-$(VMODV_VERSION)-$(APPIMAGE_ARCH).AppImage

# VmodV Debian package. Reuses the cargo cache; the script builds the release
# `modv` binary, assembles the package tree and verifies the result with
# tools/vmodv-deb-check.sh.
vmodv-deb: disk-guard docker-cargo-volume
	mkdir -p "$(DIST_DIR)"
	$(DOCKER) run --rm \
		--ulimit core=0 \
		--user "$(HOST_UID):$(HOST_GID)" \
		-e HOME=/tmp \
		-e CARGO_HOME=/cargo \
		$(CARGO_TARGET_ENV) \
		-e APP_VERSION=$(VMODV_VERSION) \
		-e DIST_DIR=$(DIST_DIR) \
		-e DEB_SUFFIX=$(DEB_SUFFIX) \
		-v "$(CURDIR)":/app \
		-v "$(CARGO_VOLUME)":/cargo \
		-w /app \
		$(RUST_IMAGE) \
		bash /app/tools/vmodv-deb.sh

# Re-run the metadata/file-list/binary checks on an already-built package.
vmodv-deb-check: docker-rust
	mkdir -p "$(DIST_DIR)"
	$(DOCKER) run --rm \
		--ulimit core=0 \
		--user "$(HOST_UID):$(HOST_GID)" \
		-e APP_VERSION=$(VMODV_VERSION) \
		-e DIST_DIR=$(DIST_DIR) \
		-e DEB_SUFFIX=$(DEB_SUFFIX) \
		-v "$(CURDIR)":/app \
		-w /app \
		$(RUST_IMAGE) \
		bash /app/tools/vmodv-deb-check.sh

docker-package: disk-guard
	mkdir -p "$(DIST_DIR)"
	docker buildx build \
		--build-arg "VMODV_VERSION=$(VMODV_VERSION)" \
		--target artifacts \
		--output type=local,dest="$(DIST_DIR)" \
		$(DOCKER_BUILD_ARGS) .

docker-rust: disk-guard
	$(DOCKER) build --target rust -t "$(RUST_IMAGE)" \
		--build-arg "BASE_IMAGE=$(BASE_IMAGE)" .

docker-cargo-volume: docker-rust
	@case "$(CARGO_VOLUME)" in \
		/*) mkdir -p "$(CARGO_VOLUME)" ;; \
		*) $(DOCKER) volume inspect "$(CARGO_VOLUME)" >/dev/null 2>&1 || $(DOCKER) volume create "$(CARGO_VOLUME)" >/dev/null ;; \
	esac
	$(DOCKER) run --rm -v "$(CARGO_VOLUME)":/cargo "$(RUST_IMAGE)" chown -R "$(HOST_UID):$(HOST_GID)" /cargo

docker-reset:
	-$(DOCKER) volume rm "$(CARGO_VOLUME)"

define run-cargo
	$(DOCKER) run --rm \
		--ulimit core=0 \
		--user "$(HOST_UID):$(HOST_GID)" \
		-e HOME=/tmp \
		-e CARGO_HOME=/cargo \
		$(CARGO_TARGET_ENV) \
		-v "$(CURDIR)":/app \
		-v "$(CARGO_VOLUME)":/cargo \
		-w /app/crates \
		$(DOCKER_RUN_ARGS) \
		"$(RUST_IMAGE)" \
		cargo $(1)
endef

# VmodV shell. Release by default so frame pacing matches the packaged app;
# use `vmodv-debug` for iteration.
dev: vmodv

vmodv: DOCKER_RUN_ARGS += $(GUI_RUN_ARGS)
vmodv: docker-cargo-volume
	$(call run-cargo,run --release -p modv-app --bin modv -- $(ARGS))

vmodv-debug: DOCKER_RUN_ARGS += $(GUI_RUN_ARGS)
vmodv-debug: docker-cargo-volume
	$(call run-cargo,run -p modv-app --bin modv -- $(ARGS))

engine: docker-cargo-volume
	$(call run-cargo,build --release $(ARGS))

engine-run: DOCKER_RUN_ARGS += $(GUI_RUN_ARGS)
engine-run: docker-cargo-volume
	$(call run-cargo,run --release -p modv-app -- $(ARGS))

engine-test: docker-cargo-volume
	$(call run-cargo,test $(ARGS))

engine-lint: docker-cargo-volume
	$(call run-cargo,clippy --all-targets -- -D warnings)
	$(call run-cargo,fmt --check)

# Re-encode resources/isf/rave.exe.fs from tools/rave-words.txt.
# Uses a throwaway Node container; the app build/tests stay Node-free.
rave-reencode:
	$(DOCKER) run --rm --user "$(HOST_UID):$(HOST_GID)" -e HOME=/tmp \
		-v "$(CURDIR)":/app -w /app "$(NODE_IMAGE)" \
		node tools/rave-reencode.mjs $(ARGS)

# Rust has no separate JS formatter any more; keep the historical alias.
lint: engine-lint

format: docker-cargo-volume
	$(call run-cargo,fmt --all)

# Real-binary e2e: the process harness, the Scene/defaults/manifest parity
# tests, the control-set colour picker/Colour Key coverage and the regression
# scenarios. One worker at a time because a process owns a virtual display and
# the software GPU is shared.
test-e2e-vmodv: DOCKER_RUN_ARGS += $(GUI_RUN_ARGS)
test-e2e-vmodv: docker-cargo-volume
	$(call run-cargo,test -p modv-app \
		--test e2e_harness \
		--test e2e_process \
		--test e2e_scenes \
		--test e2e_defaults \
		--test e2e_manifest \
		--test e2e_colour \
		--test e2e_regressions \
		--test e2e_engine_status \
		--test e2e_scene_editing \
		--test e2e_user_modules \
		-- --test-threads=1)

test-e2e: test-e2e-vmodv

shell: docker-cargo-volume
	$(DOCKER) run --rm -it --user "$(HOST_UID):$(HOST_GID)" -e HOME=/tmp \
		-e CARGO_HOME=/cargo \
		$(CARGO_TARGET_ENV) \
		-v "$(CURDIR)":/app -v "$(CARGO_VOLUME)":/cargo -w /app/crates \
		$(DOCKER_RUN_ARGS) "$(RUST_IMAGE)" bash

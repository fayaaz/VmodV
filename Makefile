# VmodV Native — Rust-only build entry points.
#
# Every command runs in Docker: the host needs only Docker (with Buildx) and
# make. There is no Node, Yarn or Electron in this tree.
#
# Common targets:
#   make build            native AppImage + .deb into dist/ (clean first)
#   make appimage-native  native AppImage only
#   make native-deb       native .deb (runs native-deb-check.sh)
#   make native-deb-check re-check an existing .deb
#   make dev              native shell (release; MODV_UI is gone)
#   make native-debug     native shell, debug profile
#   make engine           release workspace build
#   make engine-test      cargo test (pass ARGS="..." to narrow)
#   make engine-lint      clippy -D warnings + rustfmt --check
#   make test-e2e-native  the real-binary e2e harness and parity tests
#   make shell            interactive shell in the Rust toolchain
#   make docker-package   build both installers via Dockerfile `artifacts`

NATIVE_VERSION ?= 2.0.0
# Dev-only asset tooling: the wordcodec is a small Node script run in a
# throwaway container (no Node is needed for the app build/tests).
NODE_IMAGE ?= node:24-slim
DOCKER_BUILD_ARGS ?=
DIST_DIR ?= dist

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
# native shell needs the render node for Vulkan (and falls back to lavapipe
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

# The process harness spawns its own Xvfb when the container has no display.
# Use the X11 forwarding path for it so an existing Wayland session does not
# change which Vulkan surface the tests exercise.
X11_GUI_ENV := -e DISPLAY="$(DISPLAY)" -e XDG_SESSION_TYPE=x11
X11_GUI_MOUNTS := -v /tmp/.X11-unix:/tmp/.X11-unix \
	$(if $(wildcard $(XAUTHORITY)),-e XAUTHORITY=/tmp/.Xauthority -v "$(XAUTHORITY):/tmp/.Xauthority:ro")
X11_GUI_AUDIO := $(if $(wildcard $(PULSE_SOCKET)),-e PULSE_SERVER=unix:/tmp/pulse-native -v "$(PULSE_SOCKET):/tmp/pulse-native")
GUI_RUN_ARGS_X11 := --net=host --shm-size=1g --ulimit core=0 $(DRI_ARGS) $(X11_GUI_ENV) $(X11_GUI_MOUNTS) $(X11_GUI_AUDIO)

# Building the binary and both installers needs several GB of BuildKit cache.
# Fail before a build starts when the filesystem is nearly full instead of
# letting Docker fill the disk.
MIN_FREE_GB ?= 20

.DEFAULT_GOAL := build
.PHONY: build appimage appimage-native native-deb native-deb-check clean dev native native-debug lint format test-e2e test-e2e-native engine engine-test engine-lint engine-run rave-reencode shell docker-rust docker-cargo-volume docker-package docker-reset disk-guard disk-clean

disk-guard:
	@free=$$(df -Pk "$(CURDIR)" | awk 'NR==2 {print int($$4/1048576)}'); \
	if [ "$$free" -lt "$(MIN_FREE_GB)" ]; then \
		echo "error: only $${free} GB free on the build filesystem (need $(MIN_FREE_GB) GB)."; \
		echo "       run 'make disk-clean' (docker builder prune) and retry."; \
		exit 1; \
	fi

disk-clean:
	-$(DOCKER) builder prune -af
	-$(DOCKER) image prune -f
	-$(DOCKER) container prune -f

clean:
	rm -rf "$(DIST_DIR)"

# Build the native AppImage and Debian package. This is the release entry
# point; there is no Electron fallback build.
build: clean appimage-native native-deb

# Native AppImage: reusable Rust toolchain image + cargo cache, no Electron.
appimage-native: disk-guard docker-cargo-volume
	mkdir -p "$(DIST_DIR)"
	$(DOCKER) run --rm \
		--ulimit core=0 \
		--user "$(HOST_UID):$(HOST_GID)" \
		-e HOME=/tmp \
		-e CARGO_HOME=/cargo \
		-e APP_VERSION=$(NATIVE_VERSION) \
		-v "$(CURDIR)":/app \
		-v "$(CARGO_VOLUME)":/cargo \
		-w /app \
		$(RUST_IMAGE) \
		bash /app/tools/native-appimage.sh

# Backwards-compatible alias for the only AppImage build that exists.
appimage: appimage-native

# Native Debian package. Reuses the cargo cache; the script builds the release
# `modv` binary, assembles the package tree and verifies the result with
# tools/native-deb-check.sh.
native-deb: disk-guard docker-cargo-volume
	mkdir -p "$(DIST_DIR)"
	$(DOCKER) run --rm \
		--ulimit core=0 \
		--user "$(HOST_UID):$(HOST_GID)" \
		-e HOME=/tmp \
		-e CARGO_HOME=/cargo \
		-e APP_VERSION=$(NATIVE_VERSION) \
		-e DIST_DIR=$(DIST_DIR) \
		-v "$(CURDIR)":/app \
		-v "$(CARGO_VOLUME)":/cargo \
		-w /app \
		$(RUST_IMAGE) \
		bash /app/tools/native-deb.sh

# Re-run the metadata/file-list/binary checks on an already-built package.
native-deb-check: docker-rust
	mkdir -p "$(DIST_DIR)"
	$(DOCKER) run --rm \
		--ulimit core=0 \
		--user "$(HOST_UID):$(HOST_GID)" \
		-e APP_VERSION=$(NATIVE_VERSION) \
		-e DIST_DIR=$(DIST_DIR) \
		-v "$(CURDIR)":/app \
		-w /app \
		$(RUST_IMAGE) \
		bash /app/tools/native-deb-check.sh

docker-package: disk-guard
	mkdir -p "$(DIST_DIR)"
	docker buildx build \
		--build-arg "NATIVE_VERSION=$(NATIVE_VERSION)" \
		--target artifacts \
		--output type=local,dest="$(DIST_DIR)" \
		$(DOCKER_BUILD_ARGS) .

docker-rust: disk-guard
	$(DOCKER) build --target rust -t "$(RUST_IMAGE)" .

docker-cargo-volume: docker-rust
	@$(DOCKER) volume inspect "$(CARGO_VOLUME)" >/dev/null 2>&1 || $(DOCKER) volume create "$(CARGO_VOLUME)" >/dev/null
	$(DOCKER) run --rm -v "$(CARGO_VOLUME)":/cargo "$(RUST_IMAGE)" chown -R "$(HOST_UID):$(HOST_GID)" /cargo

docker-reset:
	-$(DOCKER) volume rm "$(CARGO_VOLUME)"

define run-cargo
	$(DOCKER) run --rm \
		--ulimit core=0 \
		--user "$(HOST_UID):$(HOST_GID)" \
		-e HOME=/tmp \
		-e CARGO_HOME=/cargo \
		-v "$(CURDIR)":/app \
		-v "$(CARGO_VOLUME)":/cargo \
		-w /app/crates \
		$(DOCKER_RUN_ARGS) \
		"$(RUST_IMAGE)" \
		cargo $(1)
endef

# Native shell. Release by default so frame pacing matches the packaged app;
# use `native-debug` for iteration.
dev: native

native: DOCKER_RUN_ARGS += $(GUI_RUN_ARGS)
native: docker-cargo-volume
	$(call run-cargo,run --release -p modv-app --bin modv -- $(ARGS))

native-debug: DOCKER_RUN_ARGS += $(GUI_RUN_ARGS)
native-debug: docker-cargo-volume
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

# Real-binary e2e: the process harness plus the Scene/defaults/manifest parity
# tests. One worker at a time because a process owns a virtual display and the
# software GPU is shared.
test-e2e-native: DOCKER_RUN_ARGS += $(GUI_RUN_ARGS_X11)
test-e2e-native: docker-cargo-volume
	$(call run-cargo,test -p modv-app \
		--test e2e_harness \
		--test e2e_process \
		--test e2e_scenes \
		--test e2e_defaults \
		--test e2e_manifest \
		-- --test-threads=1)

test-e2e: test-e2e-native

shell: docker-cargo-volume
	$(DOCKER) run --rm -it --user "$(HOST_UID):$(HOST_GID)" -e HOME=/tmp \
		-e CARGO_HOME=/cargo \
		-v "$(CURDIR)":/app -v "$(CARGO_VOLUME)":/cargo -w /app/crates \
		$(DOCKER_RUN_ARGS) "$(RUST_IMAGE)" bash

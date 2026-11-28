# VmodV — Rust-only build entry points.
#
# Every command runs in Docker: the host needs only Docker (with Buildx) and
# make. There is no Node, Yarn or Electron in this tree.
#
# Common targets:
#   make build            VmodV AppImage + .deb into dist/ (clean first)
#   make appimage       VmodV AppImage only (runs vmodv-appimage-check.sh)
#   make vmodv-deb       VmodV .deb (runs vmodv-deb-check.sh)
#   make vmodv-deb-check re-check an existing .deb
#   make vmodv-appimage-check re-check an existing AppImage's fat bundle
#   make dev              VmodV shell (release; MODV_UI is gone)
#   make vmodv-debug     VmodV shell, debug profile
#   make vmodv-shot      capture the main output for SCENE to OUT (PNG)
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
# Extra Buildx flags for the image builds. CI passes the GitHub Actions cache
# here: `--cache-from type=gha,scope=...` (and `--cache-to` where it owns the
# export), so a fresh runner imports the apt/rustup layers instead of redoing
# them.
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
WASM_IMAGE ?= modv-wasm:local
# Image builds go through Buildx so CI can pass the GitHub Actions cache via
# DOCKER_BUILD_ARGS (`--cache-from/--cache-to type=gha,scope=...`). The source
# label records the Dockerfile hash and base image the image was built from:
# when CI has already loaded the image (docker/build-push-action), repeated
# `make` invocations reuse it instead of rebuilding the apt/rustup layers.
DOCKERFILE_HASH ?= $(shell git hash-object Dockerfile 2>/dev/null || sha256sum Dockerfile 2>/dev/null | cut -d' ' -f1)
IMAGE_SOURCE_LABEL ?= org.vmodv.source
IMAGE_SOURCE_VALUE = $(DOCKERFILE_HASH)|$(BASE_IMAGE)
image-is-current = $(DOCKER) image inspect -f '{{index .Config.Labels "$(IMAGE_SOURCE_LABEL)"}}' "$(1)" 2>/dev/null | grep -qxF '$(IMAGE_SOURCE_VALUE)'
CARGO_VOLUME ?= modv-cargo
HOST_UID ?= $(shell id -u)
HOST_GID ?= $(shell id -g)
DOCKER_RUN_ARGS ?=
# COVERAGE=1 instruments the canonical test suites with cargo-llvm-cov; the
# resulting profiles are merged by `make coverage-report` at the end of the
# job. Off locally so ordinary `make engine-test` stays fast.
COVERAGE ?= 0
ifeq ($(COVERAGE),1)
TEST_WRAPPER := llvm-cov --no-report
else
TEST_WRAPPER :=
endif
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
.PHONY: build appimage vmodv-appimage-check vmodv-deb vmodv-deb-check clean dev vmodv vmodv-debug vmodv-shot lint format test-e2e test-e2e-vmodv coverage-report coverage-lcov coverage-merge coverage-summary engine engine-test engine-lint engine-run rave-reencode shell docker-rust docker-wasm docker-cargo-volume docker-package docker-reset disk-guard disk-clean disk-clean-targets version web-check web-build web-serve

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
# point; there is no Electron fallback build. The `.deb` is amd64-only, so a
# non-amd64 host builds just its native AppImage (an explicit `make vmodv-deb`
# still reaches the script, which fails fast with the detected host arch).
BUILD_TARGETS := appimage
ifneq ($(filter $(shell uname -m),x86_64 amd64),)
BUILD_TARGETS += vmodv-deb
endif

build: clean $(BUILD_TARGETS)

# VmodV AppImage: reusable Rust toolchain image + cargo cache, no Electron.
# tools/vmodv-appimage.sh verifies the fat bundle with
# tools/vmodv-appimage-check.sh on the image it just produced, so `make build`
# (and this target) fails on a thin bundle just like `vmodv-deb` does.
appimage: disk-guard docker-cargo-volume
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
	@if $(call image-is-current,$(RUST_IMAGE)); then \
		echo "docker-rust: $(RUST_IMAGE) already built from this Dockerfile and base image"; \
	else \
		$(DOCKER) buildx build --load --target rust -t "$(RUST_IMAGE)" \
			--build-arg "BASE_IMAGE=$(BASE_IMAGE)" \
			--label "$(IMAGE_SOURCE_LABEL)=$(IMAGE_SOURCE_VALUE)" \
			$(DOCKER_BUILD_ARGS) . ; \
	fi

docker-cargo-volume: docker-rust
	@case "$(CARGO_VOLUME)" in \
		/*) mkdir -p "$(CARGO_VOLUME)" ;; \
		*) $(DOCKER) volume inspect "$(CARGO_VOLUME)" >/dev/null 2>&1 || $(DOCKER) volume create "$(CARGO_VOLUME)" >/dev/null ;; \
	esac
	@# Recursive chown over a warm registry/target tree is slow; every make
	@# target would otherwise redo it. The root directory ownership is enough
	@# to know a previous invocation already fixed the volume up.
	@$(DOCKER) run --rm -v "$(CARGO_VOLUME)":/cargo "$(RUST_IMAGE)" sh -c \
		'[ "$$(stat -c %u:%g /cargo)" = "$(HOST_UID):$(HOST_GID)" ] || chown -R "$(HOST_UID):$(HOST_GID)" /cargo'

docker-wasm: docker-cargo-volume
	@if $(call image-is-current,$(WASM_IMAGE)); then \
		echo "docker-wasm: $(WASM_IMAGE) already built from this Dockerfile and base image"; \
	else \
		$(DOCKER) buildx build --load --target wasm -t "$(WASM_IMAGE)" \
			--label "$(IMAGE_SOURCE_LABEL)=$(IMAGE_SOURCE_VALUE)" \
			$(DOCKER_BUILD_ARGS) . ; \
	fi

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

# Same container, but routes `test` through the coverage wrapper when
# COVERAGE=1. Kept separate from run-cargo so lint/build/run are never
# instrumented.
define run-cargo-test
	$(call run-cargo,$(TEST_WRAPPER) $(1))
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

# Capture the shell's real main output for one named module/Point10 scene:
#   make vmodv-shot SCENE="Plane9 Waves of Sin" OUT=scratch/shots/waves-of-sin.png
#   make vmodv-shot SCENE="Plane9 Spiky" OUT=scratch/shots/spiky.png \
#       VMODV_SHOT_ARGS="1280 720 300"
# Runs on the host (bash + python3) and drives the `vmodv` target (and therefore
# Docker) itself; it needs no other Make target.
vmodv-shot:
	@test -n "$(SCENE)" || { echo "error: set SCENE=\"<canonical scene name>\"" >&2; exit 2; }
	@test -n "$(OUT)" || { echo "error: set OUT=scratch/shots/<name>.png" >&2; exit 2; }
	"$(CURDIR)/tools/vmodv-shot.sh" "$(SCENE)" "$(OUT)" $(VMODV_SHOT_ARGS)

engine: docker-cargo-volume
	$(call run-cargo,build --release $(ARGS))

engine-run: DOCKER_RUN_ARGS += $(GUI_RUN_ARGS)
engine-run: docker-cargo-volume
	$(call run-cargo,run --release -p modv-app -- $(ARGS))

engine-test: docker-cargo-volume
	$(call run-cargo-test,test $(ARGS))

# The Point10 volume simulations and ISF corpus checks in modv-renderers are
# the slowest part of the workspace and exercise the same lavapipe device as
# the shader/e2e suites. CI runs them on that leg, excluded from the workspace
# sweep; `test_gpu` names are left to test-shader-gpu so nothing runs twice.
test-renderers: docker-cargo-volume
	$(call run-cargo-test,test -p modv-renderers -- --test-threads=1 --skip test_gpu)

# GPU-executed shader unit tests (compute shim, built-in registry, ISF corpus).
# CI runs this step immediately before the e2e suites in the same job, and
# skips these tests in the workspace sweep so they are not run twice.
test-shader-gpu: docker-cargo-volume
	$(call run-cargo-test,test -p modv-renderers --lib test_gpu -- --test-threads=1)

engine-lint: docker-cargo-volume
	$(call run-cargo,clippy --all-targets -- -D warnings)
	$(call run-cargo,fmt --check)

# GitHub Pages module catalogue. `module-docs` regenerates docs/modules/*.json
# from the app's registry; `preview-wasm` rebuilds the WebAssembly runtime the
# live WebGPU previews use. Both outputs are committed so Pages serves plain
# static files. `pages-data` refreshes both at once.
module-docs: docker-cargo-volume
	$(call run-cargo,run --quiet -p modv-app --bin gen-module-docs)

preview-wasm: docker-wasm
	$(DOCKER) run --rm \
		--ulimit core=0 \
		--user "$(HOST_UID):$(HOST_GID)" \
		-e HOME=/tmp \
		-e CARGO_HOME=/cargo \
		$(CARGO_TARGET_ENV) \
		-v "$(CURDIR)":/app \
		-v "$(CARGO_VOLUME)":/cargo \
		-w /app \
		"$(WASM_IMAGE)" \
		bash /app/tools/build-preview-wasm.sh

pages-data: module-docs preview-wasm

# Browser shell (wasm32 + WebGPU). `web-build` writes web/pkg; `web-serve`
# serves the static page locally for a browser smoke test.
web-check: docker-wasm
	$(DOCKER) run --rm \
		--ulimit core=0 \
		--user "$(HOST_UID):$(HOST_GID)" \
		-e HOME=/tmp \
		-e CARGO_HOME=/cargo \
		-e CARGO_TARGET_DIR=/cargo/target \
		-v "$(CURDIR)":/app \
		-v "$(CARGO_VOLUME)":/cargo \
		-w /app \
		"$(WASM_IMAGE)" \
		cargo check --manifest-path /app/crates/Cargo.toml --lib \
			--target wasm32-unknown-unknown -p modv-web

web-build: docker-wasm
	$(DOCKER) run --rm \
		--ulimit core=0 \
		--user "$(HOST_UID):$(HOST_GID)" \
		-e HOME=/tmp \
		-e CARGO_HOME=/cargo \
		-e CARGO_TARGET_DIR=/cargo/target \
		-v "$(CURDIR)":/app \
		-v "$(CARGO_VOLUME)":/cargo \
		-w /app \
		"$(WASM_IMAGE)" \
		bash /app/tools/build-web.sh

web-serve:
	$(DOCKER) run --rm -p 8000:8000 \
		-v "$(CURDIR)/web":/web -w /web \
		python:3.12-slim python -m http.server 8000


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

# Static/packaging/CI guards: pure text and filesystem tests that never launch
# the app. This is the single source of truth for the `--test` list — CI and
# release validation both call this recipe, so adding a guard test only needs
# one edit (ci_test_coverage.rs fails if a target is left out).
test-static-guards: docker-cargo-volume
	$(call run-cargo-test,test -p modv-app \
		--test appimage_arm64 \
		--test appimage_bundle \
		--test branding \
		--test ci_coverage_report \
		--test ci_docker_cache \
		--test ci_test_coverage \
		--test deb_package \
		--test macos_portable_layout \
		--test media_probe \
		--test module_docs \
		--test ndi_readback \
		--test notices \
		--test pages_workflow \
		--test release_cleanup \
		--test release_resilience \
		--test release_resume \
		--test removed_content \
		--test undo_history \
		--test windows_portable_layout \
		-- --test-threads=1)

# Real-binary e2e: the process harness, the Scene/defaults/manifest parity
# tests, the control-set colour picker/Colour Key coverage and the regression
# scenarios. One worker at a time because a process owns a virtual display and
# the software GPU is shared.
test-e2e-vmodv: DOCKER_RUN_ARGS += $(GUI_RUN_ARGS)
test-e2e-vmodv: docker-cargo-volume
	$(call run-cargo-test,test -p modv-app \
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

# Merge all profiles recorded by the COVERAGE=1 suites into browsable HTML
# plus LCOV/JSON. Run in the same job/target dir as those suites so the object
# files still carry the coverage mappings that match the profiles.
coverage-report: docker-cargo-volume
	@mkdir -p "$(CURDIR)/coverage"
	@find "$(CURDIR)/crates" -maxdepth 1 -name '*.profraw' -delete
	$(call run-cargo,llvm-cov report --html --output-dir /app/coverage) > /dev/null
	$(call run-cargo,llvm-cov report --lcov --output-path /app/coverage/lcov.info) > /dev/null
	$(call run-cargo,llvm-cov report --json --output-path /app/coverage/coverage.json) > /dev/null

# Release validation jobs run on separate runners, so they cannot merge their
# profiles in place. Each produces just this LCOV profile; the `coverage` job
# adds them together afterwards.
coverage-lcov: docker-cargo-volume
	@mkdir -p "$(CURDIR)/coverage"
	@find "$(CURDIR)/crates" -maxdepth 1 -name '*.profraw' -delete
	$(call run-cargo,llvm-cov report --lcov --output-path /app/coverage/lcov.info) > /dev/null

# `actions/download-artifact` places the two profiles under
# coverage/profiles/{validate,e2e}/ on the runner. lcov sums the per-line
# counts; genhtml renders the combined tracefile as one browsable report.
coverage-merge: docker-rust
	@mkdir -p "$(CURDIR)/coverage"
	$(DOCKER) run --rm --user "$(HOST_UID):$(HOST_GID)" -e HOME=/tmp \
		-v "$(CURDIR)":/app -w /app \
		"$(RUST_IMAGE)" \
		bash -c 'lcov -a coverage/profiles/validate/lcov.info \
			-a coverage/profiles/e2e/lcov.info \
			-o coverage/lcov.info \
			&& genhtml coverage/lcov.info --output-directory coverage/html \
				--title "VmodV line coverage"'

# Print a Markdown coverage table for $GITHUB_STEP_SUMMARY, alongside the
# Docker build summary the build-push steps already write. Pure host python3
# (like tools/vmodv-shot.sh): no container or coverage re-run needed.
coverage-summary:
	@python3 tools/coverage-summary.py "$(CURDIR)/coverage/lcov.info"

shell: docker-cargo-volume
	$(DOCKER) run --rm -it --user "$(HOST_UID):$(HOST_GID)" -e HOME=/tmp \
		-e CARGO_HOME=/cargo \
		$(CARGO_TARGET_ENV) \
		-v "$(CURDIR)":/app -v "$(CARGO_VOLUME)":/cargo -w /app/crates \
		$(DOCKER_RUN_ARGS) "$(RUST_IMAGE)" bash

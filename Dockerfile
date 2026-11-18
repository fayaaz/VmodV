# syntax=docker/dockerfile:1
#
# Rust-only build for VmodV Native. There is no Node, Yarn or Electron stage:
# the app is the `modv` binary in the `crates/` workspace, and the native
# installers bundle that binary plus `resources/isf` and the Point10 textures.
#
# Targets:
#   rust             clang/fmt, GStreamer/PipeWire/Vulkan/Xvfb toolchain used by
#                    `make engine*`, `make dev` and `make test-e2e-native`.
#   native-package   builds both installers inside the build (slow, no cache);
#                    `make build` uses the cached `docker run` path instead.
#   appimage / deb   metadata-only stages holding the respective artifact.
#   artifacts        both installers, for `docker buildx --output type=local`.

ARG DEBIAN_RELEASE=trixie

FROM debian:${DEBIAN_RELEASE}-slim AS base
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
      ca-certificates \
      curl \
      wget \
      git \
      make \
      g++ \
      pkg-config \
      file \
      fakeroot \
      dpkg \
      binutils \
      xz-utils \
      zstd \
      libarchive-tools \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# The day-to-day toolchain for every `make engine*` / `make native*` target.
# The runtime packages are also what the Xvfb-backed native e2e harness and the
# packaged-app smoke checks need, so one image serves build, test and package.
FROM base AS rust
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
      libvulkan-dev \
      mesa-vulkan-drivers \
      libasound2-dev \
      libudev-dev \
      libpipewire-0.3-dev \
      libpipewire-0.3-0 \
      libgstreamer1.0-dev \
      libgstreamer-plugins-base1.0-dev \
      gstreamer1.0-plugins-base \
      gstreamer1.0-plugins-good \
      gstreamer1.0-plugins-bad \
      gstreamer1.0-libav \
      gstreamer1.0-pipewire \
      libclang-dev \
      libxkbcommon-dev \
      libxkbcommon-x11-0 \
      libgl1-mesa-dev \
      libwayland-dev \
      libx11-dev \
      libx11-6 \
      libxcursor1 \
      libxrandr2 \
      libxi6 \
      libxinerama1 \
      libxcb-render0 \
      libxcb-shape0 \
      libxcb-xfixes0 \
      libxcb-xkb1 \
      xvfb \
      xauth \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --default-toolchain stable --profile minimal --no-modify-path \
    && rustup component add rustfmt clippy

# Build both native installers in one stage. The scripts are the same ones the
# Makefile runs with a cargo volume cache; this stage exists for reproducible
# `docker buildx` release artifacts and does not use Node.
FROM rust AS native-package
ARG NATIVE_VERSION=2.0.0
ENV APP_VERSION=${NATIVE_VERSION}
COPY . .
RUN bash tools/native-appimage.sh \
    && bash tools/native-deb.sh

FROM scratch AS appimage
COPY --from=native-package /app/dist/*.AppImage /

FROM scratch AS deb
COPY --from=native-package /app/dist/*.deb /

FROM scratch AS artifacts
COPY --from=appimage / /
COPY --from=deb / /

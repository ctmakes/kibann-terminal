# A Linux desktop-less box to build and run Kibann under Xvfb.
# Used by scripts/linux-dev.sh; nothing here is shipped.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential pkg-config curl ca-certificates git rsync xz-utils file patchelf \
      libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
      librsvg2-dev libxdo-dev libssl-dev \
      xvfb openbox x11-utils xdotool wmctrl imagemagick dbus-x11 \
      fonts-dejavu fonts-jetbrains-mono fonts-noto-cjk fonts-noto-color-emoji fontconfig \
      zsh vim htop locales \
  && locale-gen en_US.UTF-8 \
  && rm -rf /var/lib/apt/lists/*
ENV LANG=en_US.UTF-8

# Node 24, as CI.
RUN curl -fsSL https://deb.nodesource.com/setup_24.x | bash - \
  && apt-get install -y nodejs && rm -rf /var/lib/apt/lists/*

# Zig for libghostty-vt's build: linux-dev.sh passes .zig-version, as CI
# uses.
ARG ZIG
RUN arch=$(uname -m) \
  && curl -fsSL "https://ziglang.org/download/${ZIG}/zig-${arch}-linux-${ZIG}.tar.xz" \
     | tar -xJ -C /opt && ln -s /opt/zig-${arch}-linux-${ZIG}/zig /usr/local/bin/zig

RUN useradd -m -s /bin/zsh dev \
  && mkdir -p /home/dev/work /home/dev/.cargo/registry \
  && chown -R dev:dev /home/dev
USER dev
# rustup; the first cargo command installs rust-toolchain.toml's Rust.
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
ENV PATH=/home/dev/.cargo/bin:$PATH
WORKDIR /home/dev

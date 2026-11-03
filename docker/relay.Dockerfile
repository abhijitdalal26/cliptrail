# clipd-relay: the ClipTrail off-LAN sync relay (ciphertext only).
# Build from the repo root:
#   docker build -f docker/relay.Dockerfile -t cliptrail-relay .
# Multi-arch (amd64 VPS, arm64 Raspberry Pi) works with the same file.

FROM rust:1.96-slim-bookworm AS build
WORKDIR /src
# Only what the workspace needs to resolve and build the relay. The desktop
# app is a workspace member, so its manifest and sources must exist, but it
# isn't compiled. (rust-toolchain.toml is left out on purpose: the image
# already is 1.96 and needs no clippy/rustfmt.)
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY third_party ./third_party
COPY apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/build.rs ./apps/desktop/src-tauri/
COPY apps/desktop/src-tauri/src ./apps/desktop/src-tauri/src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p clipd-relay \
    && cp target/release/clipd-relay /clipd-relay \
    && mkdir -p /data

# Small runtime with glibc + libgcc only, no shell, runs as uid 65532.
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /clipd-relay /usr/local/bin/clipd-relay
COPY --from=build --chown=65532:65532 /data /data
ENV CLIPD_RELAY_BIND=0.0.0.0:8787 \
    CLIPD_RELAY_DATA=/data
EXPOSE 8787
VOLUME ["/data"]
USER nonroot
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s \
    CMD ["/usr/local/bin/clipd-relay", "--healthcheck"]
ENTRYPOINT ["/usr/local/bin/clipd-relay"]

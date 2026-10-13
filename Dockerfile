# mcpd: a single static binary with its interface compiled in.
#
# The image is the binary on the Alpine base, with nothing installed on top.
# That base has busybox, a shell and apk, and they stay for one reason: the
# HEALTHCHECK below runs busybox wget inside the container. A distroless or
# scratch base would need a health subcommand in the binary first. The binary
# itself is static and needs none of them.
#
# One Dockerfile for every platform. The interface and the compiler run on the
# machine doing the build (BUILDPLATFORM) and the compiler cross-compiles to
# the image's platform (TARGETPLATFORM), so an arm64 image never runs rustc
# under emulation, which would take hours for a fat-LTO build.
#
# Every base image is pinned to a digest as well as a tag. A tag moves when the
# publisher rebuilds it, so two builds of the same commit a week apart could
# start from different bytes; the digest is what makes a rebuild reproducible.
# Dependabot reads these pins and proposes the new digest when the tag moves.

# --- the cross-compilation helpers --------------------------------------------
# xx maps TARGETPLATFORM to a Rust target triple, installs the target's C
# libraries into a sysroot, and points cargo's linker and the cc crate at it.
FROM --platform=$BUILDPLATFORM tonistiigi/xx:1.6.1@sha256:923441d7c25f1e2eb5789f82d987693c47b8ed987c4ab3b075d6ed2b5d6779a3 AS xx

# --- the web application -----------------------------------------------------
# Static files, the same bytes on every platform, so built once on the build
# machine.
FROM --platform=$BUILDPLATFORM node:22-alpine@sha256:c610fcdfb1d5b4740dd70c284ed3cb16bb857e0f7166196e36a5501df7a3aa32 AS ui

WORKDIR /build
# Copy manifests first so a source-only change does not reinstall the world.
#
# `npm ci` and nothing else. The output of this install is the interface that
# `include_dir!` compiles into the shipped binary, so a lock file that no longer
# matches package.json has to stop the build here rather than fall through to
# an unlocked resolve that puts unpinned packages into a release.
COPY web/package.json web/package-lock.json ./web/
RUN cd web && npm ci --no-audit --no-fund

# The site reads other parts of the repository at build time rather than
# carrying copies: its code samples from the example programs, its reference
# pages from the spec, the deploy page from the manifests in deploy/, and the
# conformance page from the harness under tests/e2e. Every one of those trees
# has to be present or the build fails on the first page that reads it.
COPY spec ./spec
COPY examples ./examples
COPY deploy ./deploy
COPY tests/e2e ./tests/e2e
# The cost page renders the built-in price table from its data file.
COPY crates/mcpd-core/prices ./crates/mcpd-core/prices
COPY web ./web
RUN cd web && npm run build

# --- the binary --------------------------------------------------------------
# Current stable, not the floor in Cargo.toml. The floor (rust-version, 1.88)
# is the oldest compiler the crate graph declares it needs, and it is a
# statement about the code; this is a statement about what builds the release.
# A newer compiler is safe here for the reason .github/dependabot.yml gives.
#
# release.yml pins its binary builds to the same compiler (1.98.1, the one in
# this digest), so an archive and an image of one tag are built alike.
FROM --platform=$BUILDPLATFORM rust:1.98-alpine@sha256:1716b3aa042d735f4566d14dc54e8037de9d69556e2d5dd58131d93a613d173d AS build

COPY --from=xx / /
# clang and lld cross-link for any target; musl-dev is the build machine's own
# libc, for build scripts that run here.
RUN apk add --no-cache clang lld musl-dev

ARG TARGETPLATFORM
# The target's C runtime and compiler support, for rusqlite's bundled SQLite.
# On a native build this installs into / like a plain apk add.
RUN xx-apk add --no-cache musl-dev gcc

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY spec ./spec
COPY --from=ui /build/web/dist ./crates/mcpd-server/ui

# musl gives a genuinely static binary, so the final stage needs no libc.
# `--locked` because Cargo.lock is what a release is built from: a lock file
# that no longer matches the manifests stops the build rather than being
# resolved again here. The dist profile strips the binary itself.
ENV RUSTFLAGS="-C target-feature=+crt-static"
RUN xx-cargo build --locked --profile dist -p mcpd \
 && cp "target/$(xx-cargo --print-target-triple)/dist/mcpd" /mcpd \
 && xx-verify --static /mcpd

# --- the user ----------------------------------------------------------------
# A non-root user, and a data directory it owns. Running as root inside a
# container is a habit worth not having. The chart and the manifests in
# deploy/k8s run the pod as this same uid and gid, 10001.
#
# Made on the build machine and copied in, so the stage below runs no command
# at all. A RUN in the image's own stage would execute arm64 binaries on an
# amd64 builder, which needs QEMU installed there; plain text files do not
# care which machine wrote them.
FROM --platform=$BUILDPLATFORM alpine:3.24@sha256:28bd5fe8b56d1bd048e5babf5b10710ebe0bae67db86916198a6eec434943f8b AS user
RUN addgroup -g 10001 mcpd \
 && adduser -D -H -h /data -s /sbin/nologin -u 10001 -G mcpd mcpd \
 && mkdir -p /data

# --- the image ---------------------------------------------------------------
# The base carries the Mozilla certificate bundle (ca-certificates-bundle) at
# /etc/ssl/certs/ca-certificates.crt. reqwest 0.13 verifies TLS peers against
# the platform's roots rather than a bundle compiled into the binary, so the
# OTLP forward endpoint, the judge and a ClickHouse over HTTPS read that file.
# It also means an operator's own CA works by mounting it into
# /etc/ssl/certs, which a compiled-in bundle could not honour.
FROM alpine:3.24@sha256:28bd5fe8b56d1bd048e5babf5b10710ebe0bae67db86916198a6eec434943f8b

COPY --from=user /etc/passwd /etc/group /etc/shadow /etc/
COPY --from=user --chown=10001:10001 /data /data
COPY --from=build /mcpd /usr/local/bin/mcpd

USER 10001:10001
WORKDIR /data
VOLUME ["/data"]
EXPOSE 4317

ENV MCPD_LISTEN=0.0.0.0:4317 \
    MCPD_DATA=/data/mcpd.db

# The liveness endpoint, not the readiness one: /readyz touches storage, and a
# slow query must not make the container report unhealthy. The port is read
# from MCPD_LISTEN so an operator who moves the listener does not silently
# break the probe. busybox wget is in the base image; curl is not.
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD wget -q -T 2 -O /dev/null "http://127.0.0.1:${MCPD_LISTEN##*:}/healthz" || exit 1

ENTRYPOINT ["/usr/local/bin/mcpd"]
CMD ["serve"]

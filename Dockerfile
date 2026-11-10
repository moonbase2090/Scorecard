# Multi-stage build of sc and sc-mcp. The runtime image is Debian slim so the
# binaries keep the glibc they were linked against in the bookworm builder.
# Pack compilers are not included; git is, because sc analyze reads the work tree.

FROM rust:1.88-bookworm AS build

WORKDIR /src
ENV CARGO_TERM_COLOR=never \
    CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY skills ./skills
COPY analyzer.toml.example ./

RUN cargo build --release --locked -p sc-cli -p sc-mcp

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/* \
    && git config --system --add safe.directory '*'

COPY --from=build /src/target/release/sc /src/target/release/sc-mcp /usr/local/bin/
COPY LICENSE /usr/share/doc/sc/LICENSE

LABEL org.opencontainers.image.source="https://github.com/moonbase2090/Scorecard" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.title="sc" \
      org.opencontainers.image.description="Local code-quality gate"

WORKDIR /work
ENTRYPOINT ["sc"]

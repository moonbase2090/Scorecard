# Package sc and sc-mcp built from this repository.
# The builder image is the workspace MSRV. rust-toolchain.toml tracks stable,
# which would download another toolchain inside the image.

FROM rust:1.88.0-bookworm AS build
WORKDIR /src
COPY . .
RUN rm -f rust-toolchain.toml \
    && cargo build --release --locked -p sc-cli -p sc-mcp \
    && mkdir -p /out \
    && cp target/release/sc target/release/sc-mcp /out/

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates git \
    && rm -rf /var/lib/apt/lists/* \
    && git config --system --add safe.directory '*'
COPY --from=build /out/sc /usr/local/bin/sc
COPY --from=build /out/sc-mcp /usr/local/bin/sc-mcp
WORKDIR /work
ENTRYPOINT ["sc"]
CMD ["--version"]

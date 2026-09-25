# Toolchain image for scorecard language packs.
# Host binaries win. The runner uses this image when a pack tool is missing.
FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        bats \
        ca-certificates \
        clang-tidy \
        curl \
        gnupg \
        gradle \
        maven \
        nodejs \
        npm \
        openjdk-17-jdk-headless \
        php-cli \
        shellcheck \
        unzip \
        wget \
    && rm -rf /var/lib/apt/lists/*

RUN wget -q https://packages.microsoft.com/config/debian/12/packages-microsoft-prod.deb -O /tmp/packages-microsoft-prod.deb \
    && dpkg -i /tmp/packages-microsoft-prod.deb \
    && rm /tmp/packages-microsoft-prod.deb \
    && apt-get update \
    && apt-get install -y --no-install-recommends dotnet-sdk-8.0 \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL -o /usr/local/bin/phpstan \
        https://github.com/phpstan/phpstan/releases/download/2.1.17/phpstan.phar \
    && curl -fsSL -o /usr/local/bin/phpunit \
        https://phar.phpunit.de/phpunit-11.5.21.phar \
    && chmod 0755 /usr/local/bin/phpstan /usr/local/bin/phpunit

RUN npm install -g typescript@5.8.3 eslint@9.25.1 \
    && npm cache clean --force

RUN apt-get update \
    && apt-get install -y --no-install-recommends php-xml php-mbstring \
    && rm -rf /var/lib/apt/lists/*

RUN echo "deb http://deb.debian.org/debian bookworm-backports main" > /etc/apt/sources.list.d/backports.list \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
        cmake \
        g++ \
        lcov \
        make \
        php-pcov \
    && apt-get install -y --no-install-recommends -t bookworm-backports kcov \
    && rm -rf /var/lib/apt/lists/* \
    && npm install -g c8@10.1.3 \
    && npm cache clean --force \
    && dotnet tool install --tool-path /opt/dotnet-tools dotnet-coverage \
    && ln -sf /opt/dotnet-tools/dotnet-coverage /usr/local/bin/dotnet-coverage

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH="/usr/local/cargo/bin:/opt/dotnet-tools:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"

# Rust and Python are the host-missing fallback. Host binaries still win.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        git \
        libssl-dev \
        pkg-config \
        python3 \
        python3-pip \
        python3-pytest \
        python3-venv \
    && pip3 install --break-system-packages pytest-cov ruff \
    && curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR=/usr/local/bin sh \
    && curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | \
        sh -s -- -y --default-toolchain stable --profile minimal \
        --component llvm-tools,clippy,rustfmt --no-modify-path \
    && cargo install --locked --version 0.9.1 cargo-llvm-cov \
    && git config --system --add safe.directory '*' \
    && chmod -R a+rwX /usr/local/rustup /usr/local/cargo \
    && rm -rf /var/lib/apt/lists/* /root/.cache /tmp/*

# Official Go. Bookworm's golang-go package is too old for current modules.
RUN set -eux \
    && arch="$(dpkg --print-architecture)" \
    && curl -fsSL "https://go.dev/dl/go1.27.1.linux-${arch}.tar.gz" -o /tmp/go.tgz \
    && rm -rf /usr/local/go \
    && tar -C /usr/local -xzf /tmp/go.tgz \
    && ln -sf /usr/local/go/bin/go /usr/local/bin/go \
    && ln -sf /usr/local/go/bin/gofmt /usr/local/bin/gofmt \
    && rm -f /tmp/go.tgz

WORKDIR /src
CMD ["bash"]

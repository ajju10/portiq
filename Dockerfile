FROM rust:1.96 AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./

RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

COPY src src

RUN touch src/main.rs

RUN cargo build --release

FROM debian:trixie-slim

ARG UID=1000
ARG GID=1000
ARG UNAME=nonroot
ARG GNAME=nonroot

RUN groupadd -g ${GID} ${GNAME} && \
    useradd -m -u ${UID} -g ${GNAME} ${UNAME}

RUN apt-get update && \
    DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --chown=${UNAME}:${GNAME} --from=builder /app/target/release/portiq /usr/bin/portiq

COPY portiq.example.yaml /etc/portiq/portiq.yaml

USER ${UNAME}

ENTRYPOINT ["portiq", "--config", "/etc/portiq/portiq.yaml"]

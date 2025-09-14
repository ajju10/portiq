FROM rust:1.89 AS chef

RUN cargo install cargo-chef 

RUN apt-get update && apt-get install -y \
    cmake \
    pkg-config \
    build-essential \
    clang \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

FROM chef AS planner

COPY . .

RUN cargo chef prepare  --recipe-path recipe.json

FROM chef AS builder

COPY --from=planner /app/recipe.json recipe.json

RUN cargo chef cook --release --recipe-path recipe.json

COPY . .

RUN cargo build --release

FROM debian:trixie-slim AS runtime

RUN mkdir -p /etc/portiq

WORKDIR /app

COPY --from=builder /app/target/release/portiq .

ENTRYPOINT ["./portiq"]

CMD ["--config", "/etc/portiq/portiq.yml"]

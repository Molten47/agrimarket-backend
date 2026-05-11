# -- Stage 1: Builder ------------------------------------------------
FROM rust:1.88-slim AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev `n    libpq-dev \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates/api/Cargo.toml      crates/api/Cargo.toml
COPY crates/auth/Cargo.toml     crates/auth/Cargo.toml
COPY crates/core/Cargo.toml     crates/core/Cargo.toml
COPY crates/mailer/Cargo.toml   crates/mailer/Cargo.toml
COPY crates/payment/Cargo.toml  crates/payment/Cargo.toml
COPY crates/ws/Cargo.toml       crates/ws/Cargo.toml

RUN mkdir -p crates/api/src      && echo 'fn main(){}' > crates/api/src/main.rs
RUN mkdir -p crates/auth/src     && echo '' > crates/auth/src/lib.rs
RUN mkdir -p crates/core/src     && echo '' > crates/core/src/lib.rs
RUN mkdir -p crates/mailer/src   && echo '' > crates/mailer/src/lib.rs
RUN mkdir -p crates/payment/src  && echo '' > crates/payment/src/lib.rs
RUN mkdir -p crates/ws/src       && echo '' > crates/ws/src/lib.rs

RUN SQLX_OFFLINE=true cargo build --release 2>/dev/null; exit 0

COPY . .

RUN touch crates/api/src/main.rs \
          crates/auth/src/lib.rs \
          crates/core/src/lib.rs \
          crates/mailer/src/lib.rs \
          crates/payment/src/lib.rs \
          crates/ws/src/lib.rs

RUN SQLX_OFFLINE=true cargo build --release --bin agrimarket

# -- Stage 2: Runtime ------------------------------------------------
FROM debian:bookworm-slim
WORKDIR /app

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/agrimarket .
CMD ["./agrimarket"]

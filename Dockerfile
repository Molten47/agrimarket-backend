# ── Stage 1: Builder ─────────────────────────────────────────────────────────
FROM rust:1.88-slim AS builder

WORKDIR /app

# System deps needed to compile (openssl, pkg-config)
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace manifests first — Docker cache layer trick:
# if only src changes, Cargo deps are NOT re-downloaded
COPY Cargo.toml Cargo.lock ./
COPY crates/api/Cargo.toml      crates/api/Cargo.toml
COPY crates/auth/Cargo.toml     crates/auth/Cargo.toml
COPY crates/core/Cargo.toml     crates/core/Cargo.toml
COPY crates/mailer/Cargo.toml   crates/mailer/Cargo.toml
COPY crates/payment/Cargo.toml  crates/payment/Cargo.toml
COPY crates/ws/Cargo.toml       crates/ws/Cargo.toml

# Dummy src files so cargo can resolve the workspace without real source
RUN mkdir -p crates/api/src      && echo 'fn main(){}' > crates/api/src/main.rs
RUN mkdir -p crates/auth/src     && echo '' > crates/auth/src/lib.rs
RUN mkdir -p crates/core/src     && echo '' > crates/core/src/lib.rs
RUN mkdir -p crates/mailer/src   && echo '' > crates/mailer/src/lib.rs
RUN mkdir -p crates/payment/src  && echo '' > crates/payment/src/lib.rs
RUN mkdir -p crates/ws/src       && echo '' > crates/ws/src/lib.rs

# Pre-fetch & compile all dependencies (cached unless Cargo.toml changes)
RUN cargo build --release 2>/dev/null; exit 0

# Now copy the real source and build for real
COPY . .

# Touch main.rs so Cargo knows to recompile the binary
RUN touch crates/api/src/main.rs

RUN cargo build --release --bin agrimarket

# ── Stage 2: Runtime (tiny Debian image) ─────────────────────────────────────
FROM debian:bookworm-slim

WORKDIR /app

# Runtime deps only (no Rust toolchain)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy compiled binary from builder
COPY --from=builder /app/target/release/agrimarket .

# Render assigns PORT dynamically — app must read this


CMD ["./agrimarket"]

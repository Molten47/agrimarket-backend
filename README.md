# AgriMarket

Farm-to-table marketplace — connecting rural UK farmers directly to consumers.

**Portfolio project. Production mindset.**

---

## Stack

| Layer      | Technology                                          |
|------------|-----------------------------------------------------|
| Backend    | Rust · Axum · SQLx                                  |
| Database   | PostgreSQL 16 · pg_cron · Redis 7                   |
| Frontend   | React · Vite · TypeScript · TailwindCSS · shadcn/ui |
| Payments   | Stripe (PaymentIntents)                             |
| Email      | Resend                                              |
| Hosting    | Supabase (DB + Edge Functions) · Fly.io (Rust API)  |
| Containers | Docker · Docker Compose (local dev)                 |

---

## Project Structure

```
agrimarket/
├── backend/
│   ├── Cargo.toml             # Workspace root
│   └── crates/
│       ├── api/               # Axum server, routes, middleware
│       ├── auth/              # JWT RS256, refresh token chains
│       ├── core/              # Shared models, config, error, DB pool
│       ├── mailer/            # Resend email templates
│       ├── payment/           # Stripe PaymentIntents, webhooks
│       └── ws/                # WebSocket notification hub
├── frontend/                  # React/Vite app (Phase 3)
├── migrations/                # SQLx migration files (run in order)
│   ├── 001_extensions_and_enums.sql
│   ├── 002_tables.sql
│   ├── 003_indexes.sql
│   └── 004_cron_jobs.sql
├── scripts/
│   └── gen_keys.sh            # RS256 key generation
├── docker-compose.yml
├── .env.example
└── .gitignore
```

---

## Local Development Setup

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Docker Desktop](https://www.docker.com/products/docker-desktop/)
- [sqlx-cli](https://github.com/launchbadge/sqlx/tree/main/sqlx-cli)

```bash
cargo install sqlx-cli --no-default-features --features postgres
```

### 1. Clone and configure

```bash
git clone https://github.com/yourusername/agrimarket
cd agrimarket
cp .env.example .env
```

### 2. Generate RS256 keys

```bash
bash scripts/gen_keys.sh
# Prints JWT_PRIVATE_KEY_B64 and JWT_PUBLIC_KEY_B64 — paste into .env
```

### 3. Start Docker services

```bash
docker compose up -d
# Migrations run automatically via docker-entrypoint-initdb.d

# Optional: start pgAdmin at http://localhost:5050
docker compose --profile tools up -d
```

### 4. Verify migrations

```bash
psql postgresql://agrimarket:agrimarket_dev@localhost:5432/agrimarket \
  -c "\dt"
# Should list all 11 tables
```

### 5. Run the API server

```bash
cd backend
cargo run -p api
# Server starts at http://localhost:8080
# Health check: GET http://localhost:8080/health
```

---

## Database Schema

See `migrations/002_tables.sql` for the full schema.

**11 tables:** `farmers`, `refresh_tokens`, `categories`, `products`, `stock`,
`cart`, `cart_items`, `orders`, `order_items`, `tracking`, `notifications`

**Key design decisions:**
- SDA pipeline: Products ⊃ Stock ⊃ Orders — enforced via FK constraints
- Guest checkout — no account required for consumers
- `order_key` idempotency — duplicate POSTs never double-charge
- Price snapshots in `order_items.unit_price_gbp`
- Soft deletes only (`is_deleted` flag, never `DELETE`)
- 7 pg_cron jobs for background maintenance

---

## Build Phases

- [x] Phase 1 — Foundation: Docker, migrations, Rust workspace scaffold
- [ ] Phase 2 — Backend core: Auth, Products, Orders, Payments
- [ ] Phase 3 — Frontend: React/Vite, Farmer dashboard, Consumer checkout
- [ ] Phase 4 — Integrations: Stripe webhooks, Resend, WebSockets, Cron

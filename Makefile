# AgriMarket — Developer convenience commands
# Usage: make <target>

.PHONY: up down logs db-shell redis-shell migrate run check fmt

# ── Docker ────────────────────────────────────────────────────────────────────

up:
	docker compose up -d
	@echo "✅ Postgres running at localhost:5432"
	@echo "✅ Redis running at localhost:6379"

up-tools:
	docker compose --profile tools up -d
	@echo "✅ pgAdmin running at http://localhost:5050"

down:
	docker compose down

logs:
	docker compose logs -f

# ── Database ──────────────────────────────────────────────────────────────────

db-shell:
	docker exec -it agrimarket_postgres psql -U agrimarket -d agrimarket

redis-shell:
	docker exec -it agrimarket_redis redis-cli -a agrimarket_dev

# Check pg_cron job health
cron-status:
	docker exec -it agrimarket_postgres psql -U agrimarket -d agrimarket \
	  -c "SELECT jobname, status, return_message, start_time FROM cron.job_run_details ORDER BY start_time DESC LIMIT 20;"

# ── Rust ──────────────────────────────────────────────────────────────────────

run:
	cd backend && cargo run -p api

check:
	cd backend && cargo check --workspace

fmt:
	cd backend && cargo fmt --all

clippy:
	cd backend && cargo clippy --workspace -- -D warnings

test:
	cd backend && cargo test --workspace

# ── Keys ──────────────────────────────────────────────────────────────────────

gen-keys:
	bash scripts/gen_keys.sh

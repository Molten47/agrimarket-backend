-- Migration: 001_extensions_and_enums
-- AgriMarket — Postgres extensions and custom enum types

-- ── Extensions ────────────────────────────────────────────────────────────────
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
CREATE EXTENSION IF NOT EXISTS "pg_trgm";   -- for ILIKE search on product names
CREATE EXTENSION IF NOT EXISTS "pg_cron";   -- for scheduled maintenance jobs

-- ── Enums ─────────────────────────────────────────────────────────────────────
CREATE TYPE stock_status AS ENUM (
    'in_stock',
    'low_stock',
    'out_of_stock'
);

CREATE TYPE order_status AS ENUM (
    'pending',
    'confirmed',
    'processing',
    'dispatched',
    'delivered',
    'cancelled'
);

CREATE TYPE payment_status AS ENUM (
    'pending',
    'paid',
    'failed',
    'refunded'
);

CREATE TYPE notification_channel AS ENUM (
    'email',
    'websocket',
    'push'
);

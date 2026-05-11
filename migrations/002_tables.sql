-- Migration: 002_tables
-- AgriMarket — all 11 tables in FK dependency order
-- British conventions: county/postcode, GBP in numeric(10,2), UTC timestamps

-- ── 1. FARMERS ────────────────────────────────────────────────────────────────
CREATE TABLE farmers (
    id            UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    email         VARCHAR(254) NOT NULL UNIQUE,
    password_hash TEXT         NOT NULL,
    farm_name     VARCHAR(120) NOT NULL,
    full_name     VARCHAR(120) NOT NULL,
    phone         VARCHAR(20),
    county        VARCHAR(80)  NOT NULL,
    postcode      VARCHAR(10)  NOT NULL,
    bio           TEXT,
    is_active     BOOLEAN      NOT NULL DEFAULT true,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- ── 2. REFRESH_TOKENS ─────────────────────────────────────────────────────────
-- family_id groups all tokens in a rotation chain.
-- Seeing a reused token → revoke entire family (compromise detection).
CREATE TABLE refresh_tokens (
    id          UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    farmer_id   UUID        NOT NULL REFERENCES farmers(id) ON DELETE CASCADE,
    token_hash  TEXT        NOT NULL UNIQUE,
    family_id   UUID        NOT NULL,
    is_revoked  BOOLEAN     NOT NULL DEFAULT false,
    expires_at  TIMESTAMPTZ NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at  TIMESTAMPTZ
);

-- ── 3. CATEGORIES ─────────────────────────────────────────────────────────────
-- Self-referencing for two-level hierarchy: Poultry → Chicken, Turkey, Quail
CREATE TABLE categories (
    id          UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    name        VARCHAR(80) NOT NULL,
    slug        VARCHAR(80) NOT NULL UNIQUE,
    parent_id   UUID        REFERENCES categories(id) ON DELETE SET NULL,
    description TEXT,
    is_active   BOOLEAN     NOT NULL DEFAULT true
);

-- ── 4. PRODUCTS ───────────────────────────────────────────────────────────────
CREATE TABLE products (
    id             UUID            PRIMARY KEY DEFAULT uuid_generate_v4(),
    farmer_id      UUID            NOT NULL REFERENCES farmers(id) ON DELETE CASCADE,
    category_id    UUID            NOT NULL REFERENCES categories(id),
    name           VARCHAR(160)    NOT NULL,
    slug           VARCHAR(160)    NOT NULL UNIQUE,
    description    TEXT,
    price_per_unit NUMERIC(10, 2)  NOT NULL CHECK (price_per_unit >= 0),
    unit           VARCHAR(30)     NOT NULL,  -- kg, dozen, bunch, litre
    is_active      BOOLEAN         NOT NULL DEFAULT true,
    is_deleted     BOOLEAN         NOT NULL DEFAULT false,   -- soft delete
    created_at     TIMESTAMPTZ     NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ     NOT NULL DEFAULT now()
);

-- ── 5. STOCK ──────────────────────────────────────────────────────────────────
-- 1:1 with products. A product MUST have a stock row (enforced in app layer).
CREATE TABLE stock (
    id                   UUID           PRIMARY KEY DEFAULT uuid_generate_v4(),
    product_id           UUID           NOT NULL UNIQUE REFERENCES products(id) ON DELETE CASCADE,
    quantity_available   NUMERIC(10, 3) NOT NULL DEFAULT 0 CHECK (quantity_available >= 0),
    quantity_reserved    NUMERIC(10, 3) NOT NULL DEFAULT 0 CHECK (quantity_reserved >= 0),
    low_stock_threshold  NUMERIC(10, 3) NOT NULL DEFAULT 5,
    stock_status         stock_status   NOT NULL DEFAULT 'out_of_stock',
    updated_at           TIMESTAMPTZ    NOT NULL DEFAULT now()
);

-- ── 6. CART ───────────────────────────────────────────────────────────────────
-- Guest carts identified by session_key (frontend UUID stored in localStorage).
-- expires_at enforced by pg_cron expire_carts job.
CREATE TABLE cart (
    id          UUID        PRIMARY KEY DEFAULT uuid_generate_v4(),
    session_key VARCHAR(64) NOT NULL UNIQUE,
    expires_at  TIMESTAMPTZ NOT NULL DEFAULT (now() + INTERVAL '7 days'),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── 7. CART_ITEMS ─────────────────────────────────────────────────────────────
-- No quantity reservation here — reservation happens on ORDER placement.
CREATE TABLE cart_items (
    id         UUID           PRIMARY KEY DEFAULT uuid_generate_v4(),
    cart_id    UUID           NOT NULL REFERENCES cart(id) ON DELETE CASCADE,
    product_id UUID           NOT NULL REFERENCES products(id) ON DELETE CASCADE,
    quantity   NUMERIC(10, 3) NOT NULL CHECK (quantity > 0),
    added_at   TIMESTAMPTZ    NOT NULL DEFAULT now(),
    UNIQUE (cart_id, product_id)
);

-- ── 8. ORDERS ─────────────────────────────────────────────────────────────────
-- order_key is client-generated UUID — duplicate POST → 409, never double-charges.
-- SDA enforced: stock_id FK means no order without a valid stock row.
CREATE TABLE orders (
    id                UUID           PRIMARY KEY DEFAULT uuid_generate_v4(),
    stock_id          UUID           NOT NULL REFERENCES stock(id),
    order_key         VARCHAR(64)    NOT NULL UNIQUE,
    guest_email       VARCHAR(254)   NOT NULL,
    guest_phone       VARCHAR(20),
    delivery_address  TEXT           NOT NULL,
    delivery_county   VARCHAR(80)    NOT NULL,
    delivery_postcode VARCHAR(10)    NOT NULL,
    order_status      order_status   NOT NULL DEFAULT 'pending',
    payment_status    payment_status NOT NULL DEFAULT 'pending',
    payment_method    VARCHAR(30),
    payment_reference VARCHAR(120),
    total_amount_gbp  NUMERIC(10, 2) NOT NULL CHECK (total_amount_gbp >= 0),
    placed_at         TIMESTAMPTZ    NOT NULL DEFAULT now(),
    updated_at        TIMESTAMPTZ    NOT NULL DEFAULT now()
);

-- ── 9. ORDER_ITEMS ────────────────────────────────────────────────────────────
-- unit_price_gbp is a SNAPSHOT — price changes after order don't affect history.
CREATE TABLE order_items (
    id             UUID           PRIMARY KEY DEFAULT uuid_generate_v4(),
    order_id       UUID           NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_id     UUID           NOT NULL REFERENCES products(id),
    quantity       NUMERIC(10, 3) NOT NULL CHECK (quantity > 0),
    unit_price_gbp NUMERIC(10, 2) NOT NULL,
    subtotal_gbp   NUMERIC(10, 2) NOT NULL
);

-- ── 10. TRACKING ─────────────────────────────────────────────────────────────
-- Append-only — never UPDATE rows. Each checkpoint = new INSERT.
CREATE TABLE tracking (
    id             UUID           PRIMARY KEY DEFAULT uuid_generate_v4(),
    order_id       UUID           NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    status         VARCHAR(30)    NOT NULL,   -- mirrors order_status values
    location_label VARCHAR(120),
    lat            NUMERIC(9, 6),
    lng            NUMERIC(9, 6),
    event_time     TIMESTAMPTZ    NOT NULL DEFAULT now()
);

-- ── 11. NOTIFICATIONS ────────────────────────────────────────────────────────
-- farmer_id nullable → guest notification via recipient_email.
-- payload JSONB stores Resend template variables.
CREATE TABLE notifications (
    id              UUID                 PRIMARY KEY DEFAULT uuid_generate_v4(),
    farmer_id       UUID                 REFERENCES farmers(id) ON DELETE SET NULL,
    order_id        UUID                 REFERENCES orders(id)  ON DELETE SET NULL,
    channel         notification_channel NOT NULL DEFAULT 'email',
    recipient_email VARCHAR(254),
    event_type      VARCHAR(60)          NOT NULL,
    payload         JSONB                NOT NULL DEFAULT '{}',
    is_sent         BOOLEAN              NOT NULL DEFAULT false,
    sent_at         TIMESTAMPTZ,
    created_at      TIMESTAMPTZ          NOT NULL DEFAULT now()
);

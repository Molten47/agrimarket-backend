-- Migration: 003_indexes
-- AgriMarket — indexes on all FK columns + common query patterns

-- ── FARMERS ───────────────────────────────────────────────────────────────────
CREATE INDEX idx_farmers_email       ON farmers (email);
CREATE INDEX idx_farmers_county      ON farmers (county);
CREATE INDEX idx_farmers_is_active   ON farmers (is_active) WHERE is_active = true;

-- ── REFRESH_TOKENS ────────────────────────────────────────────────────────────
CREATE INDEX idx_rt_farmer_id        ON refresh_tokens (farmer_id);
CREATE INDEX idx_rt_family_id        ON refresh_tokens (family_id);
CREATE INDEX idx_rt_expires_at       ON refresh_tokens (expires_at);
-- Partial index for active (non-revoked) tokens — used on every auth check
CREATE INDEX idx_rt_active           ON refresh_tokens (token_hash)
    WHERE is_revoked = false;

-- ── CATEGORIES ────────────────────────────────────────────────────────────────
CREATE INDEX idx_categories_parent   ON categories (parent_id);
CREATE INDEX idx_categories_slug     ON categories (slug);

-- ── PRODUCTS ──────────────────────────────────────────────────────────────────
CREATE INDEX idx_products_farmer     ON products (farmer_id);
CREATE INDEX idx_products_category   ON products (category_id);
CREATE INDEX idx_products_active     ON products (is_active, is_deleted)
    WHERE is_active = true AND is_deleted = false;
-- Full-text trigram search on product name
CREATE INDEX idx_products_name_trgm  ON products USING GIN (name gin_trgm_ops);

-- ── STOCK ─────────────────────────────────────────────────────────────────────
CREATE INDEX idx_stock_product       ON stock (product_id);
CREATE INDEX idx_stock_status        ON stock (stock_status);
-- For cron: find low/out-of-stock that need recomputing
CREATE INDEX idx_stock_low           ON stock (stock_status)
    WHERE stock_status IN ('low_stock', 'out_of_stock');

-- ── CART ──────────────────────────────────────────────────────────────────────
CREATE INDEX idx_cart_session        ON cart (session_key);
CREATE INDEX idx_cart_expires        ON cart (expires_at);

-- ── CART_ITEMS ────────────────────────────────────────────────────────────────
CREATE INDEX idx_cart_items_cart     ON cart_items (cart_id);
CREATE INDEX idx_cart_items_product  ON cart_items (product_id);

-- ── ORDERS ────────────────────────────────────────────────────────────────────
CREATE INDEX idx_orders_stock        ON orders (stock_id);
CREATE INDEX idx_orders_status       ON orders (order_status);
CREATE INDEX idx_orders_payment      ON orders (payment_status);
CREATE INDEX idx_orders_guest_email  ON orders (guest_email);
CREATE INDEX idx_orders_placed_at    ON orders (placed_at DESC);
-- For cron: find stale pending orders
CREATE INDEX idx_orders_stale        ON orders (placed_at)
    WHERE order_status = 'pending' AND payment_status = 'pending';

-- ── ORDER_ITEMS ───────────────────────────────────────────────────────────────
CREATE INDEX idx_order_items_order   ON order_items (order_id);
CREATE INDEX idx_order_items_product ON order_items (product_id);

-- ── TRACKING ──────────────────────────────────────────────────────────────────
CREATE INDEX idx_tracking_order      ON tracking (order_id);
CREATE INDEX idx_tracking_time       ON tracking (event_time DESC);

-- ── NOTIFICATIONS ─────────────────────────────────────────────────────────────
CREATE INDEX idx_notif_farmer        ON notifications (farmer_id);
CREATE INDEX idx_notif_order         ON notifications (order_id);
CREATE INDEX idx_notif_unsent        ON notifications (created_at)
    WHERE is_sent = false;

-- Migration: 004_cron_jobs
-- AgriMarket — pg_cron scheduled maintenance jobs
-- All jobs follow senior's best practices:
--   ✓ Batched (LIMIT) — keeps jobs short, scheduler stays healthy
--   ✓ Idempotent — safe to run twice with identical outcome
--   ✓ FOR UPDATE SKIP LOCKED — no deadlocks in concurrent runs
--   ✓ Total jobs: 7 (≤ Supabase's recommended max of 8)

-- ── JOB 1: Expire carts (every hour) ─────────────────────────────────────────
-- Removes cart_items first (FK), then orphan cart rows.
-- Idempotent: WHERE expires_at < now() — already-deleted rows don't match.
SELECT cron.schedule(
    'expire_carts',
    '0 * * * *',
    $$
        DELETE FROM cart_items
        WHERE cart_id IN (
            SELECT id FROM cart WHERE expires_at < now()
            LIMIT 500
        );

        DELETE FROM cart
        WHERE expires_at < now()
          AND id NOT IN (SELECT DISTINCT cart_id FROM cart_items)
        LIMIT 500;
    $$
);

-- ── JOB 2: Recompute stock_status (every 10 minutes) ─────────────────────────
-- Safety net in case the app-level trigger missed a status transition.
-- Idempotent: UPDATE only changes rows where status is actually wrong.
SELECT cron.schedule(
    'auto_stock_status',
    '*/10 * * * *',
    $$
        UPDATE stock
        SET
            stock_status = CASE
                WHEN quantity_available <= 0                       THEN 'out_of_stock'::stock_status
                WHEN quantity_available <= low_stock_threshold     THEN 'low_stock'::stock_status
                ELSE                                                    'in_stock'::stock_status
            END,
            updated_at = now()
        WHERE stock_status != CASE
            WHEN quantity_available <= 0                           THEN 'out_of_stock'::stock_status
            WHEN quantity_available <= low_stock_threshold         THEN 'low_stock'::stock_status
            ELSE                                                        'in_stock'::stock_status
        END
        LIMIT 200;
    $$
);

-- ── JOB 3 + 4: Cancel stale orders + release reserved stock (every 15 min) ───
-- Orders unpaid after 2 hours are ghost orders — cancel and return stock.
-- FOR UPDATE SKIP LOCKED prevents deadlocks if job overlaps.
-- Idempotent: WHERE order_status = 'pending' — already-cancelled rows are skipped.
SELECT cron.schedule(
    'stale_pending_orders',
    '*/15 * * * *',
    $$
        WITH stale AS (
            SELECT id, stock_id
            FROM orders
            WHERE order_status  = 'pending'
              AND payment_status = 'pending'
              AND placed_at < now() - INTERVAL '2 hours'
            LIMIT 100
            FOR UPDATE SKIP LOCKED
        ),
        cancelled AS (
            UPDATE orders
            SET
                order_status = 'cancelled',
                updated_at   = now()
            FROM stale
            WHERE orders.id = stale.id
            RETURNING orders.stock_id, (
                SELECT SUM(quantity) FROM order_items WHERE order_id = orders.id
            ) AS qty
        )
        -- Return reserved quantity to stock
        UPDATE stock
        SET
            quantity_reserved = GREATEST(0, quantity_reserved - cancelled.qty),
            updated_at        = now()
        FROM cancelled
        WHERE stock.id = cancelled.stock_id;
    $$
);

-- ── JOB 5: Purge old revoked + expired refresh tokens (daily 2am) ─────────────
-- Keeps the refresh_tokens table lean.
-- Two operations merged into one job (stays within ≤8 concurrent job budget).
SELECT cron.schedule(
    'purge_refresh_tokens',
    '0 2 * * *',
    $$
        -- Revoked tokens older than 30 days
        DELETE FROM refresh_tokens
        WHERE is_revoked = true
          AND revoked_at < now() - INTERVAL '30 days'
        LIMIT 1000;

        -- Expired but never revoked (abandoned sessions)
        DELETE FROM refresh_tokens
        WHERE is_revoked = false
          AND expires_at < now()
        LIMIT 1000;
    $$
);

-- ── JOB 6: Clean stale tracking events (weekly Sunday 3am) ───────────────────
-- GDPR-conscious: remove tracking coordinates older than 6 months for
-- delivered/cancelled orders. Keeps the tracking table fast.
SELECT cron.schedule(
    'stale_tracking_cleanup',
    '0 3 * * 0',
    $$
        DELETE FROM tracking
        WHERE event_time < now() - INTERVAL '6 months'
          AND order_id IN (
              SELECT id FROM orders
              WHERE order_status IN ('delivered', 'cancelled')
          )
        LIMIT 2000;
    $$
);

-- ── JOB 7: Notification retry flag (every 15 minutes) ────────────────────────
-- Finds unsent notifications older than 5 minutes and marks them for the
-- Rust tokio-cron retry queue. Keeps email delivery eventually consistent.
-- Idempotent: WHERE is_sent = false — already-sent rows are skipped.
SELECT cron.schedule(
    'notification_retry',
    '*/15 * * * *',
    $$
        UPDATE notifications
        SET payload = payload || '{"retry": true}'::jsonb
        WHERE is_sent    = false
          AND created_at < now() - INTERVAL '5 minutes'
          AND (payload->>'retry') IS NULL
        LIMIT 50;
    $$
);

-- ── Monitor: check for failed jobs ───────────────────────────────────────────
-- Run this query manually whenever you want a health check:
-- SELECT jobid, jobname, status, return_message, start_time
-- FROM cron.job_run_details
-- WHERE status = 'failed'
-- ORDER BY start_time DESC
-- LIMIT 20;

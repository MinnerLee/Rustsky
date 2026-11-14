ALTER TABLE orders
    ADD COLUMN wx_payment_started TINYINT NOT NULL DEFAULT 0,
    ADD COLUMN wx_payment_checked_at DATETIME NULL,
    ADD COLUMN wx_transaction_id VARCHAR(32) CHARACTER SET ascii COLLATE ascii_bin NULL,
    ADD COLUMN wx_refund_state VARCHAR(12) CHARACTER SET ascii COLLATE ascii_bin NULL,
    ADD COLUMN wx_refund_checked_at DATETIME NULL,
    ADD UNIQUE KEY uq_wx_transaction (wx_transaction_id),
    ADD KEY idx_wx_refund (wx_refund_state, wx_refund_checked_at);
GRANT UPDATE (wx_payment_started, wx_payment_checked_at, wx_transaction_id, wx_refund_state, wx_refund_checked_at)
    ON rustsky_dev.orders TO 'rustsky_app'@'127.0.0.1';

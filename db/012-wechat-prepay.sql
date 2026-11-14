ALTER TABLE orders
    ADD COLUMN wx_prepay_id VARCHAR(118) CHARACTER SET ascii COLLATE ascii_bin NULL,
    ADD COLUMN wx_prepay_requested_at DATETIME NULL;
GRANT UPDATE (wx_prepay_id, wx_prepay_requested_at) ON rustsky_dev.orders TO 'rustsky_app'@'127.0.0.1';

ALTER TABLE orders
    MODIFY COLUMN address VARCHAR(350) CHARACTER SET utf8mb4 NULL,
    MODIFY COLUMN consignee VARCHAR(50) CHARACTER SET utf8mb4 NULL,
    ADD UNIQUE KEY uq_order_number (number),
    ADD KEY idx_orders_user (user_id, id);
GRANT INSERT ON rustsky_dev.orders TO 'rustsky_app'@'127.0.0.1';
GRANT UPDATE (amount) ON rustsky_dev.orders TO 'rustsky_app'@'127.0.0.1';
GRANT INSERT ON rustsky_dev.order_detail TO 'rustsky_app'@'127.0.0.1';

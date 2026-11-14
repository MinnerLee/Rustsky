ALTER TABLE orders
    ADD COLUMN delivery_started_at DATETIME NULL,
    ADD KEY idx_orders_state_time (status, order_time),
    ADD KEY idx_orders_delivery_time (status, delivery_started_at);
ALTER TABLE order_detail ADD KEY idx_order_detail_order (order_id);
CREATE TABLE order_event (
    id BIGINT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    order_id BIGINT NOT NULL,
    type TINYINT NOT NULL,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    KEY idx_order_event_order (order_id, type, created_at),
    KEY idx_order_event_time (created_at)
);
CREATE TABLE order_event_gate (id TINYINT NOT NULL PRIMARY KEY);
INSERT INTO order_event_gate (id) VALUES (1);
GRANT SELECT, INSERT, DELETE ON rustsky_dev.order_event TO 'rustsky_app'@'127.0.0.1';
GRANT SELECT, UPDATE (id) ON rustsky_dev.order_event_gate TO 'rustsky_app'@'127.0.0.1';
GRANT UPDATE (delivery_started_at) ON rustsky_dev.orders TO 'rustsky_app'@'127.0.0.1';

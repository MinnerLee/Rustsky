ALTER TABLE orders ADD COLUMN payment_simulated TINYINT NOT NULL DEFAULT 0;
GRANT UPDATE (status, pay_status, checkout_time, delivery_time, cancel_time, cancel_reason, rejection_reason, payment_simulated)
    ON rustsky_dev.orders TO 'rustsky_app'@'127.0.0.1';

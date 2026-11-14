ALTER TABLE shopping_cart
    ADD COLUMN item_kind TINYINT GENERATED ALWAYS AS (IF(dish_id IS NULL, 2, 1)) VIRTUAL,
    ADD COLUMN item_id BIGINT GENERATED ALWAYS AS (COALESCE(dish_id, setmeal_id, 0)) VIRTUAL,
    ADD COLUMN flavor_key VARCHAR(50) CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_bin
        GENERATED ALWAYS AS (COALESCE(dish_flavor, '')) VIRTUAL,
    ADD UNIQUE KEY uq_cart_item (user_id, item_kind, item_id, flavor_key);
GRANT INSERT, UPDATE, DELETE ON rustsky_dev.shopping_cart TO 'rustsky_app'@'127.0.0.1';

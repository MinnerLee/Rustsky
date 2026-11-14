GRANT INSERT, UPDATE, DELETE ON rustsky_dev.dish TO 'rustsky_app'@'127.0.0.1';
GRANT INSERT, DELETE ON rustsky_dev.dish_flavor TO 'rustsky_app'@'127.0.0.1';
GRANT UPDATE (status, update_user, update_time) ON rustsky_dev.setmeal TO 'rustsky_app'@'127.0.0.1';

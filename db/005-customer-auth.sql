ALTER TABLE `user` ADD CONSTRAINT uq_user_openid UNIQUE (openid);
GRANT INSERT (openid, create_time) ON rustsky_dev.`user` TO 'rustsky_app'@'127.0.0.1';
GRANT INSERT, UPDATE, DELETE ON rustsky_dev.address_book TO 'rustsky_app'@'127.0.0.1';

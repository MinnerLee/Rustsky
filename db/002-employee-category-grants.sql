GRANT UPDATE (username, name, employee_no, phone, sex, id_number, status, update_time, update_user)
ON rustsky_dev.employee TO 'rustsky_app'@'127.0.0.1';

GRANT INSERT, UPDATE, DELETE ON rustsky_dev.category TO 'rustsky_app'@'127.0.0.1';
GRANT INSERT (username, name, employee_no, password, phone, sex, id_number,
              status, create_time, update_time, create_user, update_user)
    ON rustsky_dev.employee TO 'rustsky_app'@'127.0.0.1';

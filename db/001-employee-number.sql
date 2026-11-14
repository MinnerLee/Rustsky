ALTER TABLE employee
    MODIFY COLUMN password VARCHAR(255) CHARACTER SET utf8 COLLATE utf8_bin NOT NULL,
    ADD COLUMN employee_no VARCHAR(32) CHARACTER SET ascii COLLATE ascii_bin NULL COMMENT '员工工号' AFTER username,
    ADD CONSTRAINT uq_employee_no UNIQUE (employee_no);

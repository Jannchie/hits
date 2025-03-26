-- 创建主表
DROP TABLE IF EXISTS counters CASCADE;
CREATE TABLE IF NOT EXISTS counters (
    key TEXT NOT NULL,
    count INTEGER NOT NULL DEFAULT 0,
    minute_window TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (key, minute_window)
) PARTITION BY HASH (key);

-- 存储过程用于创建分区
DO $$ 
DECLARE
    partition_count INT := 128;  -- 修改为所需的分区数
    i INT;
BEGIN
    FOR i IN 0..(partition_count - 1) LOOP
        EXECUTE format('
            CREATE TABLE IF NOT EXISTS counters_p%s
            PARTITION OF counters
            FOR VALUES WITH (MODULUS %s, REMAINDER %s)
        ', i, partition_count, i);
    END LOOP;
END $$;
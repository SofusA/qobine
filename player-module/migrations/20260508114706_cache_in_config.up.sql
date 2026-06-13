
ALTER TABLE configuration ADD COLUMN cache_directory TEXT;

ALTER TABLE configuration ADD COLUMN cache_ttl_hours INTEGER;

ALTER TABLE configuration ADD COLUMN volume REAL;

drop table volume;

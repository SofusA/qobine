CREATE TABLE IF NOT EXISTS "cache_entries" (
    "path" text primary key not null,
    "last_opened" text not null
);

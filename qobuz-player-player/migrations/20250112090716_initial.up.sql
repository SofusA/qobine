CREATE TABLE IF NOT EXISTS "config" (
	"username"	TEXT UNIQUE,
	"password"	TEXT,
	"user_token"	TEXT,
	"active_secret"	TEXT,
	"app_id"	TEXT
);

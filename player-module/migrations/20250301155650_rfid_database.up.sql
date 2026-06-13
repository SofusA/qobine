CREATE TABLE IF NOT EXISTS "rfid_references" (
	"id"	TEXT UNIQUE NOT NULL,
	"reference_type"	INT NOT NULL,
	"album_id"	TEXT,
	"playlist_id"	INT
);

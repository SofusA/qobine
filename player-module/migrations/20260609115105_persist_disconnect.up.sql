ALTER TABLE configuration ADD COLUMN disconnect_server_url TEXT;
ALTER TABLE configuration ADD COLUMN disconnect_password TEXT;
ALTER TABLE configuration ADD COLUMN device_name TEXT;
ALTER TABLE configuration ADD COLUMN enable_disconnect BOOLEAN NOT NULL DEFAULT 0;
ALTER TABLE configuration ADD COLUMN auto_play BOOLEAN;

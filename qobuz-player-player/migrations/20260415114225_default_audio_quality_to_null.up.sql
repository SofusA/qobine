ALTER TABLE configuration RENAME TO configuration_old;

CREATE TABLE configuration (
    max_audio_quality INTEGER DEFAULT NULL
);

INSERT INTO configuration (max_audio_quality)
SELECT max_audio_quality
FROM configuration_old;

DROP TABLE configuration_old;

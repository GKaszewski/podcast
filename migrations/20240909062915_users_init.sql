CREATE TABLE
    IF NOT EXISTS users (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid () NOT NULL,
        username VARCHAR(32) NOT NULL UNIQUE,
        password VARCHAR(255) NOT NULL,
        is_superuser BOOLEAN NOT NULL DEFAULT FALSE,
        created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
    )
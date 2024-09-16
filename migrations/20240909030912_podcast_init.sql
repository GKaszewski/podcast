CREATE TABLE
    IF NOT EXISTS podcast (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid () NOT NULL,
        title VARCHAR(255) NOT NULL,
        url VARCHAR(255) NOT NULL,
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    );
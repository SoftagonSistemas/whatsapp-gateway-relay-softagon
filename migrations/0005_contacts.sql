CREATE TABLE IF NOT EXISTS contacts (
    session_id TEXT NOT NULL,
    jid TEXT NOT NULL,
    lid TEXT,
    phone_number TEXT,
    full_name TEXT,
    first_name TEXT,
    push_name TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (session_id, jid),
    FOREIGN KEY (session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_contacts_session_lid ON contacts(session_id, lid);
CREATE INDEX IF NOT EXISTS idx_contacts_session_phone ON contacts(session_id, phone_number);

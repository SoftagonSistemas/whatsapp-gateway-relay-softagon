CREATE TABLE IF NOT EXISTS mcp_audit_log (
    id TEXT PRIMARY KEY,
    transport TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    session_id TEXT,
    arguments_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL,
    error_message TEXT,
    duration_ms INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mcp_audit_created ON mcp_audit_log(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_mcp_audit_tool ON mcp_audit_log(tool_name, created_at DESC);

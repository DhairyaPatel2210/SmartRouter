//! Ordered migrations. Each entry runs once; `PRAGMA user_version` records
//! how many have been applied. Never edit a shipped migration, append a new one.

pub const MIGRATIONS: &[&str] = &[
    // 1 — initial schema (docs/SPEC.md → Architecture and data model)
    r#"
    CREATE TABLE agents (
        id TEXT PRIMARY KEY,
        display_name TEXT NOT NULL,
        kind TEXT NOT NULL,                -- paid | open_source
        cli TEXT NOT NULL,
        version TEXT,
        install_path TEXT,
        auth_ok INTEGER NOT NULL DEFAULT 0,
        enabled INTEGER NOT NULL DEFAULT 1,
        supports_local_models INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE runtimes (
        id TEXT PRIMARY KEY,
        display_name TEXT NOT NULL,
        type TEXT NOT NULL,                -- ollama | lmstudio | llamacpp | mlx
        endpoint TEXT NOT NULL,
        models_dir TEXT,
        managed_by_app INTEGER NOT NULL DEFAULT 0,
        running INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE providers (
        id TEXT PRIMARY KEY,
        display_name TEXT NOT NULL,
        type TEXT NOT NULL,
        base_url TEXT,
        key_ref TEXT,                      -- keychain entry name, never the key
        enabled INTEGER NOT NULL DEFAULT 1,
        monthly_cap_usd REAL
    );
    CREATE TABLE models (
        id TEXT PRIMARY KEY,
        display_name TEXT NOT NULL,
        provider_id TEXT NOT NULL,
        runtime_id TEXT,
        name TEXT NOT NULL,
        tier TEXT NOT NULL,                -- local | cheap_cloud | premium
        size_gb REAL,
        mem_needed_gb REAL,
        quant TEXT,
        ctx_len INTEGER,
        tool_calling INTEGER,
        price_in_per_m REAL,
        price_out_per_m REAL,
        path TEXT,
        installed INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX models_provider ON models(provider_id);
    CREATE TABLE workspaces (
        id TEXT PRIMARY KEY,
        path TEXT NOT NULL UNIQUE,
        display_name TEXT NOT NULL,
        is_git INTEGER NOT NULL DEFAULT 0,
        default_mode TEXT,
        library_profile_id TEXT,
        local_only INTEGER NOT NULL DEFAULT 0,
        last_opened_at INTEGER NOT NULL,
        pinned INTEGER NOT NULL DEFAULT 0,
        settings_json TEXT NOT NULL DEFAULT '{}'
    );
    CREATE TABLE library_items (
        id TEXT NOT NULL,
        kind TEXT NOT NULL,                -- skill | agent | rule
        display_name TEXT NOT NULL,
        scope TEXT NOT NULL,               -- global | workspace
        source_path TEXT NOT NULL,
        enabled INTEGER NOT NULL DEFAULT 1,
        targets_json TEXT NOT NULL DEFAULT '[]',
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (kind, id, scope, source_path)
    );
    CREATE TABLE library_profiles (
        id TEXT PRIMARY KEY,
        display_name TEXT NOT NULL,
        item_ids_json TEXT NOT NULL DEFAULT '[]'
    );
    CREATE TABLE runs (
        id TEXT PRIMARY KEY,
        workspace_id TEXT NOT NULL,
        goal TEXT NOT NULL,
        mode TEXT NOT NULL,
        status TEXT NOT NULL,
        started_at INTEGER NOT NULL,
        ended_at INTEGER,
        est_cost_usd REAL NOT NULL DEFAULT 0,
        peak_mem_mb REAL NOT NULL DEFAULT 0,
        library_snapshot_hash TEXT,
        budget_planning INTEGER NOT NULL DEFAULT 0,
        branch TEXT,
        base_ref TEXT,
        summary_json TEXT NOT NULL DEFAULT '{}'
    );
    CREATE INDEX runs_ws ON runs(workspace_id, started_at DESC);
    CREATE TABLE steps (
        id TEXT PRIMARY KEY,
        run_id TEXT NOT NULL,
        idx INTEGER NOT NULL,
        title TEXT NOT NULL,
        class TEXT NOT NULL,               -- high | low | trivial
        kind TEXT NOT NULL DEFAULT 'execute', -- plan | execute | review
        agent_id TEXT,
        model_id TEXT,
        tier TEXT,
        library_agent_id TEXT,
        status TEXT NOT NULL,
        attempts INTEGER NOT NULL DEFAULT 0,
        escalated INTEGER NOT NULL DEFAULT 0,
        route_reason TEXT,
        tokens_in INTEGER NOT NULL DEFAULT 0,
        tokens_out INTEGER NOT NULL DEFAULT 0,
        tokens_estimated INTEGER NOT NULL DEFAULT 0,
        cost_usd REAL NOT NULL DEFAULT 0,
        paid_equiv_usd REAL NOT NULL DEFAULT 0,
        commit_ref TEXT,
        started_at INTEGER,
        ended_at INTEGER,
        detail_json TEXT NOT NULL DEFAULT '{}'
    );
    CREATE INDEX steps_run ON steps(run_id, idx);
    CREATE TABLE events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        run_id TEXT NOT NULL,
        step_id TEXT,
        ts INTEGER NOT NULL,
        type TEXT NOT NULL,                -- stdout | tool_call | file_edit | tokens | error | governor | check | route
        payload_json TEXT NOT NULL
    );
    CREATE INDEX events_step ON events(step_id, id);
    CREATE INDEX events_run ON events(run_id, id);
    CREATE TABLE metrics (
        ts INTEGER NOT NULL,
        target TEXT NOT NULL,
        rss_mb REAL NOT NULL,
        cpu_pct REAL NOT NULL,
        vram_mb REAL
    );
    CREATE INDEX metrics_ts ON metrics(ts);
    CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value_json TEXT NOT NULL
    );
    "#,
];

---
name: write-migration
description: Use when a task needs a database schema migration. Covers naming, reversibility and data safety.
displayName: Write migration
---

# Write migration

## When to use
Any change to a database schema or a data backfill.

## Steps
1. Find the project's migration tool and folder (e.g. `migrations/`, `prisma/migrations`, `alembic/versions`).
2. Create a new, timestamped migration; never edit one that has shipped.
3. Write both `up` and `down` (or document why it can't be reversed).
4. For large tables, avoid locking operations; add columns as nullable, backfill in batches, then add constraints.
5. Run the migration locally and run the test suite.

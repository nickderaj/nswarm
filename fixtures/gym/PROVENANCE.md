# Sanitized v0 gym fixture provenance

`v0-gym-v5.sqlite3` is an empty, sanitized SQLite database. It contains no
rows, owner identifiers, paths, credentials, or private state.

Its DDL was transcribed from the five `MIGRATIONS` entries in the frozen v0
source at ultron commit `2d7052011c17bd028fdae0fdfd521918c11de560`:

`apps/gym/src/gym/db/schema.py`

The source file's SHA-256 is
`3ab8de4524a5d1b222ee65c968362dfaa53f1c76e07a75a9c90a46896744ddc4`.
The SQL preserves the table/index definitions and migration order, sets
`PRAGMA user_version=5`, and adds no v1 columns or tables. The checked-in
`v0-gym-v5.sql` regenerates the SQLite fixture with the system `sqlite3` CLI.
Because SQLite file headers vary across library versions,
`scripts/check_gym_fixture.sh` compares normalized dumps, schema version, and
integrity rather than database bytes.

The v0 baseline intent adapter is a Rust transcription of
`ActivityRepository.log_metric` and the `/weight` handler at that same commit.
CI therefore needs neither the private sibling checkout nor its Python
environment.

The fixed-time Step 2 corpus has an empty nondeterministic-field allow-list.

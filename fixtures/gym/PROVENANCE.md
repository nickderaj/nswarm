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

`log-body-weight-v0-golden.json` was captured out of band by loading the real
`ActivityRepository` from that v0 commit and calling `log_metric` against a
fresh copy of the empty fixture with the committed instant converted through
Python `ZoneInfo("Europe/London")`. The repository implementation file's
SHA-256 is
`877b310be5f381a834e9ff88515a31b62596d523cbad119f8fa37e17e0d18180`.
The captured v0 row stores
`2026-08-29T09:15:30.123456+01:00`, matching `datetime.isoformat()` and the
configured-zone clock in v0. CI layers those committed golden rows onto the
empty frozen-schema snapshot; it does not regenerate expected behavior from v1
SQL or require the private sibling checkout.

The fixed-time Step 2 corpus has an empty nondeterministic-field allow-list.

`parity-corpus.json` extends the sanitized fixed-time corpus to strength,
cardio, and stated-preference writes. Its SQL goldens encode the observable rows
written by frozen v0 `StrengthRepository`, `ActivityRepository`, and
`ReflectionService` contracts against a fresh schema-v5 database. They contain
only synthetic training values. Preference default timestamps are normalized
to the fixed instant by the corpus test; all other fields have an empty
nondeterminism allow-list.

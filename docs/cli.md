# CLI

```text
ilearned add --topic TOPIC --when TEXT --if TEXT --do TEXT --check TEXT
ilearned search [--topic TOPIC] [--text MATCH] [--semantic QUERY]
                 [--limit N] [--offset N] [--deep]
ilearned modify --topic TOPIC --id ID [--when TEXT] [--if TEXT]
                 [--do TEXT] [--check TEXT]
ilearned delete --topic TOPIC --id ID
ilearned promote --topic TOPIC --id ID
ilearned downgrade --topic TOPIC --id ID
ilearned clear (--topic TOPIC | --all)
ilearned serve
```

- Global `--json` output mode wherever it makes sense. Human output is for
  interactive use; JSON output is stable for agent and script use.
- Destructive `delete`/`clear` require explicit confirmation.
- `clear` requires exactly one of `--topic` / `--all`.
- Failures write a structured error to stderr and return a non-zero exit
  code: 0 ok, 1 not-found, 2 invalid input, 3 embedding unavailable,
  4 internal.
- Config: `--db` path / `ILEARNED_DB`, `--bind` / `ILEARNED_BIND`;
  defaults `./ilearned.db` and `127.0.0.1:8787`.

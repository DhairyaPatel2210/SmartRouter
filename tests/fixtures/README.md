# Adapter fixtures

| Adapter | File | Source |
| --- | --- | --- |
| opencode | `opencode/run.jsonl` | **Recorded** from OpenCode 1.18.34 `run --format json` against the fake provider (`orchestrator --fake-provider 18777`). Paths anonymised to `/tmp/ws`. |
| claude | `claude/stream.jsonl` | Synthetic, matching Claude Code 2.1.x `-p --output-format stream-json --verbose` (flags verified with `--help`). Not recorded to avoid spending the user's subscription. |
| cursor | `cursor/stream.jsonl` | Synthetic, per Cursor CLI docs (`cursor-agent -p --output-format stream-json`). CLI not installed on the build machine. |
| codex | `codex/exec.jsonl` | Synthetic, per Codex docs (`codex exec --json`). CLI not installed on the build machine. |
| copilot | `copilot/output.txt` | Synthetic plain-text output. |
| aider | `aider/output.txt` | Synthetic plain-text output. |
| goose | `goose/output.txt` | Synthetic plain-text output. |

Re-record a fixture whenever a CLI's known-good version changes in `catalog/agents.json`.

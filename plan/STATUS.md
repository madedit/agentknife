# STATUS

## Doing
- P1-CORE-003 — coordinator — workflow written (.github/workflows/ci.yml); not yet run: repo has no GitHub remote

## Blocked
- P1-PROV-002 (follow-up) — needs: owner, an Anthropic API key — run `cargo run -p ak-providers --example record_anthropic` to add real recordings under `tests/fixtures/anthropic/recorded/`. Until then, tests use synthetic fixtures built from the documented wire format.

## Done
- P1-CORE-001 — 53e3549 — workspace skeleton, versions pinned 2026-10-20, rust-toolchain.toml generated
- P1-CORE-002 — acdddd6 — xtask ci: fmt, clippy, test, layering, versions, vendor, no-node, wasm
- P1-CORE-004 — 5571829 — CodeMirror 6 vendored via esm.sh (17 modules + 4 esm.sh helpers), import map, CI integrity checks
- P1-I18N-001 — 13f6195 — ak_types::i18n (fluent, en + zh-TW, OS locale + override), xtask i18n parity check
- P1-PROV-001 — aaf8e6e — Provider trait + StreamEvent contract, neutral chat types (ak_types::chat), SSE decoder, MessageAssembler, fixture replay harness
- P1-PROV-002 — (hash in next commit) — Anthropic provider (adaptive thinking + effort, verbatim replay, drop_block safety net, refusal + server fallbacks, eager tool streaming, prompt caching, models API caps), ReqwestTransport, RecordingTransport + record_anthropic example, synthetic fixtures; PROV-001 contract revised (effort, ProviderReplay, Fallback, Refusal, invalid tool args)

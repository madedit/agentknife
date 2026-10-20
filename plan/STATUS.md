# STATUS

## Doing
- P1-CORE-003 — coordinator — workflow written (.github/workflows/ci.yml); not yet run: repo has no GitHub remote

## Blocked
- P1-PROV-002 (follow-up) — needs: owner, an Anthropic API key — run `cargo run -p ak-providers --example record_anthropic` to add real recordings under `tests/fixtures/anthropic/recorded/`. Until then, tests use synthetic fixtures built from the documented wire format.
- P1-PROV-003 (OpenAI part) — needs: owner decision — OpenAI's current models (GPT-6 Astra, GPT-6.1 Sol) only support tool calling through the **Responses API**; GPT-6 Sol/Luna allow tools in Chat Completions only with reasoning_effort "none" (research 2026-10-20). The Chat Completions adapter is therefore not enough for OpenAI agents. Proposed: a new task P1-PROV-005 for an OpenAI Responses adapter. Also unverified: OpenAI's `developer` role and mid-stream error format.
- P1-PROV-003 (follow-up) — needs: a local Ollama/LM Studio (free) — run `AK_MODEL=… cargo run -p ak-providers --example record_openai_compat` to add real recordings. Unverified until then: LM Studio `/api/v1/models` shape, OpenRouter `delta.reasoning` and whether streamed `reasoning_details` need merging.

## Done
- P1-CORE-001 — 53e3549 — workspace skeleton, versions pinned 2026-10-20, rust-toolchain.toml generated
- P1-CORE-002 — acdddd6 — xtask ci: fmt, clippy, test, layering, versions, vendor, no-node, wasm
- P1-CORE-004 — 5571829 — CodeMirror 6 vendored via esm.sh (17 modules + 4 esm.sh helpers), import map, CI integrity checks
- P1-I18N-001 — 13f6195 — ak_types::i18n (fluent, en + zh-TW, OS locale + override), xtask i18n parity check
- P1-PROV-001 — aaf8e6e — Provider trait + StreamEvent contract, neutral chat types (ak_types::chat), SSE decoder, MessageAssembler, fixture replay harness
- P1-PROV-002 — 7e90826 — Anthropic provider (adaptive thinking + effort, verbatim replay, drop_block safety net, refusal + server fallbacks, eager tool streaming, prompt caching, models API caps), ReqwestTransport, RecordingTransport + record_anthropic example, synthetic fixtures; PROV-001 contract revised (effort, ProviderReplay, Fallback, Refusal, invalid tool args)
- P1-PROV-003 — (hash in next commit) — OpenAI-compatible Chat Completions provider with flavors (OpenAI, Ollama, LM Studio, OpenRouter, generic): per-flavor fields, reasoning from reasoning/reasoning_content/reasoning_details/<think> tags, whole or fragmented tool calls, verbatim replay (Ollama reasoning, OpenRouter reasoning_details), tool-result images moved to a user message, capability probes (Ollama /api/show, LM Studio /api/v1/models, OpenRouter /models) that flag models without tools; synthetic fixtures + record_openai_compat example. OpenAI agent support: see Blocked.

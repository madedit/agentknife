# STATUS

## Doing
- P1-CORE-003 — coordinator — workflow written (.github/workflows/ci.yml); not yet run: repo has no GitHub remote

## Blocked
- P1-PROV-002 (follow-up) — needs: owner, a real Anthropic API key — run `cargo run -p ak-providers --example record_anthropic`. The adapter is verified live against an Anthropic-COMPATIBLE local server only (text, tool round trip, vision). Claude-only features are still verified only against synthetic fixtures: adaptive thinking + signature replay, the models API, refusal/fallback.
- P1-PROV-003 (follow-up) — needs: Ollama / LM Studio / OpenRouter access — run `AK_MODEL=… cargo run -p ak-providers --example record_openai_compat`. Verified live on a llama-server-style endpoint (generic flavor). Unverified: the Ollama/LM Studio/OpenRouter capability endpoints, OpenRouter `delta.reasoning` and whether streamed `reasoning_details` need merging.

## Done
- P1-CORE-001 — 53e3549 — workspace skeleton, versions pinned 2026-10-20, rust-toolchain.toml generated
- P1-CORE-002 — acdddd6 — xtask ci: fmt, clippy, test, layering, versions, vendor, no-node, wasm
- P1-CORE-004 — 5571829 — CodeMirror 6 vendored via esm.sh (17 modules + 4 esm.sh helpers), import map, CI integrity checks
- P1-I18N-001 — 13f6195 — ak_types::i18n (fluent, en + zh-TW, OS locale + override), xtask i18n parity check
- P1-PROV-001 — aaf8e6e — Provider trait + StreamEvent contract, neutral chat types (ak_types::chat), SSE decoder, MessageAssembler, fixture replay harness
- P1-PROV-002 — 7e90826 — Anthropic provider (adaptive thinking + effort, verbatim replay, drop_block safety net, refusal + server fallbacks, eager tool streaming, prompt caching, models API caps), ReqwestTransport, RecordingTransport + record_anthropic example, synthetic fixtures; PROV-001 contract revised (effort, ProviderReplay, Fallback, Refusal, invalid tool args)
- P1-PROV-003 — 44d4e46 — OpenAI-compatible Chat Completions provider with flavors (OpenAI, Ollama, LM Studio, OpenRouter, generic): per-flavor fields, reasoning from reasoning/reasoning_content/reasoning_details/<think> tags, whole or fragmented tool calls, verbatim replay (Ollama reasoning, OpenRouter reasoning_details), tool-result images moved to a user message, capability probes (Ollama /api/show, LM Studio /api/v1/models, OpenRouter /models) that flag models without tools; synthetic fixtures + record_openai_compat example. OpenAI agent support: P1-PROV-005 (owner decision 2026-10-20).
- P1-PROV-VERIFY — 362071a — live check against the owner's local server (127.0.0.1:8080, qwen3.8-flash-next-gptq-nvfp4) through both adapters. Real recordings committed under tests/fixtures/*/recorded/. Fixed: the recorder lost exchanges whose body was not fully drained (it now writes on drop); the Anthropic adapter now falls back to defaults when a compatible server lacks GET /v1/models/{id}; generic servers read context/vision from /models (meta.n_ctx, input_modalities).
- P1-AGENT-001 — (hash in next commit) — ak-agent turn loop:
  - model → tools → model; AI-written step labels via an injected `label` argument.
  - PermissionGate + Confirmer hooks.
  - Cancellation: the stream is dropped, and remaining tool calls get "cancelled" results so history stays well-formed.
  - Outcomes: refusal discards partial output; also step limit, max tokens, failure.
  - turn.lock lifecycle + Checkpointer hook (no-op until PROJ-003).
  - ScriptedProvider fake LLM.
  - ak-types: append-only `Conversation` (schema_version 1, checkpoint entries, migration guard), `AgentEvent`, `TurnOutcome`.
  - Cross-provider test (Anthropic history → Ollama).
  - Live run OK through both protocols on the owner's local server (`agent_live` example).

---
name: online-client-lessons
description: mychess-online (M8-3a) - TLS provider choice, keyring 4 API, test timing pitfalls with sockets
metadata:
  type: project
---

- reqwest 0.13 feature `rustls` pulls aws-lc-rs (OpenSSL license). Use `rustls-no-provider` + direct `rustls` 0.23 with `ring` and `install_default()` in `Client::new`; tree then has no aws-lc. Only new license outside deny.toml: `webpki-root-certs` (CDLA-Permissive-2.0).
- keyring 4.2 (feature `v1`) keeps the old `Entry::new(service, user)` API and picks the native store lazily; `Error::BadEncoding` carries secret bytes, so never Display/Debug it.
- `tokio::time::pause` auto-advances while the runtime waits on socket I/O, so socket tests (FakeLichess) use real short times; only pure-logic tests (RequestGate) use `start_paused`.
- clippy lints hit in this workspace: `let_underscore_drop` (use `.ok()`), `err_expect`, `assert_is_empty`, `needless_pass_by_value`, `unused_self` on `fn close(self)` handles (use `drop(self)`).
- `xtask/src/main.rs` and `docs/arquitetura.md` are edited concurrently by other agents: use small exact-string replacements, never rewrite whole files.

**Why:** avoids re-deriving the P2 licence decision and test flakiness.
**How to apply:** touching mychess-online, the TLS stack, or writing socket tests.

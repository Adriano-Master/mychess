---
name: mychess-app-m4-3-engines-test-notes
description: Armadilhas dos testes m4_3 (gerencia de engines, lado do app) - fake binario, saves em voo, perform vs update
metadata:
  type: project
---

- Binario `mychess-fake-engine` nao e construido por `cargo test -p mychess-app`; `tests/m4_3_common::fake_binary()` o acha em `<target>/debug/` (pai de `deps`) e os testes pulam se faltar. Construir antes com `cargo build -p mychess-engine --features testing --bin mychess-fake-engine` no mesmo CARGO_TARGET_DIR.
- Em `update`, so ha `Effect::SaveEngines` com `AppState::with_preferences_path(Some(..))`; e qualquer mudanca seguinte fica em "reenvio" ate responder `EngineMessage::Saved` (testes que contam saves precisam responder o anterior).
- `Started` de `start_engine` ja pede save (last_used): em testes de Quit isso e um save em voo.
- Testes de integracao nao herdam o `cfg_attr(test, allow(assert_is_empty))` do lib: cada arquivo precisa de `#![allow(clippy::assert_is_empty, unwrap_used, expect_used)]`.
- `Hotkey` F2 so e dado a 1a engine; adicionar a 2a depois nao da F3 (so quando nenhuma tecla existe): ambiguidade da spec D427.

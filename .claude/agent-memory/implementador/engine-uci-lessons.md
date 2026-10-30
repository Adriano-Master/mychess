---
name: engine-uci-lessons
description: Armadilhas verificadas no mychess-engine (M4-1): split de duplex, feature testing, D-numbers esgotados, lints
metadata:
  type: project
---

- `tokio::io::split` de um `duplex`: largar só a `WriteHalf` NÃO fecha o stream (a outra metade mantém o Arc); para o par ver EOF é preciso `shutdown().await` (ver `ManualEngine::close`).
- Feature `testing` + `CARGO_BIN_EXE_*`: o crate é dev-dependency de si mesmo com `features = ["testing"]`; `cargo xtask arch` ignora (só `normal,build`).
- Clippy do workspace: `let _ = <Result/Option com Drop>` dispara `let_underscore_drop` (use `.ok();` ou `drop(..)`); `assert!(x.is_empty())` dispara `assert_is_empty`; `push_str(&format!())` dispara `format_push_string`.
- Numeração D-xxx de specs: em 2026-10-22 estavam usados até D606 (m4-* ocupam D400-D497); a próxima livre era D607. Conferir com `rg -o "D[0-9]{3}" docs`.

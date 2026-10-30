---
name: mychess-app-m8-1-play-test-notes
description: Notas dos testes m8_1 (jogo contra engine, app) - tempo real nos eventos, FENs que dao xeque, mutacao em copia, producao mudando junto
metadata:
  type: project
---

- `fixed_now`/`fixed_roll` do `PlayFlow` sao `pub(crate)`: de `tests/` os eventos da engine usam `Instant::now()` real. Passe `at` = `Instant::now() + N s` com margem de 1 s; o teto absoluto por lance (`ceiling`) nao e testavel por fora.
- Parar a engine do jogo: `EngineMessage::Stop(run)` so pausa depois de `EngineMessage::Stopped(run)`; `Exited` pausa na hora.
- FEN `4k3/.../4R3/4K3` com rei preto em e8 e torre em e2 da xeque ilegal (vez do branco): use `3k4/...`.
- Mover peca do outro lado na vez da engine devolve `IllegalMove` (o glue acha o lance na posicao atual), nao `NotYourTurn`; para `NotYourTurn` use um lance legal do lado da engine.
- Mutacao: copiar o repo (tar, sem target/reference/.git/assets) para o scratchpad e aplicar substituicoes de texto com um script Python; ~40 mutantes em ~9 min com `CARGO_BUILD_JOBS=2` reaproveitando o target. Padroes quebram quando o implementador edita o fonte em paralelo.
- A producao (`play/*.rs`, `state/play_flow.rs`) estava sendo editada durante a escrita dos testes (correcoes de revisao): reler o fonte antes de afirmar "bug"; o tempo do lance atravessando pausa (`carried`) e `can_mate` via `has_insufficient_material` mudaram no meio.
- Tripla repeticao: construa casos que distingam roque (torres saindo e voltando, plies 9 x 10) e en passant (c7c5 com peao em d5, depois reis andando).
- Clippy `--all-targets` do crate falha por `large_futures` em `src/bases/browser_tests.rs` (antigo, fora do M8-1); use `--tests` e filtre por `m8_1`.

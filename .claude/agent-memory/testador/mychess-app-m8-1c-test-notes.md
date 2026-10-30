---
name: mychess-app-m8-1c-test-notes
description: Notas dos testes m8_1c (takeback, empate, Store clock) do mychess-app - gerador de partida quieta, livro responde sozinho
metadata:
  type: project
---

- `tests/m8_1c_common/mod.rs` reusa `m8_1_common` via `#[path]` e traz `play_quiet`/`quiet_line`/`quiet_opening` (partida longa sem repeticao, captura ou xeque) e `white_pov` (Score e sempre do ponto de vista das Brancas).
- Com livro `mini_book`, jogador de Brancas: apos 1.e4 a engine responde e5 SOZINHA (lance livre); `opponent_moved` nesse ponto da ilegal.
- DRAW_MIN_PLY compara `moves.len()` (lances desde a posicao inicial), nao o ply da FEN; Brancas aceitam em 40/42, Pretas em 41.
- Spec D1506 diz "arredondado", D1514 "truncado" para o tempo; implementacao trunca (`clock_text`).
- `clippy -D warnings --tests` do crate pode falhar por `report/` de outro agente; rodar sem `-D` e filtrar por `m8_1c`.

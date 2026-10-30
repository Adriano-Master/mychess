---
name: annotate-flow-lessons
description: Armadilhas do M4-6 fase A (anotação automática) em mychess-app: harness de eventos sintéticos, hooks no engine_event, testes alheios quebrados
metadata:
  type: project
---

- O fluxo da anotação é testável sem engine real: `update(Message::Engine(EngineMessage::Event{run, event}))` com `SearchTagged`/`SearchStarted`/`Info`/`BestMove` sintéticos; ao fim da execução `refresh_engines` já devolve a busca do tabuleiro (a engine volta a `analyzing`), então o laço de teste deve parar quando `!annotation_running()`.
- `cargo fmt --all --check` acusa também arquivos de outros implementadores em paralelo (GUI, evalbar); formate só os seus (`rustfmt --edition 2024 <arquivos>` formata filhos de `mod` recursivamente: não rode em `lib.rs`/`state.rs`).
- Python no `Edit` de arquivos compartilhados (`state.rs`) falha em silêncio se o texto mudou por outro implementador: confira com `grep` depois.
- `tests/m4_5_pure.rs`, `tests/m2_2_shortcuts.rs` e o proptest `engines::store::tests::list_round_trips` falharam durante o M4-6 sem relação com ele.
- No `markExercise` do Scid o exercício é a posição DEPOIS do erro (quem joga agora tem o golpe); a segunda linha vem do MultiPV da busca dessa posição.
- UCI `Stop` com a `Search` ainda atrás de um `isready` só zera `pending.search`: nenhum `BestMove` vem. A execução termina na hora no `cancel()` e descarta eventos tardios por `(run, SearchId)` (`Flow::annotate_stale`); teste com `drive(..)` precisa de `SearchTagged`+`SearchStarted` manuais para ter uma busca "iniciada".
- `EngineMessage::CommandDone{run,result}` é construído por testes alheios (m4_3_*): não mude seus campos; use variante nova (`CommandFailed`).

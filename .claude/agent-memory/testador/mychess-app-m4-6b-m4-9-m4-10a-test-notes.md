---
name: mychess-app-m4-6b-m4-9-m4-10a-test-notes
description: Notas dos testes m4_6b (lote SI4), m4_9 (grafico em linha) e m4_10a (nucleo do relatorio) em mychess-app
metadata:
  type: project
---

- m4_6b_batch.rs reusa `m4_6_common::Rig` (so `mod m4_6_common;`) e adiciona `Bt` com runtime tokio: `settle()` executa so `AnnotateBatchStep` via `perform`; `pump()` executa os demais efeitos nao-engine (abrir base). Filtro por `BasesMessage::SetSearchText` + `ListMessage::FilterByText`.
- Bug achado (M4-6b, D1523): `batch_step_done` chama `annotate_launch` e depois encerra o lote se `flow.annotate.is_none()`; quando a partida e pulada por `NothingToAnnotate` (`annotate_abort` -> `batch_advance` ja empilhou o passo), o lote e encerrado como `BatchStopped`. Reproducao: partida SI4 com FEN de lance 30 + `opening_only=Some(5)`.
- `skipped` so e alcancavel com FEN de numero de lance alto + `opening_only`; a partida sem lances entra como anotada (comentario inicial `13:...`).
- `graphs`: `line` e `Some` com 0 pontos quando Auto cai em Score sem avaliacoes (nao `None`).

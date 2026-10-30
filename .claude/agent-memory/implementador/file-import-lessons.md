---
name: file-import-lessons
description: M3-8 (Importar arquivo PGN) - armadilhas verificadas de desenho e de teste
metadata:
  type: project
---

- Job novo da lista = `ListJobKind` + `Effect` + `ListMessage` de resposta + `PendingListJob`; o encerramento (cancela + prazo de 5 s) e o `Busy` vem de graca. Para destino que ainda nao e slot (base nova) nao use `finish_job` (exige o mesmo `source`): leia `flow.list_job` direto como `games_copied`.
- Dialogo de varios arquivos exigiu `Message::FilesPicked` (o `FilePicked` leva um caminho so) + `PickDialog::run_many`; toda `Message`/`Action`/`PickKind` nova obriga listar em `online/assist.rs` (familia) e em `files_flow.rs` (match exaustivo, clippy `match_same_arms`).
- Edicoes por script Python nao passam pelo hook de rustfmt: rode `cargo fmt --all` no fim.
- `cargo clippy -p mychess-gui --all-targets` e `--lib` de teste podem estar quebrados por codigo alheio (mascaras); use `--lib --bins --test '*'` e filtre por caminho.
- Teste de cancelar no meio: thread vigia `control.progress().0 > 0` e chama `cancel()`; PGN de 60 mil partidas basta.

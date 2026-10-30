---
name: tournament-gui-lessons
description: M8-2b GUI do torneio: como testar headless sem engine, onde cada coisa liga no shell, armadilhas
metadata:
  type: project
---

- Torneio vivo em teste da GUI sem processo: `mychess_app::update(Start)` -> achar `Effect::TournamentLoad{request}` -> alimentar `TournamentMessage::Loaded{Ok(Loaded{dir: tempdir..})}` -> `Effect::TournamentPrepare{epoch}`; `Prepared{Err(PrepareFailure)}` com `max_failures=1` leva a `Phase::Decision`. Chamar `update` direto (nao `shell.update`, que descarta os efeitos) e depois `rig.sync()`.
- Testes de i18n/menu antigos (`m4_3_gui_i18n`, `m4_3_gui_keys_menu`) listam arquivos .ftl e itens do menu Ferramentas: item novo exige atualizar ambos.
- `Position`/`BoardCaches` do tabuleiro ao vivo sincronizam com `ui.dialogs.pieces` (campos disjuntos), nao com `ui.pieces()`, para nao brigar com o borrow de `dialogs.tournament`.
- `cargo fmt --all` reformata arquivos de outras sessoes; clippy do crate pode falhar por arquivo alheio (premove.rs em 2026-10-24).

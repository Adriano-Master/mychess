---
name: mychess-gui-m4-5-evalbar-test-notes
description: Como testar barra/semaforo/setas do M4-5 na GUI (pixels, cliques no tabuleiro, Rig) e achados verificados
metadata:
  type: project
---

- Pixels: `render_common::render(element, size)` (escala 2) + `Shot::pixel`; para setas compare com a mesma cena sem setas (diff de pixels) em vez de cores absolutas; `canvas::Frame` mistura com alpha 0,8 (disco numerado tambem, `scale_alpha(1.0)` nao deixa opaco).
- Seta so existe com `BoardWidget::with_engine_arrows(..).into_element()`; clique na ponta exige `accepts_moves = true`; ponta = centroide de `engine_arrow_geometry(..).head`. Seta e2e4 em 800x800 sem coords: cauda y=620 (nao 650), ponta y=480.
- Bug achado: `UiMessage::FlipBoard`/atalho de virar so chama `ui.flip()`; `app.board_flipped` (que alimenta `EvalBarView.flipped`) nunca muda (so `Action::FlipBoard` no app muda, mas o shell intercepta em `run_action`). Testes `turning_the_board_*` em `m4_5_gui_shell.rs` falham ate corrigir.
- `m3_4b_keys_layout::only_next_previous_and_toggle...` falha SEMPRE (nao intermitente) por causa do M4-5: usa `Command+Shift+L` como override livre, agora de ToggleTrafficLights (D823); trocar por `Command+Shift+K`.
- Clippy da lib do gui pode estar quebrado por edicao alheia: use `-- -A missing-docs -A clippy::... --cap-lints warn` e filtre `tests/m4_5`.

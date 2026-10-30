---
name: mychess-gui-m2-6-panes-tabs-test-notes
description: Como testar paineis com x e abas (M2-6) na GUI - clique no centro da aba cai no x, arrasto real com point_at, achados de layout
metadata:
  type: project
---

- `tests/m2_6_gui_common/mod.rs` (helpers) reaproveita `m2_4_common::Rig`. Nunca nomeie o `let rig = rig();` e depois chame `rig()` de novo (sombreia a funcao); use `self::rig()`.
- Celula da aba (`tab_id`) e mais larga que o conteudo e o CENTRO dela cai sobre o botao x da aba: `sim.click(id)` fecha a aba em vez de seleciona-la. Clique em `bounds.x + 12`.
- Arrasto real: `sim.point_at(p)` + `simulate([ButtonPressed])`, `point_at(destino)` + `simulate([ButtonReleased])` no MESMO Simulator; `simulate` usa o cursor do `point_at`, nao o do `CursorMoved`. Pega valida: espaco livre da barra de titulo (grupo de 1: a esquerda do x). Soltar no centro do corpo (`notation-pane`) = fusao; `Edge` = 2 px da borda.
- O widget nunca emite Dropped com pane == target; mande isso a mao no shell e o `pane_grid::State::drop` destroi o layout (nao e defeito alcancavel).
- Achados reais: abas esticam a barra inteira (sem area livre para arrastar grupos de 2+; so a tira de padding de 2 px acima); x das abas some (largura 0) em grupo estreito (<~280 px, ex.: com Tree aberta).
- Mutacao sem tocar producao: copiar o workspace para o scratchpad (`cp -r` sem target/reference) e usar o mesmo CARGO_TARGET_DIR; script `run_mut26.py` troca trechos e roda os testes.

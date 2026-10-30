---
name: mychess-gui-m5-2-tree-panel-test-notes
description: Como testar o painel da arvore (M5-2a) na GUI com fluxo real - TreeRig que drena Tasks, pick_list por posicao, armadilhas
metadata:
  type: project
---

- `tests/m5_2_gui_common/mod.rs` `TreeRig`: `Rig::shell.update` devolve `Task` que o `Rig` descarta; para a busca da arvore rodar, converta com `iced_test::runtime::task::into_stream(task)`, `block_on(collect())` e realimente `Action::Output(msg)` no `shell.update` (laco). Abrir o painel so por mensagem de UI/atalho: abrir por `TreeMessage::Open` no app deixa `show_tree` false e o `sync_tree` manda Close.
- Para ver "Buscando" (searching) mande a mensagem sem drenar a Task (`send_without_running`).
- `pick_list` no simulador: texto selecionado e opcoes NAO sao achaveis por `find`; clique por posicao (a direita do rotulo + ~24 px) e, no menu aberto, `simulate([CursorMoved, ButtonPressed, ButtonReleased])` - sem `CursorMoved` o menu so confirma o item ja selecionado.
- Media de Elo/Perf exige >= 10 partidas com Elo e so existe em SI4 (PGN em memoria nao tem); ECO nao aparece sem arquivo ECO.
- `windows::tree::view(&shell)` e publico: renderiza o painel sozinho (evita colidir com "Elo"/"Year" da lista de partidas, que e revelada ao abrir base).
- Rolagem: bounds do `find` nao mudam apos `WheelScrolled`; compare fingerprints.
- Pixel: `render_common::render` e sempre tema claro, escala 2; achar a barra procurando a maior corrida horizontal da cor (preto 0x1a1a1a) dentro dos bounds da linha (`container.id("tree-row-N")`).
- Cuidado: `export CARGO_TARGET_DIR` nao persiste entre chamadas do Bash; repita em cada comando.

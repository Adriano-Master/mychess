---
name: opening-report-gui-lessons
description: M7-2c (janela do relatorio de abertura) - armadilhas verificadas de teste headless e de camada no shell
metadata:
  type: project
---

- `UiMessage::Key` chega ao `on_key` com `typed = None`: um teste "a tecla `e` nao vai ao jogo" fica vacuo. Use `Message::Dialog(DialogMessage::Key { input, text_focused, typed: Some(c) })` e prove com um rig de controle (sem camada) que a mesma tecla enche o `move_entry`.
- `Simulator::click(id)` falha com `TargetNotVisible` para conteudo abaixo da dobra de um `scrollable`: use uma janela alta (1400x6000) so para cliques; ids em `container(button).id(..)` sao clicaveis, ids em texto nao.
- Sem como espiar o `Task`: `into_stream(task)` + contar `Action::Widget(_)` prova que Ctrl+F pediu foco; `finish_update` junta `Task::batch` de nones, entao `into_stream(..).is_none()` quase nunca vale.
- Camada nova (como o EPD): campo em `dialogs`, `layer_open(app)` barato (sem montar view), entrada em `modal_layer_open` e ramo em `on_key_layers` DEPOIS do `ActiveDialog`. Modelo pesado vai em cache no `sync_*` do `finish_update` (comparar `SharedReport` por ponteiro), nunca no `view`.
- Cache do tabuleiro: `BoardCaches::sync_data` precisa de `&mut caches` e `ui.pieces()` (borrow de todo `ui`): `mem::take` do cache, sincronizar, devolver.
- Testes antigos que listam o menu Ferramentas (`m4_3_gui_keys_menu`, `m9_2a_menus`, `menu_bar.rs`) e o `m4_3_gui_i18n` (lista de `.ftl`) quebram a cada item novo; `menu-tools-epd` ja faltava neles.
- O app NAO valida os lances de um favorito importado de `reports.dat` (linha ilegal so falha ao gerar); nome repetido de favorito diferencia caixa.

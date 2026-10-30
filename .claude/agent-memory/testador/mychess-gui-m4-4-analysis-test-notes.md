---
name: mychess-gui-m4-4-analysis-test-notes
description: Como testar o painel de análise (M4-4 GUI) headless e armadilhas verificadas
metadata:
  type: project
---

- Run sintética no `Rig`: `rig.app_msg(Engine(Start))` + `Started{identity}` + `Event` (sem processo); `analysis::sections`/`section` aceitam `AnalysisView::new` montada à mão (PvLine/HistoryLine com `text` fixo).
- Botões de ícone (Iniciar/Parar, Fechar) não têm texto: clicar por `Point` relativo a textos (`nth_text_bounds` com closure Selector). `rich_text` não é achável por `find`.
- O `mouse_area` do painel emite `Hover(true)` em todo clique de simulador novo: filtrar nas mensagens.
- "+ Todas" só cria irmãs se o cursor tem filhos (D442); no fim da partida só a 1ª linha. Destravar esquece as linhas. `Game::mainline()` não inclui a raiz. `format_seconds(1000)` = "1.00". `Action::all()` exclui a tabela BASE_NAMES.
- Achado: tecla do painel (Enter/dígitos/Espaço) vaza com a caixa Sobre aberta (`on_key` não cobre `show_about`).

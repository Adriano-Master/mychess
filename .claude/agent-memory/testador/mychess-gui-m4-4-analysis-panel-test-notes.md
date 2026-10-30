---
name: mychess-gui-m4-4-analysis-panel-test-notes
description: Como testar o painel de análise (M4-4) no iced_test sem engine e armadilhas verificadas
metadata:
  type: project
---

- `tests/m4_4_gui_common/mod.rs`: run sintética (`Start`+`Started` pelo `rig.app_msg`, eventos `EngineMessage::Event`), `click_msgs`/`right_click`/`middle_click` por bounds de texto, `nth_text_bounds` (seletor closure `FnMut(Candidate)`), `strip_hover`.
- O `mouse_area` do painel emite `Hover(true)` em todo clique de simulador novo: filtrar, senão "botão desabilitado não manda nada" falha.
- Botões só com ícone (Iniciar/Parar, Fechar) não têm texto: clicar por `Point` relativo ao texto vizinho (nome da engine / "+ Move" / pontuação do cabeçalho).
- `rich_text` (PV) não é achável por `find`; links testados varrendo x ao longo da linha. Tooltips NÃO aparecem no `find` (nem com CursorMoved+Redraw); só dá para testar as chaves i18n.
- "+ Todas" no fim da linha (folha) só adiciona a 1ª linha (D442); para testar os 3 modos use partida 1.a3 a6 com cursor no início.
- `PlayMove` no meio da partida abre pergunta (prompt): para criar variação em teste use `common::sample()` + `rig.goto`.
- `format_score(Cp(0))` = "+0.00". Destravar apaga as linhas (D799).
- Achado: `analysis_key` não checa `show_about`: Enter/dígitos/Espaço agem atrás da caixa Sobre (teste ignorado).

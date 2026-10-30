---
name: mask-gui-lessons
description: M5-4c (mascaras de arvore na GUI) - armadilhas verificadas de teste e desenho no iced 0.14
metadata:
  type: project
---

- `TreeView.mask` so existe com mascara aberta; menu e recentes precisam de `app.prefs.mask` (existe sempre). `MaskCtx::of(shell, &view)` junta os dois.
- O dropdown do `iced_aw` NAO aparece no `Simulator` (overlay): teste o menu por `mask::menu::entries(&ctx)`, nao por clique. Os cartoes do `widgets::context_menu` aparecem (e `rig.click("texto")` acha).
- `Simulator::find("texto")` casa o texto INTEIRO de um widget e enxerga a janela toda (inclusive a tabela atras de um modal): para checar resultado de busca/lista, leia o estado do app (`mask_search().hits`), nao `shows`.
- Teste `m2_4_i18n` exige `set("nome"` literal no codigo (laco `for (name, v) in [..]` reprova) e `m5_2_gui_i18n` le o `.ftl` linha a linha: mensagem multilinha precisa abrir `= { $x ->` na primeira linha.
- `MaskKey`/`Marker`/`MaskColor`/`MaskNag` sao enums sem `non_exhaustive`: clippy `match_wildcard_for_single_variants` pede a variante `Other(_)` explicita.
- `TreeMask::insert` devolve "substituiu?" (false = nova): nao use em `assert!`.
- Adicionar item a barra de opcoes do painel da arvore quebra a linha em 900 px: testes com ponto fixo de rolagem precisam mirar `row_bounds("tree-row-0")`.
- Modal nova = variante de `ActiveDialog` (of, box_id, escape, enter, `dialogs::view`, `newly_opened` para foco) + `MaskUi::dialog`; Esc do `text_editor` chega ao `on_key` sem binding proprio.

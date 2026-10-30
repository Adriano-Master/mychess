---
name: mychess-app-m5-4b-mask-contract-notes
description: Notas dos testes de contrato da M5-4b (mascaras de arvore, app) - alvo movel, base de 1 jogo, bug do Deadline duplicado
metadata:
  type: project
---

- Base PGN de **um jogo so** (`open_pgn`) carrega a partida no editor: `PlayOnlyRow`/jogadas fora da linha nao avancam o cursor. Use a STD ou 2+ jogos nos testes de faixa so-da-mascara.
- O implementador revisa `tree_mask_flow.rs` enquanto se testa (D1875-D1879 novos: edicao recusada durante gravacao/busca/pergunta, `Close` recusado depois que o encerramento decidiu, `recent` so com caminhos absolutos). Escreva testes tolerantes a "aceita e pergunta de novo" OU "recusa com Busy"; releia o fluxo antes de concluir.
- `[tree_mask].recent` relativo e descartado com `InvalidValue`; monte TOML com `std::env::temp_dir().join(..)` e escape de `\` e `"`.
- Travar a assistencia nos testes: `Message::Online(OnlineMessage::MarkerRead(Some(MarkerInfo{..})))` fecha o painel da arvore (reabrir com `TreeMessage::Open` apos liberar); `rig.state.online.marker = Some(..)` direto deixa a tabela na tela (testa a visao sem celulas).
- Bug achado (ignorado): `mask_quit_resume` apos gravacao com `then=Quit` nao zera `quit.waiting`/`deadline_armed`; `Deadline(gen)` tardio roda `finish_quit` de novo (Exit duplicado).

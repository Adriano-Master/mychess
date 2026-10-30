---
name: online-gui-lessons
description: M8-3c GUI do jogo online: como testar sem rede, onde liga no shell, armadilhas de iced/clippy encontradas
metadata:
  type: project
---

- Teste headless de partida online: dev-deps `mychess-online` + `serde_json` no GUI (arestas dev nao entram no `xtask arch`); `OnlineGame::from_full(&GameFull)` de JSON e campos publicos de `OnlineState` (`phase`, `link`, `game`, `clock.sync`). `StreamUpdate` tem campos `pub(crate)`: nao da para forjar eventos de stream da GUI. Nunca rodar `Rig::send` com efeito `Effect::Online` (perform abriria rede); use `rig.msg` (descarta efeitos).
- `PremoveSetup.can_premove` e `&'a dyn Fn`: o predicado tem de morar no estado (`OnlineUi::premove_rule`, Box refeita em `sync`), nao numa closure local da view.
- `BoardColumn` e construido por literal em 3 testes: nao adicione campo; ponha a configuracao no `BoardWidget` (`has_premove()`).
- `radio` do iced nao e achavel por texto no `Simulator::find`; teste a mensagem/estado.
- Atalho novo no mapa do app colide com teste de menu que remapeia `NewGame` para `Command+Shift+K`; `K` ficou livre. Menu novo quebra o teste interno de `menu_bar` que lista os menus.
- Clippy do crate pode falhar por arquivo alheio (`tournament_ui.rs:539 match_same_arms`, 2026-10-24).

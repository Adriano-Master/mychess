---
name: mychess-app-m8-3b-online-test-notes
description: Notas dos testes m8_3 (jogo online Lichess, app) - Rig com FakeLichess, esperar pedidos exige bombear eventos, bug do torneio em Loading, armadilhas de JSON
metadata:
  type: project
---

- `tests/m8_3_common/mod.rs` tem o `Rig` (FakeLichess + AppState + `OnlineEvents`). `StreamUpdate` tem campos `pub(crate)`: tudo que vem de stream so se testa pelo fake; `OnlineState::apply` e `Ctx` sao `pub(crate)`, so `update` serve.
- `fake.wait_requests(...)` NAO bombeia os eventos do hub: se a reacao do app (reabrir stream, enviar premove) depende de `update`, o teste trava. Use `rig.wait_game_streams(n)` / `rig.wait_post(path, n)` (pump + checagem).
- `state_with(.., extra)` ja escreve `winc`/`binc`: passar `"winc"` no extra duplica a chave e a linha inteira vira `Unparsed` (o teste so "estoura o tempo"). Linhas `gameState` com chave duplicada nao sao um caso real.
- `mainline().count()` = numero de plies (raiz fora). `Effect::Online` tem so `command` e `hub`.
- Com 0 plies o `until` ja e verdadeiro antes de a linha nova ser aplicada: espere por `wtime`/`btime` do jogo, nao so por plies.
- Achado (M8-3b): `Out::Starting` aborta o torneio via `TournamentMessage::Abort`, que so age em torneio `Live`; um torneio em `Loading` continua e, ao chegar o `Loaded`, emite `TournamentPrepare` (engines) com a partida online no ar (`m8_3_lock::a_game_that_starts_while_a_tournament_loads_...`).
- Mutacao: `scratchpad/mutate.py` (22 mutantes de `online/*.rs` e `state/online_flow.rs`, restaura com `cp` e `cmp`); rodar em background, ~1-2 min por mutante com 9 binarios de teste.

---
name: tournament-core-lessons
description: M8-2a (torneio entre engines) armadilhas verificadas - TOML u64, ids de engine, fake sem wtime, large_futures herdado, EngineConfig sem PartialEq
metadata:
  type: project
---

- TOML so guarda inteiros i64: uma semente `u64` aleatoria > i64::MAX fez o `save_state` falhar em ~50% das execucoes (teste intermitente). Valide/mascare todo `u64` que vai para arquivo TOML (semente, nos).
- `EngineList` comeca os ids em 0 e nunca reusa: para testar "engine sumiu da lista" adicione e remova dois engines antes (ids 0 e 1 ficam livres).
- O `ScriptedEngine`/`mychess-fake-engine` trata `go wtime/btime` como busca infinita (so `depth`/`movetime` terminam). Para `Clock` e *ponder* use `ManualEngine::launch` + tarefa que responde (ver `tournament/driver/tests.rs`, `Fixture`), com `#[tokio::test(start_paused = true)]`.
- `EngineConfig` nao tem `PartialEq`: um `Effect` que precisa iniciar engine leva `EngineEntry` + `RunDefaults` e monta a config no `perform`.
- `Effect`/`Message` exigem `PartialEq`: `Game` nao tem, use `SharedGame`; `Action` do runner nao precisa (nao vira Effect).
- `clippy --all-targets` do mychess-app ja falha com `large_futures` (~16.5 KB) em bases/*_tests e tree_flow, herdado (AppState ~13.5 KB); `--lib` fica limpo. Encaixar `Box::pin` no await de `perform_tournament` e `Box<TournamentFlow>` evitou piorar.
- `perform` precisa de `Box::pin` ao delegar futuros grandes de engine.
- (fix da revisao M8-2a) `perform` virou `Box::pin(perform_inner)`: resolve o large_futures de `--all-targets` (nao era so heranca). `toml` 1.1 NAO falha ao serializar `u64 > i64::MAX`: escreve o literal e o arquivo vira ilegivel; valide relendo o texto (`try_render_preferences`, `render_state`).
- `mychess-engine` entrega placar das BRANCAS; `TournamentGame::reply` converte para quem moveu (D1302). Em testes de runner, `legal_reply` precisa negar para as Pretas.
- Carrossel com cores equilibradas: assento fixo = fantasma quando n e impar + circulante `(j-i) mod m in 1..=(m-1)/2`.
- `PoolInner.closed`: `shutdown` liga e `prepare` desliga (Continue depois de StopAfterGame reabre o pool).

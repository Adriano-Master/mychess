---
name: mychess-app-m8-2a-tournament-test-notes
description: Notas dos testes m8_2 (torneio entre engines, app) - Sim puro, cycles=1 com 2 jogadores e 1 partida, mutacao com backup, achados
metadata:
  type: project
---

- `Tournament` e puro: `tests/m8_2_common` tem `Sim` (responde `Action`) e `Fixture` (engine UCI roteirizada sobre `ManualEngine`, tempo pausado com `start_paused`). Glue: `Effect::TournamentLoad` com `dir: None` iria ao diretorio do usuario; reescreva `dir` para tempdir (o `dir_override` e pub(crate)).
- Com 2 participantes `cycles=1` e UMA partida; use `cycles=2` quando o teste precisa de duas.
- Roteiros curtos: mate do pastor (Brancas), mate do louco (Pretas), cavalos (tripla). Um roteiro de Brancas faz panico se o primeiro lance manual for outro (f2f3).
- Mutar producao: copiar o arquivo, aplicar `python` com `assert`, rodar, restaurar com `cp`; havia outro agente editando `runner.rs`/`tournament_flow.rs` ao mesmo tempo (risco de a restauracao sobrescrever).
- Mutante vivo util: remover `head_to_head` do sort sobrevive a testes de SB comuns; so um caso com SB/vitorias favorecendo o outro o mata (achado por busca em Python).
- Achados da 2a: `make_slug` aceita `..` interno mas `is_safe_slug` recusa (torneio "Wait..." nao retoma); `validate` nao limita `max_retries` a 0..3.

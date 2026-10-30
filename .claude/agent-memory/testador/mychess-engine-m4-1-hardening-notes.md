---
name: mychess-engine-m4-1-hardening-notes
description: Armadilhas dos testes de sessao UCI do mychess-engine (shutdown nao deterministico, proptest com runtime pausado)
metadata:
  type: project
---

`Engine::shutdown` derruba o ultimo handle junto com o sinal de quit; o `select!` da sessao escolhe ao acaso entre `quit` e `commands.recv()==None`, entao o fim e `Stop::Dropped` (sem `Exited`, espera de quit zero) em ~35-50% das vezes (achado de 2026-10-23, tests/m4_1_hardening.rs).
**How to apply:** em testes que precisam de `Exited(Quit)` use `send(Quit)` e nao `shutdown()`; teste de shutdown deve repetir 32-64 rodadas para ser deterministico.
Proptest de sessao: `Builder::new_current_thread().enable_time().start_paused(true)` por caso; `bestmove` vazio e ignorado pela sessao (busca fica aberta), filtrar no gerador.

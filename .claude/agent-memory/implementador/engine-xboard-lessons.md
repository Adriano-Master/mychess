---
name: engine-xboard-lessons
description: Armadilhas do M4-2 (XBoard) em mychess-engine: coalescedor, shutdown vs Quit, paused clock, numeração D
metadata:
  type: project
---

- Corrida shutdown/Quit corrigida (2026-10-23): `shutdown` segura o handle até `done`, sessões usam `biased;` + `gone()` (canal fechado com `quit` levantado = `Stop::Quit`). `Stop::Dropped` (wait ZERO, sem `quit`) agora só ocorre de verdade quando todos os handles somem sem Quit/shutdown.
- O coalescedor mescla um `Info` sem PV (ex.: `stat01`) em um pendente com PV dentro do intervalo; testes que esperam o Info "sem linha" devem usar `info_interval = 0`.
- Engine falsa que fica ticando (search ativo) com `start_paused`: um teste que espera um evento que nunca vem gira a 100% de CPU até o timeout virtual; rode testes suspeitos um a um com `timeout`.
- `CARGO_TARGET_DIR` relativo é relativo ao cwd do cargo (rodar de `crates/x` cria `crates/x/target`); use caminho absoluto.
- Arquivos criados por heredoc no Bash não passam pelo hook do rustfmt: rodar `rustfmt --edition 2024 <arquivos>` ao fim.

---
name: mychess-cli-m3-5-test-notes
description: Armadilhas dos testes do mychess-cli e do sort SI4 (flock vs fork, EventDate +-3 anos, progresso do import, divergencias do Scid)
metadata:
  type: project
---

- Testes que abrem `Si4Writer` varias vezes em threads paralelas que tambem fazem `Command::spawn` podem ver `AlreadyOpen` espurio: o filho herda o descritor com flock entre fork e exec. Repetir o `open` com pausa curta (ver `set_flags` em `crates/mychess-cli/tests/cli_contract.rs`).
- `EventDate` no indice so guarda anos a +-3 do ano da partida e exige ano na data da partida; fixtures/proptest de `eventdate` devem respeitar isso, senao a ordenacao parece errada.
- Progresso do `import` e por bytes lidos em blocos de 1 MiB: com poucas partidas so aparece `import: 100%`. Para cancelar no meio via `CancelOn` (writer que aciona o token), use PGN > 1 MiB (9000 partidas). O `export` emite a cada 100 partidas (400 partidas -> `export: 20%`).
- `sort_games_with_hook` e `FailPoint` sao acessiveis como `mychess_db::si4::sort::...` e `mychess_db::si4::journal::FailPoint`; `hook.reached().len()` da o numero de estagios para varrer falhas.
- Scid `IndexEntry::Compare` result: None=0,White=1,Black=2,Draw=3 e None vira Draw+1 (index.cpp:~720), isto e, empate < sem resultado; o comentario do Scid diz "juntos", o codigo nao. A spec do MyChess diz "juntos".
- Scid `strCompare` (misc.cpp:289) usa `char` com sinal: nomes com byte inicial >= 0x80 ordenam ANTES; `name_order` do MyChess poe por ultimo.
- `import` ignora PlyCount (D349) e reimporta `Site ""` como `?`: round trip de bancos do Scid precisa normalizar essas duas coisas.

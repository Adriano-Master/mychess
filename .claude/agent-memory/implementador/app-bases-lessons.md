---
name: app-bases-lessons
description: Armadilhas do modelo de slots/Clipbase no mychess-app (testes rule65, Message PartialEq, Scid Length, session ao trocar de slot)
metadata:
  type: project
---

- O teste `rule65_the_update_modules_do_no_io` (tests/m2_2_flow.rs) varre todo `src/state/*.rs` (exceto `flow_tests.rs`) atrás de `std::fs` etc. Testes que usam arquivos reais ficam fora de `src/state/` (aqui em `src/bases/state_tests.rs`).
- `Message`/`Effect` exigem `Clone + PartialEq`: bancos/controle de abertura entram por `Arc` comparado com `ptr_eq` (`SharedSource`, `OpenControl`).
- Os testes do M2 codificam o `GameChooser` para PGN com várias partidas; a migração D503 é opt-in por `AppState::open_pgn_in_slots`.
- `Length` do Scid é `(meios-lances + 1) / 2` (2 meios-lances = 1); em testes de jogar lance, navegue ao fim (`Nav::GameEnd`) senão o lance vira pergunta de variação e a partida não fica alterada.
- `Si4Writer` só trabalha em arquivos; para um banco em memória que cresce, `NameBuilder` + `PreparedGame` + `Si4Database::from_parts` (um mini-banco por leitura) funciona (ver `bases/clipbase.rs`).

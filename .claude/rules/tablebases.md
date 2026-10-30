---
paths:
  - "crates/mychess-tablebase/**"
  - "crates/mychess-app/src/tablebase*/**"
  - "crates/mychess-app/src/state/tablebase_flow.rs"
---

# Tablebases (Syzygy local e consulta online)

- Especificação: `docs/specs/m8-8-tablebases.md`; decisão: ADR 0012.
- **Sem `mmap`** no `shakmaty-syzygy` e nenhum `unsafe` no crate: usa-se
  `Tablebase::new()` (`OsFilesystem`). Reabrir isso exige a regra de
  `desempenho-e-unsafe.md` e uma ADR nova.
- Arquivo de tabela é entrada hostil: toda chamada ao `shakmaty-syzygy` fica
  em `catch_unwind` (`Internal`), tamanhos são conferidos antes de aceitar o
  arquivo e a sondagem roda em `spawn_blocking`.
- **Syzygy não tem DTM**: o crate nunca inventa um; a interface mostra DTZ e só
  mostra DTM quando a fonte online o fornece.
- Nenhum tipo do `shakmaty-syzygy` sai de `mychess-tablebase`; a GUI não o
  usa em código de produção (usa `mychess_app::tablebase`). Como
  `dev-dependency`, com a feature `testing`, ele serve às sondas falsas dos
  testes da GUI.
- A conversão WDL/DTZ → categoria fica em funções puras (`convert.rs`) sobre a
  trait `RawTables`, testadas com `Wdl`/`Dtz` feitos à mão; os `real_*` só
  provam a ligação.
- Cliente HTTP: sempre o do `mychess-online` (`Client::get_public`, sem token).
  Nenhum teste automático toca `tablebase.lichess.ovh`; use o `FakeLichess`.
- **Dados do usuário não entram no repositório**: nenhum `.rtbw`/`.rtbz` em
  fixtures. Testes com tabelas reais são `#[ignore = "requires
  MYCHESS_TEST_SYZYGY"]` e leem a pasta da variável de ambiente (as de 3 peças,
  de poucos KB, bastam).
- A consulta online só roda quando o usuário a liga e é **ajuda externa**:
  fica na família `Family::Tablebases` do bloqueio de partidas online.

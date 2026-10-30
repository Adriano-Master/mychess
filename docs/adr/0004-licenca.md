# ADR 0004: Licença GPL-3.0-or-later

- **Status:** aceita pelo dono do projeto em 2026-10-29, ao decidir que o leitor
  de PGN em Rust (tradução de trechos do `pgnparse.cpp`, com o cabeçalho de
  origem) fica como está e que os sons AGPL do Lichess permanecem no projeto
  (ADR 0015) até que algum titular peça a remoção. Reversível: se houver pedido
  de remoção, o trecho ou o asset sai.

## Contexto

- O Scid é distribuído sob a GNU GPL **sem número de versão** em
  `reference/scid_vs_pc-4.27/COPYING`. Pela seção 9 da GPLv2, nesse caso pode-se
  escolher qualquer versão publicada pela FSF.
- Componentes incluídos no Scid vs PC: Polyglot (GPL-2.0 *or later*) e CQL
  (GPL implícita, segundo os autores, em `src/cql/COPYING`).
- O MyChess vai consultar o código do Scid e eventualmente portar algoritmos,
  o que pode torná-lo obra derivada.
- `shakmaty`, `pgn-reader` e `shakmaty-syzygy` são GPL-3.0-or-later (ADR 0003).

## Decisão

Licenciar o MyChess como **GPL-3.0-or-later** (`license` no `Cargo.toml` do
workspace), com o texto da licença no arquivo `LICENSE`.

## Consequências

- Todas as dependências precisam ser compatíveis com a GPLv3; o `cargo deny
  check` (`deny.toml`) aplica a lista de licenças permitidas.
- **Código portado do Scid** (tradução de C++/Tcl para Rust) leva um cabeçalho
  com a origem e o copyright original (ver `.claude/rules/referencia-scid.md`).
  Os créditos ficam no `README.md`.
- **Dados que vêm com o Scid vs PC** (bancos de exemplo, livros `.bin`, arquivo
  de *spelling*) **não** são redistribuídos sem autorização dos autores: o
  `COPYING` pede contato prévio para reutilização.
- Se o dono do projeto preferir outra licença, a ADR 0003 precisa ser
  revista, porque o `shakmaty` obriga GPL-3.0+, e esta ADR será substituída.

## Risco de compatibilidade de licença (registrado em 2026-10-21)

Este registro **não altera o status** desta ADR (continua proposta). Ele
documenta um risco encontrado ao fechar o M1, para o dono do projeto verificar.

- O `src/pgnparse.cpp` do Scid vs PC 4.27 traz apenas "Copyright (c) 2001-2003
  Shane Hudson. All rights reserved", sem nenhuma menção à licença no próprio
  arquivo.
- O `COPYING` do Scid diz que os arquivos são distribuídos "under the terms of
  the Gnu General Public License (GPL)" **sem número de versão**, e anexa o
  texto da GPL **v2** (que contém a seção 9 e o modelo de aviso "version 2 ...
  or (at your option) any later version"). Nenhum arquivo analisado concede,
  de forma explícita, "or later" como termo do autor original.
- O contexto desta ADR assume que, sem número de versão, vale a seção 9 da
  GPLv2 e qualquer versão pode ser escolhida. Essa leitura é razoável, mas é
  uma **interpretação**: o `COPYING` cita o texto da v2 e não diz que o autor
  aceita versões posteriores.
- Portes quase literais de `pgnparse.cpp` existem em
  `crates/mychess-pgn/src/read/{text,header,glyph,movetext}.rs` (cada arquivo
  tem o cabeçalho "Derived from Scid vs PC 4.27, src/pgnparse.cpp"). Se o
  upstream for GPL-2.0-only, esses arquivos **podem não poder ser
  relicenciados** como GPL-3.0-or-later (a GPLv2 e a GPLv3 só se combinam
  quando há "or later").
- **Ação necessária (dono do projeto):** verificar a licença efetiva no
  upstream (repositório e histórico do Scid vs PC, e, se preciso, perguntar aos
  mantenedores atuais e a Shane Hudson). Se for GPL-2.0-only, as opções são
  reescrever esses módulos sem derivar do texto do Scid (a partir só do
  comportamento especificado), obter permissão de relicenciamento, ou
  reavaliar a licença do projeto (o que reabre a ADR 0003, porque o `shakmaty`
  exige GPL-3.0+). Hoje só esses quatro arquivos levam o cabeçalho "Derived from"; portes
  futuros devem ser incluídos na mesma verificação.

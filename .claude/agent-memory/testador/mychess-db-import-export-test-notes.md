---
name: mychess-db-import-export-test-notes
description: Armadilhas verificadas ao testar import/export PGN e buscas do mychess-db (leitura em blocos de 1 MiB, gzip sem flate2, ScidFlags no Stored)
metadata:
  type: project
---

- `GameChunker` le a entrada em blocos de 1 MiB: o progresso da importacao so e parcial se o PGN tiver bem mais de 1 MiB (250 jogos com comentario de 16 KB). Com PGN pequeno o primeiro relatorio ja e (tamanho, tamanho).
- `flate2` nao e dependencia de `mychess-db` (so de `mychess-pgn`); os testes `.pgn.gz` montam gzip "stored" a mao (cabecalho + bloco deflate sem compressao + CRC32 + ISIZE): `gzip_stored` em `tests/common/m3_3b_support.rs`. O leitor detecta gzip pelo conteudo e le membros concatenados.
- `ExportItem::stored` entrega um `Game` sem a tag `ScidFlags` (flags ficam no indice): `WriteOptions::with_scid_flags(true)` nao grava nada sozinho; o chamador precisa `set_extra("ScidFlags", flags.letters())`.
- Busca OR so visita partidas fora do filtro: para exercitar entradas ilegiveis com OR, comece com `filter.fill(0)`.
- Na ordem de execucao, `cargo test -p mychess-db` para no primeiro binario de teste que falha; use `--no-fail-fast` para ver todas as falhas.

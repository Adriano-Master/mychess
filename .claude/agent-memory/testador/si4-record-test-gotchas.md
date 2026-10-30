---
name: si4-record-test-gotchas
description: Armadilhas ao montar registros .sg4 a mao nos testes do mychess-db (tokens decimais, saida do escritor PGN, leitor valida captura propria)
metadata:
  type: project
---

Tokens do fluxo SI4 sao decimais 11-15 (0x0B-0x0F); escrever 0x13/0x14 vira lance de peca. O escritor PGN imprime `1... d5` apos comentario e quebra linhas: normalize espacos antes de comparar movetext. O leitor rejeita captura de peca propria ("capture of an own piece") ja na decodificacao (mais estrito que o Scid); alvo de lance "so de estrutura" deve ser casa vazia. Com D275, variacao que repete o lance vira `1. e4 c5 (1... e5)` (principal primeiro).

**Why:** cada um custou uma falha falsa na suite de contrato M3-1.
**How to apply:** ao escrever novos registros sinteticos em `crates/mychess-db/tests/common/si4_builder.rs`.

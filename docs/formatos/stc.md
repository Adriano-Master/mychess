# Cache da árvore de aberturas (`.stc`)

Arquivo ao lado da base (`nome.stc`, mesmo nome-base de `nome.si4`) com as
últimas árvores de aberturas calculadas, para que a busca lenta não precise
varrer a base de novo. Tudo **big-endian**. O MyChess lê arquivos do Scid vs PC
e grava no mesmo layout (versão 400).

- **Origem no Scid** (relativa a `reference/scid_vs_pc-4.27/`): `src/tree.cpp`
  (`TreeCache::WriteFile` e `ReadFile`, 280-401), `src/movelist.cpp`
  (`writeSimpleMove` e `readSimpleMove`, 147-190), `src/filter.cpp` 244-282
  (`CompressedFilter`) e 320-522 (`packBytemap`, `unpackBytemap`),
  `src/common.h` 197-253 (peças, resultados, constantes), `src/tkscid.cpp`
  15181-15210 e 15428-15432 (uso na busca) e 15402-15418 (recálculo de `score`
  e ECO).
- **Código no MyChess:** `crates/mychess-db/src/tree/stc/` (`format.rs`,
  `bytemap.rs`, `cache.rs`, `policy.rs`, `extras.rs`, `prime.rs`).
- Especificação e decisões: [`../specs/m5-3-cache-stc.md`](../specs/m5-3-cache-stc.md)
  (D1040 a D1065).

## 1. Cabeçalho (22 bytes)

| Offset | Largura | Campo | Significado |
|---|---|---|---|
| 0 | 4 | `magic` | `73 74 63 18` ("stc" e 24, `TREEFILE_MAGIC`) |
| 4 | 2 | `version` | 400 (`SCID_VERSION`); **não validada** na leitura |
| 6 | 4 | `CacheSize` | capacidade do cache que gravou; na leitura substitui a configurada (1 a 100000) |
| 10 | 4 | `NumInUse` | número de entradas; `<= CacheSize` |
| 14 | 4 | `LowestTotal` | menor `totalCount` entre as entradas |
| 18 | 4 | `LowestTotalIndex` | índice da entrada de menor total (a próxima a sair) |

## 2. Entrada (`NumInUse` vezes, em sequência)

| Offset | Largura | Campo | Significado |
|---|---|---|---|
| 0 | 1 | `toMove` | 0 brancas, 1 pretas |
| 1 | 64 | `board` | casas A1 = 0 a H8 = 63; vazio = 7 (`EMPTY`), brancas K = 1, Q = 2, R = 3, B = 4, N = 5, P = 6, pretas = código + 8 |
| 65 | 4 | `moveCount` | 0 a 60 |
| 69 | 4 | `totalCount` | partidas somadas em todos os nós |
| 73 | 71 x `moveCount` | nós | seção 3 |
| depois | 4 | `CFilterSize` | número de partidas que o filtro cobre |
| +4 | 4 | `CFilterCount` | partidas marcadas no filtro |
| +8 | 4 | `CompressedLength` | bytes do filtro comprimido |
| +12 | `CompressedLength` | dados | seção 4 |

A chave de uma entrada é `(toMove, board)`: roque e en passant **não** entram.

## 3. Nó (71 bytes, `writeSimpleMove` mais os contadores)

| Offset | Largura | Campo | Observação |
|---|---|---|---|
| 0 | 1 | `pieceNum` | índice da peça na lista do lado que joga |
| 1 | 1 | `movingPiece` | peça com o bit de cor |
| 2 | 1 | `from` | 65 (`NULL_SQUARE`) = nó `[end]` |
| 3 | 1 | `to` | |
| 4 | 1 | `capturedNum` | índice na lista do adversário |
| 5 | 1 | `capturedPiece` | 7 = nenhuma |
| 6 | 1 | `promote` | 7 = nenhuma; senão 2 a 5 |
| 7 | 1 | reservado | 0 |
| 8 | 1 | `capturedSquare` | difere de `to` no en passant |
| 9 | 1 | `castleFlags` | brancas O-O-O 1, O-O 2, pretas O-O-O 4, O-O 8 (antes do lance) |
| 10 | 1 | `epSquare` | 65 = nenhuma |
| 11 | 2 | `oldHalfMoveClock` | |
| 13 | 8 | `san` | texto C de 8 bytes: **no máximo 7 caracteres** e zeros; `[end]` no fim da partida |
| 21 | 16 | `freq[0..4]` | **ordem do arquivo**: 0 sem resultado, 1 brancas, 2 pretas, 3 empate (`common.h` 200-203). O `TreeNode` do MyChess usa `[brancas, pretas, empate, sem resultado]` |
| 37 | 4 | `total` | |
| 41 | 4 | `score` | brancas, em milésimos |
| 45 | 2 | `ecoCode` | forma de 16 bits do Scid; 0 = nenhum |
| 47 | 4 | `eloCount` | |
| 51 | 4 | `eloSum` | |
| 55 | 4 | `perfCount` | |
| 59 | 4 | `perfSum` | |
| 63 | 4 | `yearCount` | |
| 67 | 4 | `yearSum` | |

As somas (`eloSum`, `perfSum`, `yearSum`) são `unsigned int` no Scid:
**truncadas a 32 bits** se passarem de `u32::MAX`, o que dá um valor errado em
silêncio. O MyChess, ao ler, aceita o valor truncado; ao gravar, **não grava a
entrada** em que alguma soma passe de `u32::MAX` (ela é recomputada por busca
completa; D1064).

## 4. Filtro comprimido (`bytemap`)

Um byte por partida (valor do filtro, 0 = fora). `CompressedLength` bytes:

- Primeiro byte, a **flag**: `0` = o resto é uma cópia (`CompressedLength = n + 1`);
  `1` = empacotado. O compressor usa a cópia quando o empacotado seria maior
  que a entrada.
- Empacotado: fluxo de códigos de 2 bits, **quatro por byte de controle**, do
  menos significativo para o mais, com os bytes de controle **intercalados**
  com os dados. O byte de controle é reservado na saída assim que o anterior
  enche, **antes** dos dados do código que o encheu. O **último** byte de
  controle é sempre escrito (mesmo vazio ou parcial; os códigos não usados
  leem como zeros).

| Código | Significado | Dados após o controle |
|---|---|---|
| 0 | um zero | nenhum |
| 1 | repete o último literal diferente de zero | nenhum |
| 2 | corrida de `>= 9` bytes iguais (`MIN_RLE_LENGTH`) | valor, comprimento `u8`; forma longa para `> 255`: comprimento 0 e depois `u32` big-endian |
| 3 | novo literal diferente de zero | o valor |

Corridas de menos de 9 bytes saem como literais. Quirk (D1059a): o Scid lê
códigos de 2 bits além do fim real, então um tamanho de saída um pouco maior
que o real pode ser aceito se o último byte de controle tem espaço; o MyChess
faz igual. O Scid lê fora do buffer quando o filtro empacotado tem só a flag; o
MyChess rejeita como corrompido. O descompressor exige consumo exato.

## 5. O que o Scid realmente usa ao ler

Só `from`, `to`, `promote`, `san`, `freq`, `total`, os contadores, a posição
(`toMove` e `board`) e o filtro. `Position::DoSimpleMove` **sobrescreve**
`pieceNum`, `capturedPiece`, `capturedSquare`, `castleFlags`, `epSquare` e
`oldHalfMoveClock`; `score` e `ecoCode` são recalculados (`tkscid.cpp`
15402-15418). Logo os campos gravados "por melhor esforço" pelo MyChess
(`pieceNum`, `capturedNum`, `castleFlags`, `epSquare`, relógio) não afetam o
Scid; o único incompatível visível seria o `san` (por isso é gerado na posição
do nó, com en passant correto; D1063). O MyChess ignora na leitura `san`,
`score`, `ecoCode` e os campos de lance além de `from`, `to` e `promote`.

## 6. Limites de leitura

Arquivo de até 512 MiB (`MAX_STC_BYTES`); `CacheSize` em 1 a 100000;
`NumInUse <= CacheSize`; `toMove` 0 ou 1; casas 0 a 14; `moveCount <= 60`;
`from`/`to` < 64 (exceto `from == 65`); `promote` em {7, 2 a 5};
`CFilterSize <= 16 777 214` e `CFilterCount <= CFilterSize`; nenhuma alocação
passa do que resta no arquivo; o filtro comprimido é validado por inteiro.
`LowestTotal`/`LowestTotalIndex` só são aceitos se o índice aponta para uma
entrada com esse total e ele é o mínimo; senão são recalculados. Bytes após as
entradas são ignorados.

## 7. Divergências deliberadas

- **Rejeição de arquivo corrompido**: qualquer campo inválido descarta o arquivo
  inteiro (o Scid lê sem validar).
- **Política de despejo**: o índice do menor total é recalculado sobre todas as
  entradas, com desempate pelo menor índice; o Scid parte do total da entrada
  nova e só difere no empate entre ela e uma antiga de total igual (D1046).
- **Teto de escrita**: o MyChess não grava mais que 256 MiB
  (`MAX_WRITE_BYTES`), mantendo as entradas de maior `totalCount` e ajustando
  `NumInUse`/`LowestTotal*` ao que foi gravado (D1062); a leitura aceita 512 MiB.
- **Somas acima de 32 bits**: entrada omitida (D1064).
- **Escrita**: temporário exclusivo `.nome.pid.n.tmp` no mesmo diretório,
  permissões do destino copiadas, link simbólico resolvido, `rename` depois de
  fechar (D1049, D1062).

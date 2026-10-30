# Formato SI4 v400 (`.si4`, `.sn4`, `.sg4`)

Banco de partidas do Scid vs PC 4.27: três arquivos com o mesmo nome-base. Todos
os inteiros são **big-endian** (`mfile.cpp:305-345`, `misc.h:293-352`). Quando a
ajuda e o código do Scid divergem, os dois são registrados; **o código vence**.

- **Origem no Scid** (relativa a `reference/scid_vs_pc-4.27/`): `src/index.h`
  (cabeçalho, flags, `IndexEntry`), `src/index.cpp` (leitura e gravação),
  `src/namebase.h/.cpp`, `src/gfile.h/.cpp`, `src/game.cpp` (`Encode`/`Decode`,
  4140-5148), `src/position.cpp` (lista de peças), `src/misc.cpp:60-175` (ECO),
  `src/date.h`, `src/matsig.h`, `src/tkscid.cpp` (`sc_base`), `tcl/windows/switcher.tcl`
  (tipos de banco).
- **Código no MyChess:** `mychess-db` (módulo `si4`).
- Especificação da leitura: [`../specs/m3-1-leitura-si4.md`](../specs/m3-1-leitura-si4.md).
  A escrita, as *stored lines*, o diário `.txn`, a compactação e o `.si3` estão
  na seção 11 e em [`../specs/m3-2-escrita-si4.md`](../specs/m3-2-escrita-si4.md).

## 1. Visão geral

| Arquivo | Conteúdo | Tamanho |
|---|---|---|
| `<base>.si4` | cabeçalho de 182 bytes + 1 entrada de 47 bytes por partida | `182 + 47*numGames` |
| `<base>.sn4` | cabeçalho de 36 bytes + nomes de jogadores, eventos, locais e rodadas (front-coding) | variável |
| `<base>.sg4` | registros de partida (lances, variações, comentários, tags extras), em blocos | variável |

Limites: `MAX_GAMES = 16.777.214` (3 bytes; o *autoload* usa 0 como "nenhuma",
`index.h:36-39`); jogadores 1.048.575, eventos 524.287, locais 524.287, rodadas
262.143 nomes distintos (`namebase.h:46-51`); registro de partida de até 131.071
bytes (campo de 17 bits).

## 2. `.si4`: cabeçalho (182 bytes, `index.h:57-71, 927-945`)

| Offset | Larg. | Campo | Significado |
|---|---|---|---|
| 0 | 8 | `magic` | `"Scid.si"` + `0x00` (o byte 7 precisa ser 0) |
| 8 | 2 | `version` | exatamente 400 |
| 10 | 4 | `baseType` | tipo/ícone do banco (seção 6) |
| 14 | 3 | `numGames` | número de partidas |
| 17 | 3 | `autoLoad` | valor bruto (abaixo) |
| 20 | 108 | `description` | texto de até 107 bytes, completado com NUL |
| 128 | 9 | `customFlagDesc[0]` | descrição do flag customizado 1 (8 bytes + NUL) |
| 137 | 9 | `customFlagDesc[1]` | idem 2 |
| 146 | 9 | `customFlagDesc[2]` | idem 3 |
| 155 | 9 | `customFlagDesc[3]` | idem 4 |
| 164 | 9 | `customFlagDesc[4]` | idem 5 |
| 173 | 9 | `customFlagDesc[5]` | idem 6 |

Total: 8+2+4+3+3+108+54 = 182. (Em `.si3` o cabeçalho tem 128 bytes e a entrada
46; fora do escopo da v400.)

- **Magic** diferente → magic inválido. **Versão** `< 400` → arquivo antigo
  (`ERROR_FileVersion`); `> 400` → banco mais novo que o programa
  (`ERROR_OldScidVersion`) (`index.cpp:970-1000`, `common.h:53-54`).
- **Autoload** (`index.h:~659-664`): `GetAutoLoad() = (bruto == 0) ? 1 : bruto - 1`
  e `SetAutoLoad(n)` grava `n + 1`. Logo bruto 0 e 2 = "partida 1"; bruto 1 =
  "nenhuma"; bruto `k >= 2` = "partida `k-1`". Banco novo nasce com bruto 2
  (`index.cpp:853`).
- **Descrição:** bytes sem codificação; termina no primeiro NUL. Os flags
  customizados são `strncpy` de 8 bytes + NUL; o primeiro byte, em minúscula, é a
  letra exibida na lista de partidas (`index.h:672-681`).
- O Scid **não** confere o tamanho do arquivo contra `numGames`
  (`MFile::ReadNBytes` não é verificado). O MyChess rejeita `len < 182` e
  `len < 182 + 47*numGames`.

## 3. `.si4`: entrada (47 bytes, `index.cpp:173-233`, `index.h:578`)

A partida `n` (base 1) começa no offset `182 + 47*(n-1)`.

| Offset | Larg. | Campo | Significado |
|---|---|---|---|
| 0 | 4 | `Offset` | deslocamento do registro no `.sg4` |
| 4 | 2 | `Length_Low` | 16 bits baixos do tamanho do registro |
| 6 | 1 | `Length_High` | bit 7 = bit 16 do tamanho; bits 0-5 = flags customizados 1-6; bit 6 = livre (**preservar**) |
| 7 | 2 | `Flags` | seção 5 |
| 9 | 1 | `WhiteBlack_High` | bits 4-7: bits 16-19 do ID de White; bits 0-3: bits 16-19 do ID de Black |
| 10 | 2 | `WhiteID_Low` | 16 bits baixos |
| 12 | 2 | `BlackID_Low` | idem |
| 14 | 1 | `EventSiteRnd_High` | bits 5-7: bits 16-18 de Event; bits 2-4: bits 16-18 de Site; bits 0-1: bits 16-17 de Round |
| 15 | 2 | `EventID_Low` | |
| 17 | 2 | `SiteID_Low` | |
| 19 | 2 | `RoundID_Low` | |
| 21 | 2 | `VarCounts` | bits 0-3: variações; 4-7: comentários; 8-11: NAGs; 12-15: resultado |
| 23 | 2 | `EcoCode` | código ECO numérico (0 = sem ECO); `Si4Writer::set_eco` e `set_eco_batch` escrevem só estes dois bytes |
| 25 | 4 | `Dates` | bits 0-19: data da partida; bits 20-31: data do evento |
| 29 | 2 | `WhiteElo` | bits 0-11: Elo; bits 12-15: tipo de rating |
| 31 | 2 | `BlackElo` | idem |
| 33 | 4 | `FinalMatSig` | bits 0-23: assinatura de material final; bits 24-31: código de *stored line* |
| 37 | 1 | `NumHalfMoves` | 8 bits baixos do nº de meios-lances |
| 38 | 1 | `HomePawnData[0]` | bits 0-5: nº de mudanças; **bits 6-7: bits 8-9 de `NumHalfMoves`** |
| 39 | 8 | `HomePawnData[1..8]` | até 16 mudanças de peão de casa, em meios-bytes |

Total: 47 (o comentário "48 bytes" de `index.h:~215` está errado).

Larguras dos IDs: White/Black 20 bits (até 1.048.575); Event/Site 19 bits
(524.287); Round 18 bits (262.143).

Derivados:

- **Tamanho:** `Length_Low + ((Length_High & 0x80) << 9)` (17 bits, máx. 131.071).
- **Contagens** (variações, comentários, NAGs; 4 bits): escala não linear
  `{0,1,2,...,10,15,20,30,40,50}[x & 15]` (`index.h:~340`); é aproximada, o valor
  exato não é recuperável. Os flags "tem comentário/variação/NAG" derivam de
  contagem `> 0`.
- **Resultado** (`VarCounts >> 12`, `common.h:197-210`): 0 = `*`, 1 = `1-0`,
  2 = `0-1`, 3 = `1/2-1/2`. Valores 4-15 são inválidos.
- **Data da partida** (20 bits, `date.h:37-47`): `ano<<9 | mês<<5 | dia`; campo
  desconhecido vale 0; ano 0-2047.
- **Data do evento** (12 bits altos de `Dates`, `index.cpp:117-150`):
  `d = Dates >> 20`; dia = `d & 31`; mês = `(d >> 5) & 15`; `off = (d >> 9) & 7`.
  `off == 0` = sem data de evento; senão `ano = anoPartida + off - 4` (−3 a +3).
  Ao gravar, anos fora de ±3 do ano da partida se perdem (`SetEventDate`).
- **Rating:** tipo `0`=Elo, `1`=Rating, `2`=Rapid, `3`=ICCF, `4`=USCF, `5`=DWZ,
  `6`=ECF (`common.h:185` o chama `RATING_BCF`; `game.cpp:180`, `ECF`); 7-15
  reservados. A leitura limita o Elo (12 bits) a 4000 (`MAX_ELO`, `index.cpp:215-216`).
- **ECO** (`misc.cpp:60-175`): `0` = vazio. Básico: `1 + 131*(100*(letra-'A') + dezena*10 + unidade)`.
  Extensão opcional: soma `1 + 5*(letra_minúscula - 'a') + sufixo(0-4)`.
  Decodificar: `c = eco - 1`, básico = `c / 131`, ext = `c % 131`; se `ext > 0`,
  `ext-1`: letra = `/ 5`, dígito = `% 5` (omitido se 0). Letras `A`-`E`.
- **Assinatura de material** (`matsig.h:19-28, 174-185`): 4 bits para peões, 2 bits
  por peça (limitado a 3): BP bit 0, BN 4, BB 6, BR 8, BQ 10, WP 12, WN 16,
  WB 18, WR 20, WQ 22 (o comentário de `MASK_WQ` está errado; a máscara é
  `0x00C00000`).
- **`NumHalfMoves`** = `byte37 | ((byte38 >> 6) << 8)` (10 bits, máx. 1023);
  `HomePawnData[0] = byte38 & 63`.
  **Partidas com `START`:** o `AddMove` atualiza `NumHalfMoves` e `FinalMatSig` a
  cada lance da linha principal (`game.cpp:1022-1026`), mas o refresh final em
  `MakeHomePawnList` (`game.cpp:1395`) é pulado nessas partidas, e os bancos de
  exemplo (`matein1`, `endings`) têm índices velhos nessa classe. Fora dela os dois
  campos conferem com a linha principal em todas as partidas dos 6 bancos. Quem os
  usa deve ignorá-los quando `START`.
- **Home pawn data** (`game.cpp:1379-1460`, `position.cpp:535-557`): `HPSIG` de
  16 bits (bit 15 = a2 ... bit 8 = h2, bit 7 = a7 ... bit 0 = h7), começando em
  `0xFFFF`. Cada mudança grava `log2(hpAntigo - hpNovo)` (0-15) em meio-byte, o
  primeiro no nibble alto. Partida com FEN inicial tem lista vazia. Na leitura os
  9 bytes são preservados crus.

## 4. Verificação das entradas

Só índice e nomes são verificados na abertura. O `.sg4` (bloco, tamanho, formato do registro) é verificado ao ler cada partida.

`Verify` (`index.cpp:95-113`): a entrada é corrompida se um ID de nome for
`>= numNames` do tipo. O Scid recusa o banco inteiro ("name corruption"). O
MyChess devolve `Err` com o número da primeira partida corrompida, sem "consertar".

## 5. Flags (`index.h:80-127, 292-322`)

| Bit | Nome | Letra | Onde |
|---|---|---|---|
| 0 | `START` (posição inicial própria) | (derivado) | `Flags` |
| 1 | `PROMO` | (derivado) | `Flags` |
| 2 | `UPROMO` (subpromoção) | (derivado) | `Flags` |
| 3 | `DELETE` | `D` | `Flags` |
| 4 | `WHITE_OP` | `W` | `Flags` |
| 5 | `BLACK_OP` | `B` | `Flags` |
| 6 | `MIDDLEGAME` | `M` | `Flags` |
| 7 | `ENDGAME` | `E` | `Flags` |
| 8 | `NOVELTY` | `N` | `Flags` |
| 9 | `PAWN` | `P` | `Flags` |
| 10 | `TACTICS` | `T` | `Flags` |
| 11 | `KSIDE` | `K` | `Flags` |
| 12 | `QSIDE` | `Q` | `Flags` |
| 13 | `BRILLIANCY` | `!` | `Flags` |
| 14 | `BLUNDER` | `?` | `Flags` |
| 15 | `USER` | `U` | `Flags` |
| 16-21 | `CUSTOM1`-`CUSTOM6` | `1`-`6` | `Length_High` bits 0-5 |

`IDX_NUM_FLAGS = 22`. A string exibida usa a ordem `"DWBMENPTKQ!?U123456"`
(`GetFlagStr`, `index.cpp:344-362`). `CharToFlag` do Scid devolve 0 (= `START`)
para letra desconhecida (`index.cpp:295-320`); o MyChess trata como erro.

## 6. Tipo do banco (`baseType`)

0-31, a ordem da lista `icons` de `switcher.tcl:15-745`; só o número é gravado.

| Nº | Nome | Nº | Nome |
|---|---|---|---|
| 0 | Unknown | 16 | End games |
| 1 | Temporary database | 17 | Openings for White |
| 2 | Clipbase | 18 | Openings for Black |
| 3 | PGN file | 19 | Openings for either color |
| 4 | My games | 20 | Theory: 1.c4 |
| 5 | Large database | 21 | Theory: 1.d4 |
| 6 | Correspondence chess | 22 | Theory: 1.d4 d5 |
| 7 | Computer Chess | 23 | Theory: QGD |
| 8 | Index of games | 24 | Theory: Slav |
| 9 | Player collection | 25 | Theory: 1.d4 Nf6 |
| 10 | Tournament | 26 | Theory: 1.e4 |
| 11 | Internet | 27 | Theory: Sicilian |
| 12 | GM games | 28 | Theory: Sicilian 1.e4 c5 2.Nd3 d6 3.d4 |
| 13 | IM games | 29 | Theory: Caro-Kann |
| 14 | Blitz games | 30 | Theory: French |
| 15 | Tactics | 31 | Theory: Open Games |

A ordem de Tactics, Clipbase e PGN importa (`switcher.tcl:5-12`). Banco novo =
tipo 1; PGN aberto = 3; correspondência = 6. Valor `>= 32` é mostrado com o
ícone 0, mas o valor bruto é preservado.

## 7. `.sn4` (`namebase.cpp:70-250`)

Cabeçalho de 36 bytes:

| Offset | Larg. | Campo |
|---|---|---|
| 0 | 8 | `"Scid.sn"` + `0x00` |
| 8 | 4 | `timeStamp` |
| 12 | 3 | `numNames[player]` |
| 15 | 3 | `numNames[event]` |
| 18 | 3 | `numNames[site]` |
| 21 | 3 | `numNames[round]` |
| 24 | 3 | `maxFrequency[player]` |
| 27 | 3 | `maxFrequency[event]` |
| 30 | 3 | `maxFrequency[site]` |
| 33 | 3 | `maxFrequency[round]` |

(O campo `version` da struct `nameBaseHeaderT` **não** vai para o disco.)

Em seguida, quatro seções na ordem player, event, site, round (`NAME_PLAYER=0`,
`NAME_EVENT=1`, `NAME_SITE=2`, `NAME_ROUND=3`), sem separador. Cada seção tem
`numNames[t]` registros:

| Campo | Largura | Observação |
|---|---|---|
| `id` | 3 bytes se `numNames[t] >= 65536`, senão 2 | ID do nome |
| `frequency` | 3 bytes se `maxFrequency[t] >= 65536`; 2 se `>= 256`; senão 1 | nº de usos "de arquivo" |
| `length` | 1 | comprimento **total** do nome (0-255) |
| `prefixLen` | 1 | **só do 2º registro em diante** em cada seção |
| bytes novos | `length - prefixLen` | o nome é `prefixo(nome_anterior, prefixLen) + sufixo` |

- Os registros estão em ordem alfabética (comparação byte a byte com `char`
  **com sinal**), mas o **ID é explícito**: a posição no arquivo não é o ID.
  "Anterior" é o registro imediatamente anterior **no arquivo**.
- Sem NUL no arquivo. Nomes são bytes crus, sem codificação imposta.
- As frequências do arquivo só valem com `-fast`; no modo normal o Scid as
  recalcula contando as entradas do índice (`recalcNameFrequencies`).
- Validação do MyChess: `prefixLen <= length`, `prefixLen <= len(anterior)`,
  `id < numNames[t]`, sem ID duplicado, sem leitura além do fim, `numNames` e
  `maxFrequency` coerentes com o tamanho do arquivo. A checagem de ordem
  alfabética do Scid (`strtree.h:488-503`) **não** é replicada (D260).

## 8. `.sg4`: blocos

- Blocos de **131.072 bytes** (`GF_BLOCKSIZE`, `gfile.h:12`). Um registro **nunca
  cruza blocos**: `offset / 131072 == (offset + length - 1) / 131072` e
  `bloco < numBlocos` (`numBlocos = ceil(tamanho / 131072)`); senão
  `ERROR_CorruptData` (`gfile.cpp:~252-262`).
- Ao gravar, o Scid completa o bloco anterior com zeros e abre um novo quando
  `ultimoBloco + len > 131072` (`gfile.cpp:178-186, 275-307`). Para o leitor basta
  `offset` e `length` do índice.

## 9. `.sg4`: registro de partida

```
[tags extras...] 00
[flags: 1 byte]
[se flags&1: FEN, bytes, NUL]
[fluxo de lances ... 0x0F]
[comentário 1 NUL][comentário 2 NUL]...
```

Qualquer leitura além do fim do registro é erro (`ERROR_BufferRead`); comentário
sem NUL final também. Bytes depois da última string de comentário são tolerados (como no Scid).

### 9.1 Tags extras (`game.cpp:4727-4792`)

Cada item começa com 1 byte `b`; `b == 0` encerra a lista.

- `255`: data do evento em 3 bytes (big-endian, mesma codificação de data do
  índice) → `EventDate`. O Scid 4.27 nunca grava isso, mas lê.
- `241..=254`: nome de tag comum (não vem no registro), seguido de 1 byte de
  tamanho do valor e o valor.
- `1..=240`: `b` é o tamanho do nome; seguem o nome, 1 byte de tamanho do valor e o
  valor.

Tags comuns (`game.cpp:~4636-4655`): 241 `WhiteCountry`, 242 `BlackCountry`,
243 `Annotator`, 244 `PlyCount`, 245 `EventDate`, 246 `Opening`, 247 `Variation`,
248 `Setup`, 249 `Source`, 250 `SetUp`. **251-254 são `NULL`** no C (derreferência
nula); o MyChess devolve erro de corrupção. Nome de até 240 bytes, valor de até
255. Tag repetida **substitui** o valor (`AddPgnTag`, `game.cpp:763-780`).

### 9.2 Byte de flags e FEN

Bit 0 = posição inicial própria; bit 1 = promoções; bit 2 = subpromoções; os
demais são ignorados na leitura. Com o bit 0, segue uma FEN completa terminada
em NUL (6 campos, `position.cpp:3025-3066`). A leitura da FEN
(`ReadFromFEN`, `position.cpp:2850-3010`) percorre o tabuleiro de a8 a h1, define a
**ordem da lista de peças** (seção 9.4) e exige exatamente 1 rei por lado e no
máximo 16 peças por lado. FEN inválida = erro. Relógio de meio-lance e número do
lance inválidos não são erro.

### 9.3 Fluxo de lances (`DecodeVariation`, `game.cpp:4615-4660`)

| Byte | Significado |
|---|---|
| `11` (`NAG`) | o byte seguinte é o NAG, aplicado ao último lance |
| `12` (`COMMENT`) | o último lance (ou o início de partida/variação) tem comentário; o texto vem no fim |
| `13` (`START_MARKER`) | abre variação |
| `14` (`END_MARKER`) | fecha variação |
| `15` (`END_GAME`) | fim da partida (exigido no nível 0) |
| outro | lance |

- Só os bytes exatos 11-15 são especiais (`0x1B` é lance da peça nº 1).
- No nível 0 o laço só termina com `15`; em variação, só com `14`; qualquer outra
  combinação é erro.
- Uma variação é **alternativa ao último lance lido**: vem logo depois dele (e de
  seus NAGs e do `12`) e parte da posição **anterior** ao lance. Várias variações
  do mesmo lance aparecem em sequência. `13` antes de qualquer lance é erro.
- NAG 0 é ignorado; no máximo 7 NAGs por lance, os excedentes são descartados em
  silêncio (`game.h:132`, `game.cpp:562-566`).
  Um NAG antes do primeiro lance de uma linha não tem a que se anexar (o Scid o
  grava no marcador de início) e é ignorado. Vários `12` no mesmo lance contam um
  comentário só (o Scid apenas marca o lance).
- **Comentários**, depois do `END_GAME`, em pré-ordem: o do próprio nó (início da
  partida primeiro, depois cada lance), depois, para cada variação do lance **em
  ordem**, o do marcador de início da variação, seguido dos lances e das variações
  aninhadas. O total de strings deve bater com o número de `12` lidos.
- **Lance nulo** = lance de rei de uma casa até ela mesma = byte `0x00`. Só o rei: outra peça com `to == from` é erro (`isNullMove` exige rei, `movelist.h:51`).
- A decodificação **não verifica legalidade**; valida só a estrutura.

### 9.4 Lista de peças e codificação do lance

Casas: `A1=0 ... H1=7, A2=8 ... H8=63` (fileira = `sq / 8`, coluna = `sq % 8`).
`WHITE=0`, `BLACK=1`. Tipos: `KING=1, QUEEN=2, ROOK=3, BISHOP=4, KNIGHT=5,
PAWN=6`, `EMPTY=7`.

O byte de lance é `(pieceNum << 4) | v`. `pieceNum` é o índice (0-15) na **lista de
peças** do lado que joga (`Position::List[cor][0..15]`, com `Count[cor]`). **A
decodificação depende da ordem dessa lista**; a ordem de qualquer gerador de
lances do `mychess-core` é irrelevante.

Montagem e manutenção da lista:

- **Posição padrão** (`position.cpp:638-657`): índices 0-7 = Rei(e), Torre(a),
  Cavalo(b), Bispo(c), Dama(d), Bispo(f), Cavalo(g), Torre(h); 8-15 = peões a-h.
- **Posição de FEN:** `AddPiece` na ordem de leitura (a8 → h1, fileira a
  fileira); cada peça entra no fim, **exceto o rei**, que vai ao índice 0 e
  empurra a peça do índice 0 para o fim (`position.cpp:700-727`). Independente por
  cor.
- **Lance comum:** só atualiza a casa (`List[cor][pieceNum] = to`).
- **Captura** (e en passant, cuja casa é `to ∓ 8`): a peça capturada sai da lista
  do adversário **trocando com a última** (`Count[adv]--;
  List[adv][capturado] = List[adv][Count[adv]]`, `position.cpp:1841-1860`).
- **Promoção:** a peça mantém o índice; só o tipo no tabuleiro muda (o tipo do
  lance seguinte vem de `board[from]`).
- **Roque:** reconhecido pelo rei que sai da coluna `e` e chega a `c` ou `g` (só
  pela coluna, `position.cpp:1881-1894`); a torre muda de casa sem mudar de índice.
- `UndoSimpleMove` (`position.cpp:1956-2040`) restaura a ordem exatamente; é a
  base das variações.

Decodificação de `to` (peça em `from = List[cor][pieceNum]`, tipo de `board[from]`;
`game.cpp:4140-4420`):

| Peça | Regra |
|---|---|
| Rei | `v = 0`: lance nulo (`to = from`). `v = 1..10`: `to = from + {-9,-8,-7,-1,1,7,8,9,-2,+2}[v-1]` (9 = roque grande, 10 = roque pequeno). `v` 11-15 não é do rei |
| Cavalo | `v = 1..8`: `to = from + {-17,-15,-10,-6,+6,+10,+15,+17}[v-1]`; `v = 0` ou `> 8` é erro |
| Torre | `v >= 8`: mesma coluna, fileira `v - 8`; senão mesma fileira, coluna `v` |
| Bispo | `col = v & 7`, `dc = col - colunaDe(from)`; `v < 8`: `to = from + 9*dc`; `v >= 8`: `to = from - 7*dc`; `to` fora de 0-63 é erro |
| Dama | `v >= 8`: mesma coluna, fileira `v-8`; `v < 8` e `v != colunaDe(from)`: mesma fileira, coluna `v`; `v == colunaDe(from)`: **diagonal em 2 bytes**: o byte seguinte `n` deve estar em 64..=127 e `to = n - 64` |
| Peão | `d = {7,8,9,7,8,9,7,8,9,7,8,9,7,8,9,16}[v]`; `to = from + d` (brancas) ou `from - d` (negras). Promoção: `v` 0-2 sem; 3-5 Dama; 6-8 Torre; 9-11 Bispo; 12-14 Cavalo; 15 = lance duplo, sem promoção |

`pieceNum >= Count[cor]` é erro. `to` fora de 0..=63 é erro (o C não verifica para
o cavalo). Exemplo mínimo: `1.e4 e5` da posição padrão = `00 00 CF CF 0F` (sem tags,
flags 0, peão de e2 = índice 12 com `v = 15`, idem e7, fim).

## 10. Divergências e defeitos registrados no código do Scid

- `index.h:~215` diz "48 bytes"; o código usa 47.
- `matsig.h`: comentário de `MASK_WQ` ("29-31") errado.
- `common.h:185` chama o rating 6 de `RATING_BCF`; `game.cpp:180`, de `ECF`.
- `Header.version` do `.sn4` não vai para o disco.
- `commonTags`: "252-254 spare" no comentário, mas 251-254 são `NULL` (e há um 5º
  `NULL`).
- `MAX_TAGS = 40` só é aplicado por `ASSERT` (inoperante em release).
- A ajuda diz que o registro pode ter 131.072 bytes; o campo de 17 bits só
  representa até 131.071.
- Chess960 **não** é representável (roque por coluna `e` → `c`/`g`).

## 11. Escrita (fatia M3-2a)

Regras de escrita implementadas em `mychess-db` (`si4::derive`, `si4::write`,
`si4::names_write`; contrato em `../specs/m3-2-escrita-si4.md`). Conferidas
byte a byte com os 6 bancos do Scid: registros, entradas, campos derivados e
`.sn4`.

- **Entrada** (`IndexEntry::to_bytes`): o inverso das seções 3 e 5; contagens
  com `EncodeCount` (0-10, 11-12 → 10, 13-17 → 11, 18-24 → 12, 25-34 → 13,
  35-44 → 14, 45+ → 15); Elo `min(elo, 4000)` em 12 bits; `NumHalfMoves` módulo
  1024 (byte 37 = bits 0-7, bits 8-9 nos bits 6-7 de `HomePawnData[0]`); data do
  evento só se o ano da partida for ≥ 3 e o do evento estiver a ±3 (senão o campo
  fica 0, inclusive mês e dia).
- **Dados da linha principal** (`derive_mainline`): material final (`matsig_Make`,
  peões 4 bits, peças `min(3, n)`); `HomePawnData` só com posição padrão (contagem
  em `[0]`; cada mudança do `HPSIG` grava `floor(log2(antigo - novo))` em meio
  byte, o primeiro no nibble alto); `PROMO`/`UPROMO` da linha principal;
  *stored line* = a linha mais longa da tabela de `stored.cpp` (254 linhas, ver
  `stored_lines.rs`) que é prefixo da linha principal, comparada por
  `(origem, destino)` com o roque como lance de rei de duas casas; só com posição
  padrão.
- **Registro**: tags na ordem (nomes comuns 241-250 em um byte), byte de flags,
  FEN (posições não padrão), fluxo de lances (`(peça << 4) | v`; o número da peça
  vem da lista de peças), NAGs (no máximo 7 por lance), `COMMENT` onde há
  comentário, variações entre `13` e `14`, `15`, e os comentários em pré-ordem.
  Tamanho máximo 131.071 bytes; até 40 tags (nome 1-240 bytes, valor até 255).
- **`.sg4`**: o registro vai no fim; se não cabe no resto do bloco de 131.072
  bytes, começa o bloco seguinte e o intervalo é de zeros; o arquivo termina no
  fim do último registro; tamanho múltiplo exato do bloco conta como bloco cheio.
- **`.sn4`**: seções em ordem de nome (primeiro byte `u8`, depois bytes `i8` com
  NUL final), IDs de 2 bytes (3 com 65.536 nomes ou mais), frequência de 1, 2 ou 3
  bytes conforme `maxFrequency` (menor que 256, menor que 65.536, maior), prefixo
  de front-coding contra o registro anterior do arquivo; `maxFrequency` é um
  máximo histórico que só sobe; `timeStamp` preservado.
- **Banco novo**: `.si4` de 182 bytes (versão 400, autoload bruto 2), `.sn4` de
  36 bytes (tudo 0) e `.sg4` vazio.
- **Gravação segura**: registro (`.sg4`) → `.sn4` por temporário + `rename` →
  entrada → cabeçalho, cada passo com `fsync`; trava `File::try_lock` no `.si4`.

**Compactação** (M3-2b): partidas: descarta flag `D` e registros ilegíveis,
copia registros e entradas crus (só `Offset` e `NumHalfMoves` mudam), reempacota
o `.sg4` com a regra de blocos; o `.sn4` não muda. Nomes: só com nome de
frequência 0; renumera preservando a ordem; `maxFrequency` = máximo real e
`timeStamp` 0.

**`.si3` → `.si4`**: cabeçalho de 128 bytes (magic, versão, `baseType`,
`numGames`, `autoLoad`, descrição) vira o de 182 (versão 400, 54 zeros); cada
entrada de 46 bytes ganha um byte 0 (`Length_High`) após o 6º; `.sg3`/`.sn3` só
são renomeados.

**Diário `<base>.txn`** (extensão do MyChess, o Scid a ignora): texto com os
passos `rename`/`remove` de uma troca de vários arquivos; ver
`docs/specs/m3-2-escrita-si4.md`, D310-D312. Arquivos `*.tmp` com esse nome são
temporários do MyChess. `<base>.lock` (D321) é a trava de exclusão entre
processos durante uma transação; também é extensão do MyChess, o Scid a ignora, e
fica vazio (pode ser apagado com o banco fechado).

`-fast` não existe no MyChess.

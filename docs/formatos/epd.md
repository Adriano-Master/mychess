# Formato EPD (Extended Position Description)

Uma posição por linha, com operações. O MyChess lê e grava o EPD **padrão** e
oferece o dialeto do Scid como opção.

- **Origem no Scid** (relativa a `reference/scid_vs_pc-4.27/`): `src/pbook.cpp`
  (`ReadFile` 640, `WriteFile` 717, `Insert` 343, `StripOpcode` 483,
  `FindSummary` 235), `src/tkscid.cpp` (`sc_epd`, 3738-4034), `tcl/file/epd.tcl`.
- **Código no MyChess:** `crates/mychess-pgn/src/epd/` (`reader.rs`,
  `writer.rs`, `book.rs`, `record.rs`, `error.rs`).
- Especificação: [`../specs/m1-4-epd-eco.md`](../specs/m1-4-epd-eco.md), parte A.

## 1. Sintaxe

```
<tabuleiro> <vez> <roque> <en passant>[ <opcode> [<operandos>];]...
```

Exemplo: `r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - bm Bb5; id "ruy";`

- Os quatro primeiros campos seguem a leitura tolerante de FEN
  (`Position::from_fen`); roque ou en passant incoerentes são descartados em
  silêncio.
- Se a linha trouxer **contador de meios-lances e número do lance numéricos**
  logo após o quarto campo (formato FEN de 6 campos), eles ajustam os relógios
  da `Position` e não viram operações. O Scid os lê como FEN e os descarta.
- Cada operação é `opcode operandos;`. `opcode` casa `[A-Za-z][A-Za-z0-9_]*`;
  fora disso a operação é preservada como bruta, com o aviso
  `NonStandardOpcode`. Opcode sem operando é válido (`noop;`). Os operandos
  ficam como **texto bruto aparado** (aspas preservadas), para que escrever
  reproduza o que foi lido.

## 2. Dialetos

| | Padrão (default) | Scid (`EpdOptions::scid()`) |
|---|---|---|
| Separador de operações | `;` **fora de aspas** | qualquer `;` (aspas não são rastreadas) |
| Barra invertida | literal | `\s` = `;`, `\\` = `\`, `\x` = `x` para qualquer outro `x` |
| Escrita | nada é escapado | `\` vira `\\` e `;` vira `\s` |

O Scid corta em qualquer `;` e escreve `;` de texto como `\s` (ajuda `EPD`,
`pbook.cpp:685-692,752-768`). Detecção automática do dialeto fica fora do M1.

## 3. Regras de leitura

- Linhas terminam em `\n`, `\r\n` ou `\r`; **a última linha sem terminador é
  processada** (o Scid a perde, `pbook.cpp:659`).
- Linha em branco e linhas iniciadas por `#` ou `%` são ignoradas.
- Codificação: UTF-8; texto inválido é lido como `windows-1252`. Gzip é
  detectado pelos bytes mágicos `1F 8B` e descomprimido em fluxo (somente
  leitura).
- **Posição repetida: a última vence**, mantendo o lugar da primeira
  (`PBook::Insert`), com o diagnóstico `DuplicatePosition { first_line }`.
- Aspa sem fechar até o fim da linha: a operação engole o resto da linha, com
  `UnterminatedString`.
- Linha cujo FEN não leia: `InvalidPosition { error }` e a linha é ignorada (o
  Scid só escreve no `stderr`).
- Os registros ficam na ordem do arquivo; a leitura nunca reordena. Operações
  de um registro podem ser ordenadas por `sort_operations()` (comparação de
  bytes de `opcode operandos`, como o `lsort` ASCII do Tcl).

## 4. Regras de escrita

- Uma linha por registro, na ordem do livro: FEN de **4 campos** e, para cada
  operação, ` opcode operandos;`. Registro sem operações: só o FEN. Final de
  linha `\n`.
- `hmvc`/`fmvn` só são escritos com `write_clocks: true` e se o registro não os
  tiver.
- Escrita em arquivo é atômica (temporário no mesmo diretório e `rename`). Todo
  o livro é verificado **antes** do primeiro byte; se algo não puder ser
  escrito, nada é escrito.
- `EpdError::Unwritable { opcode, reason }` quando a operação não sobreviveria
  à leitura: opcode vazio ou com espaço; quebra de linha; (dialeto padrão)
  **número ímpar de aspas** ou `;` fora de aspas; operação que seria relida
  como contadores de relógio logo após o FEN.

## 5. Limites do MyChess

| Limite | Valor | Resultado |
|---|---|---|
| Tamanho de linha | 1 MiB | a linha é ignorada, com `LineTooLong` |
| Diagnósticos guardados | 100 000 | o resto é contado em `EpdLoad::dropped_diagnostics` |
| Gzip descomprimido | 256 MiB (`max_decompressed_bytes`; `None` = sem limite) | `EpdError::LimitExceeded` |
| Registros | sem limite (`max_records: Option<usize>`) | `EpdError::LimitExceeded` |

## 6. Relógios do Scid e outras operações

- O Scid guarda as operações de uma posição como texto, uma por linha
  (`opcode operandos`), sem `;`. O MyChess guarda `Vec<EpdOperation>`.
- `summary(&Position)`: se há `ce`, mostra `ce/100` com sinal e duas casas, do
  ponto de vista das **brancas** (inverte quando as pretas jogam); senão o
  primeiro operando existente entre `eco nic pv pm bm id`.
- `ce` no padrão é do ponto de vista de quem joga. O cálculo de mate do Scid
  (`32767 - (2*dm - 1)`, `epd.tcl:745-748`) não é replicado aqui; fica para a
  anotação por engine (M7).
- `strip_opcode` remove **todas** as ocorrências em todos os registros; o Scid
  remove só a primeira e nunca remove `noop;`.
- `deepest_match(&Game)` devolve `None` quando nada casa (o Scid devolve 0, que
  é ambíguo com o ply 0).

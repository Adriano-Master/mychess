# Formato de livro Polyglot (`.bin`)

Livros de aberturas no formato do Polyglot, o mesmo que o Scid vs PC lê.
Registros de 16 bytes, **big-endian**, sem cabeçalho nem rodapé, ordenados por
chave. O MyChess lê, consulta, edita, cria e mescla esses livros nativamente
(o Scid depende do programa externo `polyglot` para criar e mesclar).

- **Origem no Scid** (relativa a `reference/scid_vs_pc-4.27/`):
  `src/polyglot/book.cpp` (`scid_book_open` 201, `scid_book_disp` 231,
  `scid_book_movesupdate` 104, `find_pos` 473, `read_integer` 644),
  `book_make.cpp`, `book_merge.cpp`, `hash.cpp` 25-99, `random.cpp` 11-198,
  `move.cpp` 60-142, `board.cpp` 366-377, `fen.cpp`; `src/tkscid.cpp`
  (`sc_book` 18169-18321).
- **Código no MyChess:** `crates/mychess-book/src/` (`format.rs`, `zobrist.rs`,
  `random.rs`, `codec.rs`, `book.rs`, `edit.rs`, `merge.rs`, `build.rs`,
  `file.rs`).
- Especificação: [`../specs/m5-5-livros-polyglot.md`](../specs/m5-5-livros-polyglot.md).

## 1. Registro (16 bytes)

| Offset | Largura | Campo | Significado |
|---|---|---|---|
| 0 | 8 | `key` (u64) | Chave Zobrist Polyglot da posição **antes** do lance |
| 8 | 2 | `move` (u16) | Lance (seção 3); `0` = ausente |
| 10 | 2 | `weight` (u16) | Peso do lance (0 a 65535; os ajustes usam no máximo `0xfff0`) |
| 12 | 4 | `learn` (u32) | Aprendizado: `n` (u16, jogos) e `sum` (u16, soma de `resultado+1`); **opaco** para o MyChess, preservado ao ler e regravar |

Número de entradas = `tamanho / 16`. Um resto menor que 16 bytes é ignorado
(`scid_book_open`); na regravação é descartado. Arquivo vazio, com menos de
16 bytes ou inexistente é erro. O MyChess recusa arquivos acima de 128 MiB
(`BookError::TooLarge`) e o que não for arquivo comum.

## 2. Ordem

Entradas ordenadas por `key` como inteiro **sem sinal**. Dentro da mesma chave
vale a ordem do arquivo (os livros do Scid nem sempre estão em ordem de peso:
`Elo2400.bin` não está). A busca é binária e acha a **primeira** entrada da
chave (`find_pos`). Um arquivo fora de ordem é ordenado em memória de modo
estável e marcado como tal; a gravação escreve ordenado.

## 3. Lance (u16)

| Bits | Campo |
|---|---|
| 0-2 | coluna de destino (a=0) |
| 3-5 | fileira de destino (1=0) |
| 6-8 | coluna de origem |
| 9-11 | fileira de origem |
| 12-14 | promoção: 1 cavalo, 2 bispo, 3 torre, 4 dama |
| 15 | livre |

O **roque** é codificado como o rei capturando a própria torre: `e1h1` =
`0x0107`, `e1a1`, `e8h8`, `e8a8`. A forma `e1g1` não é aceita. Chess960 e
posições com roque não padrão não têm representação: o livro não se aplica.

## 4. Chave Zobrist

XOR de valores da tabela `Random64` (781 constantes `u64`, copiadas de
`random.cpp` 11-198; primeira `0x9D39247E33776D41`, última
`0xF8D626AAAF278509`):

| Índices | Uso |
|---|---|
| `64*p + 8*fileira + coluna` (0-767) | peça `p` na casa; `p`: peão preto 0, peão branco 1, cavalo p 2 / b 3, bispo p 4 / b 5, torre p 6 / b 7, dama p 8 / b 9, rei p 10 / b 11; a1 = fileira 0, coluna 0 |
| 768-771 | direito de roque: rei branco, dama branca, rei preto, dama preta |
| 772-779 | coluna do en passant |
| 780 | vez: só quando as **brancas** jogam |

O en passant só entra na chave se há **peão adversário adjacente** ao peão que
avançou; não se verifica pino nem a legalidade da captura. A chave é calculada
da posição, nunca de texto FEN. Vetores de conferência estão na especificação
(nove posições, p. ex. a inicial = `463b96181691fc9c`).

## 5. Porcentagem exibida

`percent = round((peso / soma) * 100)` em ponto flutuante, com `soma` = soma dos
pesos de **todas** as entradas da chave (inclusive ilegais e de peso 0) e
arredondamento metade-para-par, como o `printf("%.0f")` do Scid. Só entram na
lista os lances com peso > 0, lance ≠ 0 e legais na posição.

## 6. Escrita

Nunca no próprio arquivo: temporário no mesmo diretório, `sync`, fechar, e
`rename`; permissões do destino e symlinks são preservados. Falha no meio deixa
o original intacto.

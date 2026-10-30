# Formato `.eco` (classificação de aberturas)

Arquivo de texto com o catálogo de aberturas ECO (`scid.eco`, `scid_fr.eco`).
Cada entrada associa um código, um nome e uma linha de lances; a classificação
procura a **posição** da partida no catálogo, então transposições casam.

- **Origem no Scid** (relativa a `reference/scid_vs_pc-4.27/`): `src/pbook.cpp`
  (`PBook::ReadEcoFile` 534–637, `Insert` 343–372, `EcoSummary` 421, `EcoFind`
  451), `src/position.cpp` (`ReadLine` 2580, leitura permissiva de lances),
  `src/misc.cpp` (`eco_FromString` 71, `eco_ToString` 113, `eco_BasicCode`
  145), `src/tkscid.cpp` (`sc_eco`, 4636–5134). Ajuda `ECO` em
  `tcl/help/help.tcl`.
- **Código no MyChess:** análise do texto e classificação em
  `mychess-core` (`eco_book.rs`, `eco.rs`); leitura do **arquivo**
  (decodificação, `.gz`, limite de tamanho) em `mychess-pgn` (`eco_file.rs`).
- Especificação: [`../specs/m1-4-epd-eco.md`](../specs/m1-4-epd-eco.md), parte B.

## 1. Sintaxe

```
# comentário (só reconhecido onde se espera um código)
A00b "Barnes Opening"  1.f3 *
C89x "Spanish: Marshall, ..."
  1.e4 e5 2.Nf3 ... 18.a4 Qh5 *
```

| Elemento | Regra |
|---|---|
| Código | `[A-E][0-9][0-9]`, depois uma letra minúscula `a`-`z` opcional e, **somente com a letra**, um dígito `1`-`4` opcional: `A00`, `A00a`, `A00a1`, `E99b2`. |
| Nome | Entre aspas duplas, sem aspas internas; qualquer UTF-8. |
| Lances | Qualquer texto até o próximo `*`; pode ocupar várias linhas. Espaços e quebras colapsam em um espaço. Entrada **sem lances** é a posição inicial. |
| Comentário | `#` descarta o resto da linha, mas só onde se espera um código (não dentro de nome ou de lances). |
| Ordem de classificação | `A00`, `A00a`, `A00a1`...`A00a4`, `A00b`... (comentário do cabeçalho de `scid.eco`). |

### Códigos estendidos e código interno do Scid

O Scid guarda o ECO em 16 bits (`misc.cpp:60-110`): 0 é "sem ECO", `A00 = 1`,
`A01 = 132`; cada código básico vale o anterior + 131. Os 130 subcódigos
estendidos (`a, a1..a4, b, b1..b4, ... z, z1..z4`) ocupam os 130 valores
seguintes ao básico: a letra `x` soma `1 + 5 * (x - 'a')` e o dígito soma de
`1` a `4`. O MyChess reproduz isso em `Eco::to_scid_code`/`from_scid_code`
(`E99z4 = 65500`; 0 e valores acima de 65500 são inválidos). `Eco::basic()`
reduz ao código de três caracteres. O arquivo `scid.eco` **declara** o nível
numérico `1`-`4`, mas o arquivo real não o usa.

## 2. Regras de leitura (iguais às do Scid, salvo o que a seção 4 indica)

1. O leitor varre o texto ignorando qualquer caractere até uma letra `A`-`E`
   maiúscula. Letras `A`-`E` perdidas depois de um `*` iniciam uma entrada e
   quase sempre causam erro (herdado de `pbook.cpp:556-575`).
2. Código: as duas posições seguintes à letra devem ser dígitos, senão o
   arquivo está corrompido. Depois, a letra minúscula e o dígito opcionais.
3. Nome: tudo entre a próxima `"` e a seguinte. Texto entre o código e a
   primeira aspa é ignorado. Fim de arquivo antes do fechamento é erro.
4. Lances: tudo até o `*`; fim de arquivo antes dele é erro. O texto é guardado
   sem espaço à esquerda nem antes do `*`.
5. Interpretação dos lances (como `Position::ReadLine`): a partir da posição
   inicial padrão, cada "palavra" é reduzida a caracteres alfanuméricos e `=`;
   `OO`/`OOO` são roques; `K Q R B N` (e `k q r n`, promovidos a maiúscula;
   `b` minúsculo é peão) iniciam lance de peça; `a`-`h`, lance de peão;
   qualquer outra letra inicial é erro. Números de lance, pontos, parênteses e
   dígitos soltos desaparecem na redução, e por isso o texto fora do SAN puro
   de `scid_fr.eco` é lido sem erro. No MyChess: redução da palavra e
   `Position::parse_san` em modo tolerante.
6. Lance ilegal ou ambíguo: erro, com a linha do `*` da entrada.
7. **Posição repetida: a última entrada vence** e mantém o índice de ordem da
   primeira (`PBook::Insert` sobrescreve). O comentário "just ignore it" em
   `pbook.cpp:627` está errado; vale o código.
8. A identidade da posição é a `Position` do MyChess (tabuleiro, vez, direitos
   de roque, en passant legal), sem relógios. O Scid guarda o en passant
   quando há peão adjacente capaz de capturar mesmo que a captura seja ilegal;
   nenhuma entrada de `scid.eco` é afetada.

## 3. Números reais dos arquivos do Scid vs PC 4.27

Medidos por `compat_*` (`#[ignore]`, `cargo xtask compat`, que leem de
`reference/`; os arquivos **não** entram no repositório).

| Arquivo | Linhas | Entradas | Posições distintas | Códigos | Observações |
|---|---|---|---|---|---|
| `scid.eco` | 20 813 (UTF-8) | 10 360 | 10 360 (nenhuma repetida) | 2 537 básicos (3 caracteres) + 7 823 com letra; nenhum com dígito | até 36 meios-lances; entrada `A00a "Start"` sem lances; nomes com `ä ö ü`; mínimo de 21 peças em qualquer posição; 0 erros |
| `scid_fr.eco` | 2 369 (8 bits) | 2 364 | 2 342 (22 sobrescritas) | todos básicos | `windows-1252` (bytes `0x85 0x92 0x96 0xE0 0xE4 0xE7 0xE8 0xE9 0xEA 0xEF 0xF4 0xF6 0xFC`); 8 entradas com texto fora do SAN puro (7 com parênteses, 1 com número solto); sem entrada `Start`; mínimo de 22 peças |

A ajuda `ECO` fala em 500 códigos; o arquivo tem 2 537 entradas básicas porque o
mesmo código se repete para várias linhas.

Vetores de classificação (`scid.eco`, partida inteira): `e4 c5 Nf3 d6 d4 cxd4
Nxd4 Nf6 Nc3 a6` -> ply 10, `B90a` "Sicilian: Najdorf"; `d4 Nf6 c4 e6 Nc3 Bb4`
e a transposição `c4 e6 Nc3 Nf6 d4 Bb4` -> ply 6, `E20`; sem lances -> ply 0,
`A00a` "Start". Em `scid_fr.eco`, partida sem lances não casa.

## 4. Limites, decodificação e diagnósticos do MyChess

Diferenças deliberadas em relação ao Scid (que lê bytes crus e aborta ao
primeiro erro):

| Tema | MyChess |
|---|---|
| Codificação | UTF-8 se válido; senão `windows-1252` (`scid_fr.eco`). BOM UTF-8 ignorado; `\r\n` vira `\n`. |
| Compressão | `.gz` detectado pelos bytes mágicos `1F 8B` (não pela extensão). |
| Tamanho | 16 MiB **depois** de descomprimir (`EcoFileError::TooLarge`); `read_eco_file_limited` troca o limite. |
| Entrada | no máximo 1 MiB de nome ou de lances por entrada (`CorruptReason::TooLarge`) e 1024 meios-lances (`TooManyPlies`). |
| Erros | `EcoBookError::Corrupt { line, reason }` com `CorruptReason`: `BadCode`, `UnterminatedName`, `UnterminatedMoves`, `IllegalMove`, `TooLarge`, `TooManyPlies`. Sem livro parcial (como o Scid), a menos que `on_error: SkipEntry`. |
| `SkipEntry` | descarta a entrada corrompida e **ressincroniza na linha seguinte**, com o diagnóstico `EntrySkipped { line, reason }`. |
| Diagnósticos | `DuplicatePosition { line, first_line }` (informativo) e `EntrySkipped`; guarda 10 000 e conta o resto em `EcoReport::omitted_diagnostics`. `entries_read` conta as entradas lidas, repetidas incluídas; `len()` conta posições distintas. |
| Chess960 | o livro é de xadrez padrão: partida em modo Chess960 não é classificada (`None`). |
| Partida com FEN inicial | é classificada, como no Scid. |

### Consultas

- `classify(&Game, ClassifyOptions)`: varre a linha principal do fim para o
  começo e devolve a **maior profundidade** que está no livro; variações são
  ignoradas. `ClassifyOptions::game()` não limita; `batch()` limita a 60
  meios-lances, como `sc_eco base`. `classify_line` faz o mesmo sem montar um
  `Game`.
- `lookup(&Position)`: consulta direta.
- `summary(prefix)` equivale a `EcoSummary`: com prefixo de comprimento
  `d < 3`, pula a entrada que compartilha ao menos `d + 1` caracteres com a
  última entrada mostrada; com `d >= 3`, mostra todas.
- `find(text)` equivale a `EcoFind`: procura `text` (sensível a caixa) no texto
  `código [nome]`. Os lances não participam da busca, também no Scid.
- `translate_eco_name(nome, glossário)`: aplica os pares na ordem inversa à de
  cadastro (o Scid prepende cada par e percorre do mais novo ao mais antigo).
- `EcoBook` é imutável depois de carregado e `Send + Sync`.

## 5. Conversões

Cada entrada vira, no livro EPD, as operações `eco <código> [<nome>]` e
`moves <lances>` (`ReadEcoFile` monta `eco A00b [Barnes Opening]` e `moves
1.f3 `). `EcoBook::to_epd_book()` produz o mesmo, sem o espaço final em
`moves`. Ver [`epd.md`](epd.md).

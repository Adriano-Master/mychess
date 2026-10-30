# Mapa do código de referência: Scid vs PC 4.27

O código original do Scid vs PC fica em `reference/scid_vs_pc-4.27/`. Ele é
**somente leitura**, não é versionado e não é compilado pelo workspace. Serve
como **especificação de comportamento** e, para formatos de arquivo, como
especificação byte a byte. As regras de uso estão em
`.claude/rules/referencia-scid.md`, e a pesquisa guiada é feita pela skill
`/consultar-scid`.

Todos os caminhos abaixo são relativos a `reference/scid_vs_pc-4.27/`.

## Como obter

```sh
mkdir -p reference
curl -L https://sourceforge.net/projects/scidvspc/files/source/scid_vs_pc-4.27.tgz/download | tar xz -C reference
```

## Como pesquisar

- O diretório está no `.gitignore`, então buscas a partir da raiz do projeto o
  **ignoram**. Use caminho explícito:
  `rg -n "MAX_BASES" reference/scid_vs_pc-4.27/src`.
- Caminho típico: ajuda (`tcl/help/help.tcl`) → menu (`tcl/menus.tcl`) → Tcl da
  janela (`tcl/windows/`, `tcl/tools/`...) → comando `sc_*` em
  `src/tkscid.cpp` → classe C++ em `src/`.
- Quando a ajuda e o código divergirem, **o código vence** em questões de
  compatibilidade.

## Arquitetura do Scid vs PC

- `tkscid` é Tcl/Tk mais o núcleo C++. `src/tkscid.cpp:418-438` registra 17
  comandos `sc_*`.
- O programa `scid` é a concatenação dos arquivos Tcl listados em
  `Makefile.conf:228-262`.
- `tcscid` é o mesmo C++ compilado com `-DTCL_ONLY`, para scripts.
- São ~65 mil linhas de C++ (`src/`) e ~230 mil de Tcl (`tcl/`, incluindo 19
  idiomas).

## Núcleo C++ (`src/`)

| Arquivo | Responsabilidade | No MyChess |
|---|---|---|
| `position.cpp/.h` | Posição, geração de lances, SAN e a **lista de peças**, cuja ordem é usada na codificação SI4 | `shakmaty` + codec SI4 (`mychess-db`) |
| `game.cpp/.h` | Partida: árvore de lances, variações, comentários, NAGs; codificação/decodificação SI4; saída PGN | `mychess-core`, `mychess-db`, `mychess-pgn` |
| `index.cpp/.h` | Índice `.si4` | `mychess-db` |
| `namebase.cpp/.h` | Nomes `.sn4` | `mychess-db` |
| `gfile.cpp/.h`, `mfile.cpp`, `bytebuf.cpp` | Arquivo `.sg4` em blocos, I/O e buffers big-endian | `mychess-db` |
| `filter.cpp` | Filtro (1 byte por partida) e filtro comprimido | `mychess-db` |
| `tree.cpp` | Árvore de aberturas e cache `.stc` | `mychess-db` |
| `pgnparse.cpp` | Parser de PGN | `mychess-pgn` (sobre `pgn-reader`) |
| `pbook.cpp` | Arquivos ECO (`.eco`) e EPD | `mychess-core` / `mychess-pgn` |
| `spellchk.cpp` | Arquivo de *spelling* `.ssp` | `mychess-db` |
| `crosstab.cpp` | Tabelas de torneio | `mychess-db` ou `mychess-app` |
| `optable.cpp` | Relatório de abertura | `mychess-app` |
| `stored.cpp` | *Stored lines* (254 linhas) para o índice | `mychess-db` |
| `matsig.cpp` | Assinatura de material final e de peões | `mychess-db` |
| `date.cpp/.h`, `misc.cpp/.h` | Datas parciais, utilitários, leitura/escrita big-endian, `eco_FromString` | vários |
| `nagtext.h` | Textos dos NAGs | `mychess-core` |
| `charsetconverter.cpp`, `universalchardet/` | Detecção e conversão de codificação | `chardetng` + `encoding_rs` |
| `polyglot/` | Livros Polyglot (`book.cpp`) | `mychess-book` |
| `cql/` | CQL 5.2 (licença restrita, ver `roteiro.md`) | não portar (ADR 0009) |
| `engine.cpp`, `recog.cpp` | Engine interna (melhor casa, Scidlet) | não será portada |
| `probe.cpp`, `egtb/` | Tablebases Nalimov (licença restrita) | substituídas por Syzygy |
| `tkscid.cpp` | Ponte Tcl: todos os comandos `sc_*` | `mychess-app` (casos de uso) |
| `pgnscid.cpp`, `scmerge.cpp`, `scidt.cpp`, `scidlet.cpp` | Ferramentas de linha de comando | `mychess-cli` |
| `tkdnd/`, `tk_selection.cpp` | Arrastar e soltar, seleção X11 | iced/winit |

## Comandos `sc_*` (`src/tkscid.cpp`)

| Comando | Linhas | Propósito |
|---|---|---|
| `sc_base` | 642–3737 | Abrir, criar, fechar, exportar, importar, ordenar, estatísticas, duplicatas, tags, torneios, *piece tracker*, atualizar, reparar |
| `sc_epd` | 3738–4034 | Arquivos EPD |
| `sc_clipbase` | 4035–4205 | Copiar, colar e limpar a Clipbase |
| `sc_compact` | 4206–4635 | Compactação |
| `sc_eco` | 4636–5134 | Classificação ECO e navegador |
| `sc_filter` | 5135–6055 | Operações de filtro, estatísticas, frequências, busca de texto |
| `sc_flags` | 6056–6168 | Flags de uma partida |
| `sc_game` | 6169–10324 | Carregar, salvar, PGN, informações, tags, mesclar, novidade, tabela de torneio, desfazer, avaliações |
| `sc_info` | 10325–10772 | Limites, tablebases, prioridade, versão |
| `sc_move` | 10773–11057 | Adicionar lances e navegar |
| `sc_pos` | 11058–11947 | Posição, NAGs, comentários, FEN/HTML/TeX, análise, consulta a tablebase |
| `sc_progressBar` | 11948–11980 | Barra de progresso |
| `sc_name` | 11981–14279 | *Spelling*, edição de nomes, informações de jogador, listas, ratings, autocompletar |
| `sc_report` | 14280–14658 | Relatórios de abertura e de jogador |
| `sc_tree` | 14659–15759 | Árvore de aberturas e cache |
| `sc_search` | 15760–17972 | Buscas: posição, lances, CQL, material, cabeçalho |
| `sc_var` | 17973–18168 | Variações |
| `sc_book` | 18169–18321 | Livros Polyglot |

`doc/progref.html` documenta esses comandos.

## Interface Tcl (`tcl/`)

| Caminho | Funcionalidade |
|---|---|
| `menus.tcl` | Todos os menus; opções em 785–1553 |
| `main.tcl` | Entrada de lances, autoplay, modo tentativa, painel de informações |
| `board.tcl` | Widget do tabuleiro, marcações e setas, barra de avaliação |
| `bitmaps.tcl` | Conjuntos de peças e texturas (base64) |
| `game.tcl`, `move.tcl`, `edit.tcl` | Partida, navegação, edição, configurar posição, FEN |
| `file.tcl`, `file/*.tcl` | Bancos, marcadores, recentes, *finder*, manutenção (`maint.tcl`), EPD, *spelling* |
| `windows/*.tcl` | Lista de partidas, PGN, árvore, livro, ECO, comentários, tabela de torneio, estatísticas, seletor de bancos, localizadores |
| `search/*.tcl` | Buscas por cabeçalho, posição e material |
| `tools/*.tcl` | Análise (`analysis.tcl`), UCI, XBoard, torneios de engines (`comp.tcl`), táticas, treinadores, jogos contra engine, FICS, correspondência, tablebases, relatórios, gráficos, hardware |
| `utils/*.tcl` | Janelas e layouts (`win.tcl`), sons, fontes, gráficos, validação |
| `lang/*.tcl` | Inglês + 18 idiomas (`portbr.tcl` é o português do Brasil) |
| `help/help.tcl` | Ajuda: 81 tópicos |
| `end.tcl` | Exportação, salvar partida, editor de nomes, linha de comando (1863–2139) |

## Ajuda: a especificação para o usuário

- `tcl/help/help.tcl` (~7.500 linhas): 81 tópicos no formato
  `set helpText(<Tópico>)`, numa marcação pseudo-HTML.
- Tópicos-chave: Menus, Moves, Searches, CQL, Tree, TreeMasks, Maintenance,
  Formats, Analysis, Tourney, Book, BookTuning, Reports, Graphs, TB, FICS, CC,
  ShortCuts, Options, Cmdline. O changelog está nas linhas 6448–7250.
- Textos de interface: `tcl/lang/english.tcl` (referência) e
  `tcl/lang/portbr.tcl`. Dicas: `tcl/help/tips.tcl`.

## Formatos de arquivo (resumo)

**Todo inteiro binário é big-endian** (`misc.h`, `read/write*Bytes`). Este é um
resumo do inventário inicial; a especificação definitiva vai para
`docs/formatos/si4.md` (marco M3), conferida contra o código.

**`.si4` (índice): cabeçalho de 182 bytes**
- `"Scid.si\0"` (8 bytes), versão u16 (`400`), tipo do banco u32 (ícone 0–31)
- número de partidas u24, *autoload* u24
- descrição (108 bytes), 6 nomes de flags customizadas × 9 bytes

**`.si4`: entrada por partida (47 bytes)**
- offset u32; tamanho em 17 bits + 6 flags customizadas; flags u16
- IDs de nomes compactados (brancas/pretas 20 bits, evento/local 19, rodada 18)
- contagens de variações, comentários e NAGs (escala aproximada de 4 bits) + resultado
- ECO u16; datas u32 (data 20 bits, ano ≤ 2047; data do evento 12 bits, relativa ao ano)
- Elo 12 bits + tipo de rating 4 bits, ×2 (máximo 4000)
- assinatura de material final + código de *stored line* u32; meio-lances 10 bits
- 9 bytes de dados de peões
- 22 flags (`D W B M E N P T K Q ! ? U 1-6`); máximo de 16.777.214 partidas
- O comentário em `index.h` diz 48 bytes, mas a entrada tem **47**

**`.sn4` (nomes)**
- Cabeçalho de 36 bytes: `"Scid.sn\0"`, *timestamp* u32, 4 × número de nomes u24, 4 × frequência máxima u24
- Nomes por tipo, em ordem, com *front coding*: id (u16, ou u24 se ≥ 65.536 nomes), frequência (1–3 bytes), tamanho u8, prefixo compartilhado u8
- Máximo de 255 caracteres por nome. Limites: 2^20−1 jogadores, 2^19−1 eventos e locais, 2^18−1 rodadas

**`.sg4` (partidas)**
- Blocos de 128 KiB; uma partida nunca cruza bloco (preenchimento com zeros); máximo de 131.072 bytes por partida
- Tags extras (≤ 40, tamanho ≤ 240); tags comuns com códigos 241–250; 255 = data do evento binária; terminador 0
- Byte de flags (1 = posição inicial não padrão, 2 = promoções, 4 = subpromoções), FEN opcional
- Lances: byte `(índice na lista de peças << 4) | código`; lance diagonal de dama usa 2 bytes. Marcadores: 11 NAG, 12 comentário, 13 início de variação, 14 fim de variação, 15 fim de partida; até 8 NAGs por lance
- Comentários ao final, terminados em NUL, na ordem da partida
- **A decodificação depende da ordem da lista de peças** de `position.cpp`, e o índice depende das *stored lines* e das assinaturas

**Outros formatos**

| Formato | Código | Observação |
|---|---|---|
| `.si3` | `sc_base upgrade` (`tkscid.cpp:3457`) | Antigo: entradas de 46 bytes |
| `.stc` (cache da árvore) | `tree.cpp` `TreeCache::WriteFile/ReadFile` | Mágica `stc` + versão; tabuleiro + até 60 lances por entrada + filtro comprimido |
| `.ssp` (*spelling*) | `spellchk.cpp` `ReadSpellCheckFile` | Texto com seções `@PLAYER/@EVENT/@SITE/@ROUND`, `=` alias, `%Elo`, `%Bio`... |
| `.eco` | `pbook.cpp` `ReadEcoFile` | Texto: `A00b "Nome" 1.f3 *` (códigos estendidos) |
| `.epd` | `pbook.cpp` | Até 4 arquivos abertos |
| Polyglot `.bin` | `polyglot/book.cpp` | Entradas de 16 bytes: chave u64, lance u16, peso u16, *learn* 2 × u16 |
| `.sso`, `.stm`, `*.dat` | Tcl | Scripts Tcl (buscas salvas, máscaras de árvore, configuração) |
| `.spf` (fotos) | `main.tcl` | `photo "Nome" {GIF em base64}` |
| `.sff` (filtro) | `switcher.tcl` | Só um *stub*: nunca grava nada |

## Dados incluídos (fixtures de compatibilidade)

Podem ser lidos por testes locais `compat_*`, mas **não podem ser copiados para
o repositório** (ver `COPYING`).

| Caminho | Conteúdo | Uso em testes |
|---|---|---|
| `bases/` | 6 bancos SI4 v400: `matein1` (1.212 partidas), `matein2` (15.623), `matein3` (7.862), `matein4andmore` (3.417), `endings` (67), `tactics` (928) | Leitura SI4. Todas as partidas começam de FEN; só `tactics` tem variações, comentários e NAGs. Bancos com início padrão, ECO e Elo precisam ser gerados por nós |
| `books/` | Polyglot: `Elo2400.bin`, `gm2600.bin`, `Performance.bin`, `varied.bin` | Leitura Polyglot |
| `scid.eco` (20.813 linhas), `scid_fr.eco` (Latin-1) | Classificação ECO | ECO, SAN, Latin-1 |
| `spelling.ssp` (30 MB), `spelling.diff` | *Spelling* | Parser `.ssp` |
| `engines/phalanx/test.fin` | Problemas de xadrez | EPD/FEN |
| `sounds/` | 31 WAV (anúncio de lances) | — |
| `fonts/` | Scid Chess Standard/Traveller (TTF) | — |

A árvore não traz amostras de `.pgn`, `.epd`, `.cql`, `.sso`, `.stm`, `.stc` ou `.spf`.

## Configuração do Scid (para importação futura)

`~/.scidvspc/config/` (no Windows, `<exe>/config/`): `options.dat`,
`engines.dat` (`engine {Nome Cmd Args Dir Elo Time URL UCI UCIoptions}`),
`recent.dat`, `history.dat`, `bookmarks.dat`, `reports.dat`, `optrainer.dat`,
`treecache.dat`, `hardware.dat`, `correspondence.dat`. São todos scripts Tcl.

## Particularidades de plataforma no Scid

- **Windows**: configuração ao lado do executável, prioridade via
  `SetPriorityClass`, arrastar e soltar OLE, URLs com `cmd /c start`.
- **macOS**: menu da aplicação (Aqua), `::tk::mac::OpenDocument`, recursos em
  `../Resources`.
- **Linux/X11**: XDND, `xdg-open`, engines interrompidas com `kill -s INT`.
- **Programas externos**: engines, `pdflatex` (pacotes `xskak` e `pgfplots`),
  `sendmail`/SMTP, timeseal, `polyglot` (criação de livros), navegador web.

## Armadilhas conhecidas

- Chess960 só existe como patch (`patches/chess960-new.patch`).
- `sc_name taglist` não funciona (falta uma vírgula na lista de opções).
- A ajuda fala em `.sems`; o código usa `.sem`.
- `mtbdata.h` (tablebases em memória) não é usado pela aplicação.
- `src/cql/COPYING`: "todos os direitos reservados", com uso GPL autorizado
  por e-mail. Não porte esse código sem decidir antes (ver `roteiro.md`).

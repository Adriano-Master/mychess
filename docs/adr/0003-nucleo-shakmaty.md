# ADR 0003: Regras de xadrez e PGN sobre `shakmaty` e `pgn-reader`

- **Status:** aceita (2026-10-21). Emendada pela [ADR 0006](0006-leitor-pgn-proprio.md): o *leitor* de PGN é próprio; `pgn-reader` virou dependência de testes.

## Contexto

Geração de lances, legalidade, SAN, FEN e regras de fim de jogo (incluindo
Chess960) são a base de tudo e precisam estar corretas desde o primeiro dia.
Reescrevê-las do zero é caro e arriscado. O formato SI4, porém, codifica lances
pelo **índice da peça numa lista de peças do Scid**, um detalhe interno que
nenhuma biblioteca reproduz.

## Decisão

- `mychess-core` usa **`shakmaty` 0.30** (GPL-3.0-or-later; autor do
  python-chess, usada no ecossistema do Lichess) para posição, lances, SAN, FEN,
  UCI, Zobrist, perft e Chess960.
- `mychess-pgn` usa **`pgn-reader` 0.29** (leitor PGN em streaming, padrão
  *visitor*) para importar; o **escritor** de PGN é nosso, para reproduzir as
  opções de exportação do Scid.
- O codec SI4 (em `mychess-db`) mantém **sua própria lista de peças no estilo
  Scid** ao lado da posição do `shakmaty`, para codificar e decodificar lances
  exatamente como o Scid (ver ADR 0005).
- Tablebases Syzygy com **`shakmaty-syzygy`**.
- O `mychess-core` encapsula o `shakmaty`: os demais crates usam os tipos do
  `mychess-core`. Reexportar tipos simples (`Square`, `Role`, `Color`) é
  permitido.

## Alternativas consideradas

- **Escrever o próprio gerador de lances**, portando o `position.cpp` do Scid:
  mais trabalho e mais bugs, sem ganho.
- **`cozy-chess`/`chess` (MIT)**: deixariam a licença livre, mas são menos
  completos (SAN, Chess960, variantes, PGN) e menos mantidos.

## Consequências

- O MyChess precisa ser GPL-3.0-or-later (ver ADR 0004).
- Perft e testes de propriedade validam o *encapsulamento* e o codec SI4, não
  o `shakmaty` em si.
- Atualizações do `shakmaty` (pré-1.0) são planejadas e testadas como qualquer
  mudança de dependência.

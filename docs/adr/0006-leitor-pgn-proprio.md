# ADR 0006: Leitor de PGN próprio e tolerante (emenda à ADR 0003)

- **Status:** aceita (2026-10-21), pelo orquestrador, com base na especificação `docs/specs/m1-3-pgn.md`. O dono do projeto pode revertê-la.
- **Emenda:** a ADR 0003 previa ler PGN com a crate `pgn-reader`. Esta ADR a substitui **somente nesse ponto**; o resto da 0003 continua valendo.

## Contexto

O Scid lê PGN de forma muito tolerante (`src/pgnparse.cpp`): aceita `e8Q`, `ed`,
`0-0`, `OO`, sufixos como `+-`, comentários `;`, codificações mistas, lixo entre
partidas, e **continua** depois de um lance ilegal, anexando o comentário
`Error reading move: <token>` e registrando linha e coluna. A `pgn-reader` 0.29
é um *visitor* rápido, mas limitado ao SAN estrito: descarta em silêncio o que
não reconhece, não informa a posição de tokens inválidos e não permite o
comportamento de recuperação por linha do Scid.

## Decisão

- `mychess-pgn` implementa **lexer e leitor próprios**, com as regras de
  `pgnparse.cpp`, mais um separador de partidas (*chunker*) em streaming.
- A `pgn-reader` deixa de ser a base do leitor e vira **`dev-dependency`**:
  oráculo de testes diferenciais (em PGN bem formado as duas devem produzir os
  mesmos lances, NAGs e resultados) e referência de desempenho.
- O **escritor** continua próprio (já era na ADR 0003).
- Codificação: `chardetng` + `encoding_rs`; `.pgn.gz`: `flate2` (todas já
  aprovadas em `[workspace.dependencies]`).

## Alternativas consideradas

- **Usar a `pgn-reader` como base** e tratar a tolerância em volta: o visitor
  não entrega os tokens crus nem as posições, então a tolerância teria de ser
  reconstruída por cima, com duas passadas.
- **Contribuir mudanças para a `pgn-reader`**: o escopo de tolerância do Scid
  (recuperação por linha, avisos tipados) não é objetivo daquela crate.

## Consequências

- Mais código próprio para manter, compensado por testes diferenciais contra a
  `pgn-reader` e por um corpus de casos do Scid.
- Meta de desempenho a calibrar na primeira medição (spec D38); benchmark
  `criterion` obrigatório, inclusive para o `parse_san` da fatia 1.
- A licença não muda: a `pgn-reader` (GPL-3.0-or-later) segue só em testes.

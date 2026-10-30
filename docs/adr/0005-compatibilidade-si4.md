# ADR 0005: Compatibilidade byte a byte com bancos SI4 do Scid vs PC

- **Status:** aceita (2026-10-21)

## Contexto

Usuários do Scid vs PC têm bancos SI4 (`.si4` índice, `.sn4` nomes, `.sg4`
partidas) com milhões de partidas. O MyChess só substitui o Scid se abrir esses
bancos diretamente e gravá-los sem corromper nada, inclusive para quem
continuar usando os dois programas.

## Decisão

- `mychess-db` **lê e grava SI4 compatível byte a byte** com o Scid vs PC 4.27.
  A especificação vem do código de referência (`src/index.*`, `src/namebase.*`,
  `src/gfile.*`, codificação de partidas em `src/game.cpp`) e fica documentada
  em `docs/formatos/si4.md` antes da implementação.
- Testes de *round-trip* (ler → gravar → comparar bytes) contra bancos criados
  pelo próprio Scid vs PC (`cargo xtask compat`).
- Gravação segura: escrita em arquivo temporário e troca atômica; nunca
  corromper o banco em caso de falha ou queda de energia.
- Um formato nativo próprio só se o SI4 impedir alguma funcionalidade, com ADR
  própria.

## Consequências

- O codec precisa reproduzir detalhes internos do Scid (lista de peças,
  limites de tamanho de nomes, flags, datas parciais, ordem big-endian).
- Os limites do SI4 (número máximo de partidas, tamanho de nomes etc.) passam a
  ser limites do MyChess enquanto o banco for SI4.

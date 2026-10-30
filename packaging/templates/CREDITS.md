# Créditos do pacote MyChess 0.1.0-alpha

O MyChess é software livre sob a GPL-3.0-or-later (arquivo `LICENSE`). Código-fonte
completo: <https://github.com/Adriano-Master/mychess>. As dependências Rust e as
licenças delas estão em `LICENSES-THIRD-PARTY.txt`; as licenças das peças, fontes e
ícones embutidos estão em `LICENSES-ASSETS.md`.

## Sons (AGPL-3.0-or-later)

Os quatro pacotes de sons embutidos no executável (`sfx`, `nes`, `futuristic` e
`piano`) são do Lichess, de autoria de **Enigmahack**, sob a licença
**GNU AGPL-3.0-or-later**
(<https://github.com/lichess-org/lila/blob/master/COPYING.md>). Essa licença vale
**somente para esses arquivos de áudio**, não para o código do MyChess. Como os
arquivos são embutidos no executável, quem recebe o binário recebe também uma obra
combinada com material AGPLv3+; a combinação com a GPL-3.0-or-later é permitida pelo
§13 de ambas as licenças. Os arquivos originais e o código-fonte correspondente estão
no repositório do projeto, em `assets/sounds/`, e a decisão está registrada na ADR 0015
(`docs/adr/0015-pacotes-de-sons-e-decodificadores.md`).

## Bases de dados (pasta `MyChess/Databases/`)

As partidas de xadrez são fatos; os arquivos abaixo são compilações feitas e
fornecidas pelo autor do projeto. **A procedência original das partidas não está
documentada**: o conteúdo indica uma mistura de partidas de torneios presenciais com partidas
online, mas nenhuma licença de uso foi
declarada nos arquivos. O dono do projeto deve confirmar a origem antes de
considerá-las livres de restrições.

| Arquivo | Conteúdo | O que a inspeção encontrou |
|---|---|---|
| `Capablanca.si4`, `.sg4`, `.sn4` | 599 partidas de José Raúl Capablanca | Todas as 1889 anotações são saídas de engine (avaliações numéricas e variantes), marcadas `Annotator "Stockfish 19"`; nenhum comentário de autor humano. |
| `Carlsen.si4`, `.sg4`, `.sn4` | 7818 partidas de Magnus Carlsen | Só cabeçalhos e lances; sem comentários, variantes ou NAGs. Eventos online aparecem como `chess.com`, `chess24.com` e `lichess.org` no campo Site. |
| `SicilianNajdorf6Bg5.pgn` | 33298 partidas da Siciliana Najdorf, linha 6.Bg5 | Só cabeçalhos e lances; sem comentários, variantes, NAGs nem Annotator/Source. Cerca de 5665 partidas têm Site `chess.com INT` e 358 `lichess.org INT`; as demais são de torneios presenciais. |

Nenhum dos três conjuntos contém comentários de terceiros. Os dados dos sites online
(chess.com, lichess.org, chess24.com) pertencem aos seus respectivos serviços e
jogadores; os lances de uma partida são fatos, mas respeite os termos de cada site ao
reutilizar os dados.

## Engine de xadrez: Stockfish 10 (somente no pacote Linux)

O pacote Linux inclui, em `MyChess/Engine/stockfish/`, o **Stockfish 10** (2018) da
equipe do Stockfish (<https://github.com/official-stockfish/Stockfish>), sob a
**GNU GPL v3**, **sem modificação**. Os arquivos (build "64 POPCNT" e o
`stockfish-10-src.zip` com o código-fonte correspondente) foram fornecidos pelo dono
do projeto (os nomes dos arquivos indicam builds oficiais, mas a origem exata não foi
verificada; o código-fonte incluído permite conferir); os SHA-256 estão no `LEIA-ME.txt`
daquela pasta. O pacote Windows **não** inclui engine (veja o `LEIA-ME.txt`).

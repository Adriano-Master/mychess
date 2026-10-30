# Registros de decisão de arquitetura (ADRs)

Cada decisão estrutural do MyChess fica registrada aqui, com o contexto, a
escolha, as alternativas consideradas e as consequências.

| ADR | Decisão | Status |
|---|---|---|
| [0001](0001-arquitetura-camadas-mvu.md) | Workspace em camadas + MVU (Elm Architecture) na interface | aceita |
| [0002](0002-gui-iced.md) | Interface gráfica com iced 0.14 | aceita |
| [0003](0003-nucleo-shakmaty.md) | Regras de xadrez e PGN sobre `shakmaty` e `pgn-reader` | aceita; emendada pela 0006 (leitor de PGN próprio) |
| [0004](0004-licenca.md) | Licença GPL-3.0-or-later | aceita pelo dono (2026-10-29); o `COPYING` do Scid não indica versão da GPL (GPLv2 §9), então GPL-3.0-or-later é possível |
| [0005](0005-compatibilidade-si4.md) | Compatibilidade byte a byte com bancos SI4 do Scid vs PC | aceita |
| [0006](0006-leitor-pgn-proprio.md) | Leitor de PGN próprio e tolerante (emenda à 0003) | aceita pelo orquestrador; revertível pelo dono |
| [0007](0007-fonte-de-banco-compartilhada.md) | Fonte de banco compartilhada (`Arc<Mutex<_>>`) nos slots de banco | aceita pelo orquestrador; revertível pelo dono |
| [0008](0008-layout-com-grupos-de-abas.md) | Layout dos painéis como árvore de grupos de abas | aceita pelo orquestrador; revertível pelo dono |
| [0009](0009-busca-de-padroes-sem-cql.md) | Busca de táticas e padrões própria, sem a linguagem CQL | aceita pelo usuário (2026-10-24) |
| [0010](0010-cliente-online.md) | Cliente online `mychess-online`: Lichess pelo Board API, `reqwest` + `rustls`, token no chaveiro, bloqueio de assistência | aceita pelo orquestrador; revertível pelo dono (TLS a confirmar com `cargo deny`) |
| [0011](0011-funcionalidades-fora-do-escopo.md) | Funcionalidades retiradas ou fora do escopo: gráfico de pontuação, semáforo MultiPV e xadrez por correspondência | decisão do dono (2026-10-25); retirada ainda não executada |
| [0012](0012-tablebases-syzygy.md) | Tablebases: crate `mychess-tablebase`, Syzygy local (`shakmaty-syzygy`, sem `mmap`) e consulta online do Lichess pelo cliente do `mychess-online` | aceita pelo orquestrador; revertível pelo dono |
| [0013](0013-audio-rodio.md) | Áudio: `rodio` (só `cpal`) atrás da feature `audio` do `mychess-gui`, sons sintetizados | aceita pelo orquestrador; revertível pelo dono |
| [0014](0014-tabuleiros-eletronicos.md) | Tabuleiros eletrônicos: crate `mychess-eboard`, `serialport` sem udev e driver como processo | aceita pelo orquestrador; revertível pelo dono |
| [0015](0015-pacotes-de-sons-e-decodificadores.md) | Pacotes de sons e decodificadores OGG/MP3 (`symphonia`); assets AGPL-3.0-or-later aceitos para áudio | proposta pelo orquestrador; revertível pelo dono |
| [0016](0016-exportacao-visualizador-html-e-imagens.md) | Exportação: visualizador HTML próprio (arquivo autônomo por volume), SVG nativo do tabuleiro e PNG por `resvg` | aceita pelo orquestrador (2026-10-29) |
| [0017](0017-torneios-arena-lichess.md) | Torneios Arena do Lichess: escopo `tournament:write`, só ritmos do Board API, acompanhamento por *polling* e painel `PaneKind::Arena` | proposta |
| [0018](0018-distribuicao-e-pasta-do-usuario.md) | Distribuição: pasta `MyChess` na pasta pessoal (Engine, Databases, Books) criada na primeira execução, Stockfish embarcado, Windows e Linux na fase 1, sem macOS nem assinatura | aceita pelo dono (2026-10-29) |

Para registrar uma nova decisão: copie o formato de uma ADR existente, use o
próximo número e adicione a linha nesta tabela. Para mudar uma decisão, escreva
uma nova ADR que **substitui** a anterior (marque a antiga como "substituída
por NNNN"). ADRs não são apagadas.

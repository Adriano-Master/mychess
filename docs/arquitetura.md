# Arquitetura do MyChess

Este documento descreve **como o MyChess é organizado e por quê**. As decisões
estruturais têm registro próprio em [`docs/adr/`](adr/). As regras de dependência
entre crates (seção 3) são verificadas automaticamente por `cargo xtask arch`.

## 1. Objetivos e restrições

| Objetivo | Consequência no projeto |
|---|---|
| Todas as funcionalidades do Scid vs PC 4.27 | Roteiro por marcos ([`roteiro.md`](roteiro.md)); comportamento especificado a partir do código de referência |
| Multiplataforma: Linux (X11/Wayland), Windows, macOS | Rust puro sempre que possível; código específico de plataforma isolado; CI nas três plataformas |
| Interface gráfica moderna | iced 0.14 (GPU via wgpu, fallback por software), temas claro/escuro, HiDPI, peças em SVG |
| Bancos com milhões de partidas | Arquivos mapeados em memória, filtros compactos, buscas paralelas, nenhum trabalho pesado na thread da UI |
| Compatibilidade com o ecossistema | Lê e grava bancos SI4 do Scid byte a byte; PGN, EPD, Polyglot, engines UCI/XBoard |
| Testável sem janela | Toda a lógica de aplicação vive fora da GUI e é testada como funções puras |

## 2. Visão geral: camadas + MVU

O MyChess é um workspace Cargo em **camadas**. A interface segue o **MVU**
(Model–View–Update, ou *The Elm Architecture*), o padrão nativo do iced.

```
┌───────────────────────────────────────────────────────────────────┐
│ mychess-gui                                   VIEW (apresentação) │
│ iced: janelas, painéis, widgets (tabuleiro, lista virtual,        │
│ notação, gráficos), temas, traduções. Converte eventos em         │
│ Message e executa os Effects como Tasks/Subscriptions do iced.    │
└──────────────▲───────────────────────────────┬────────────────────┘
               │ lê o estado (&AppState)       │ envia Message
┌──────────────┴───────────────────────────────▼────────────────────┐
│ mychess-app                          CONTROLLER + estado da app   │
│ update(&mut AppState, Message) -> Vec<Effect>   (puro, rápido)    │
│ perform(Effect) -> Message                      (async, com I/O)  │
│ casos de uso, preferências, sessões de análise, view-models       │
└───────┬──────────────┬───────────────┬───────────────┬────────────┘
        │              │               │               │
┌───────▼──────┐ ┌─────▼───────┐ ┌─────▼──────────┐ ┌──▼──────────────┐
│ mychess-db   │ │ mychess-pgn │ │ mychess-engine │ │ book, tablebase │
│ SI4, buscas, │ │ PGN, EPD    │ │ UCI, XBoard,   │ │ online, ...     │
│ árvore, ...  │ │             │ │ processos      │ │ (planejados)    │
└───────┬──────┘ └─────┬───────┘ └─────┬──────────┘ └──┬──────────────┘
        └──────────────┴───────┬───────┴───────────────┘
                       ┌───────▼────────┐
                       │ mychess-core   │  MODEL (domínio do xadrez)
                       │ posição, lances│
                       │ notação, árvore│
                       │ de partida, ECO│
                       └────────────────┘
```

### 2.1 Por que MVU e não MVC clássico

O MVU tem as mesmas três responsabilidades do MVC, com o fluxo de dados em
**uma única direção**:

| MVC | No MyChess | Onde fica |
|---|---|---|
| **Model** | Domínio (`mychess-core`, `-db`, `-pgn`, `-engine`) + `AppState` | crates de domínio e `mychess-app` |
| **Controller** | `update(estado, mensagem) -> efeitos` | `mychess-app` |
| **View** | `view(&estado) -> Element<Message>` | `mychess-gui` |

No MVC clássico, a View *observa* o Model e o Controller o altera. Isso cria
referências cruzadas e estado mutável compartilhado. Em Rust, esse desenho vira
`Rc<RefCell<…>>` espalhado, erros de empréstimo em tempo de execução e
dificuldade para testar. No MVU:

1. a **View** é uma função do estado: só lê e devolve a árvore de widgets;
2. cada interação vira uma **Message** (um `enum`), ou seja, dados;
3. **`update` é o único lugar que altera o estado**;
4. efeitos colaterais (abrir arquivo, falar com a engine, timers) são
   devolvidos como **`Effect`** (dados) e executados fora do `update`.

Na prática é um MVC com fluxo unidirecional: separação de responsabilidades
igual, sem estado compartilhado mutável e com o Controller testável sem janela.
Decisão registrada em [ADR 0001](adr/0001-arquitetura-camadas-mvu.md).

### 2.2 Núcleo funcional, casca imperativa

- `mychess_app::update` é **puro e síncrono**: não faz I/O, não bloqueia, não
  dorme e não faz cálculo longo. Recebe uma `Message`, altera o `AppState` e
  devolve `Vec<Effect>`.
- `mychess_app::perform(effect)` é **assíncrono**: executa o efeito usando os
  crates de domínio (ler banco, iniciar engine...) e devolve a `Message` com o
  resultado. Trabalho pesado de CPU vai para `rayon` ou `spawn_blocking`.
- Fluxos contínuos (saída de engine, progresso de importação) são *streams* de
  `Message` expostos pelo `mychess-app`. A GUI os assina como `Subscription`.
- `mychess-gui` é a casca: guarda o `AppState` e o estado puramente visual
  (hover, arraste, rolagem, ids de janela), chama `update` e transforma cada
  `Effect` em `Task::perform(perform(effect), …)`.

Assim, a GUI **não conhece** SI4, PGN nem processos de engine. Ela só vê o
`mychess-app` (mensagens, estado e *view-models*, como as linhas da lista de
partidas já formatadas) e tipos simples do `mychess-core` (casa, peça, lance).

### 2.2.1 Contrato dos `NodeId` no undo/redo

`mychess_core::Game` é uma árvore em arena e `Game::clone` é um **snapshot
barato** (usado no undo). Um `NodeId` carrega índice, geração e a identidade da
partida; o `Clone` **mantém** essa identidade, para que cursores guardados antes
do snapshot continuem válidos depois de restaurá-lo. Consequências para o
`mychess-app`:

- Um id criado **antes** do clone vale nos dois lados.
- Um id criado **depois** do clone vale só no lado em que nasceu. Se o snapshot
  restaurado voltar a ser editado, o mesmo `(índice, geração)` pode ser
  realocado para outro nó, e um id da linha do tempo abandonada passaria a
  apontar para o lance errado, sem erro.
- Portanto o estado da aplicação deve **guardar o cursor junto de cada
  snapshot** (ou como caminho, via `Game::path_to`) e **descartar** qualquer
  `NodeId` guardado em caches da interface (seleção, destaque, rolagem, "refazer")
  ao desfazer ou refazer.
- `Game::truncate_start` e uma nova partida têm identidade nova: ids antigos
  são recusados (`InvalidNode`).

### 2.3 Exemplo: o usuário arrasta uma peça

1. O widget do tabuleiro emite `UiMessage::Drop { from, to }`. A GUI resolve a
   promoção (diálogo) e envia `app::Message::PlayMove(..)`.
2. `app::update` valida o lance com o `mychess-core` e decide entre nova
   variação, sobrescrever ou nova linha principal, como o Scid vs PC faz. Depois
   atualiza a partida e devolve, por exemplo,
   `[Effect::AnalyzePosition(..), Effect::PlayMoveSound(..)]`.
3. A GUI executa os efeitos como `Task`s. Os resultados voltam como `Message`s.
4. `view` redesenha o tabuleiro, a notação e os painéis afetados.

## 3. Crates e regras de dependência

| Crate | Responsabilidade | Pode depender de | Situação |
|---|---|---|---|
| `mychess-core` | Posição e regras (sobre `shakmaty`), lances, SAN/FEN/UCI, NAGs, árvore de partida (variações, comentários, marcações), cabeçalhos, ECO (`Eco`, `EcoBook`: texto do `.eco` e classificação, sem E/S), Chess960 (ver 3.1) | — | criado |
| `mychess-pgn` | Leitor de PGN próprio e tolerante (separador de partidas em streaming + análise pura por partida, ADR 0006); escritor de PGN com as opções do Scid; EPD (`EpdBook`: leitura, escrita, consultas); leitura do arquivo `.eco` (decodificação, `.gz`); escritores de documento `export` (HTML por partida, LaTeX/`xskak`, tabela NAG→LaTeX; M7-4a); codificações (`chardetng`/`encoding_rs`) e gzip (`flate2`). `pgn-reader` só em `dev-dependencies` (oráculo de testes) | core | criado |
| `mychess-db` | Formato SI4 (`.si4/.sn4/.sg4`), PGN como banco, filtros, buscas, árvore de aberturas, ordenação, duplicatas, compactação, *spelling*, exportação de arquivos (PGN, HTML, LaTeX, EPD do filtro, lista de partidas, CSV, volumes do visualizador HTML; M7-4a) | core, pgn | criado |
| `mychess-engine` | Protocolos UCI (M4-1: `Engine`/`EngineEvents` sobre uma tarefa de sessão com máquina de estados, parser próprio, processo com `tokio::process`, log em anel, engine falsa em `testing`) e XBoard (M4-2: `xboard/` com `features`, `quirks`, `thinking`, `format`, `handshake` e `session`; mesma API `Engine`, escolhida por `EngineConfig.protocol`; `detect_protocol`/`probe_xboard`; engine falsa XBoard em `testing`), sessões de análise, torneios entre engines. Mais detalhes em 3.2 | core | UCI e XBoard criados |
| `mychess-app` | Estado da aplicação, `Message`, `update`, `Effect`/`perform`, preferências, view-models | core, pgn, db, engine, book, online, tablebase (+ planejados abaixo); a dependência em `online` existe desde a M8-3b (módulo `online/`: sessão, bloqueio de assistência, importação) | criado |
| `mychess-gui` | Binário `mychess`: iced, janelas e painéis, widgets, temas, i18n, assets | app, core | criado |
| `mychess-book` | Livros Polyglot: leitura, escrita, edição e ajuste (usado pelo `mychess-app` no jogo contra a engine, M8-1, no `BookProbe` da anotação e do relatório e na janela do livro, M5-6) | core | criado (núcleo, M5-5; janela, M5-6) |
| `mychess-tablebase` | Syzygy local (`shakmaty-syzygy`, sem `mmap`), consulta online do Lichess (cliente HTTP do `mychess-online`, com cache), resultado e ordenação dos lances, gerador de posições de treino (M8-8, ADR 0012) | core, online | criado (M8-8a) |
| `mychess-eboard` | Tabuleiros eletrônicos (M8-10, ADR 0014): protocolo do Novag Citrine (porta serial com `serialport`, sem udev), protocolo do *input engine* (DGT; processo filho), lance do tabuleiro → lance legal, sessões assíncronas com *timeouts* e dublês em `testing` | core | criado (M8-10a) |
| `mychess-online` | Cliente do Lichess pelo Board API (M8-3a, ADR 0010): modelos tolerantes, NDJSON (`LineFramer`), `Client` HTTP (`reqwest` + `rustls`/`ring`) com `RequestGate` (uma requisição por vez, 429 com *backoff*, fila prioritária para lances), streams com vigilância de silêncio e reconexão, `TokenStore` (chaveiro, arquivo 0600, memória) e servidor falso em `testing`. Depois: FICS (com timeseal) | core | criado (M8-3a: Lichess) |
| `mychess-cli` | Ferramentas de linha de comando (equivalentes a `pgnscid`, `scmerge`, `scidt`) | core, pgn, db (não usa o `app`: a CLI não carrega estado de interface) | criado |
| `xtask` | Automação do workspace (`cargo xtask …`), só `std` | — | criado |

Regras (dependências **normais e de build**; `dev-dependencies` são livres):

1. As dependências só apontam **para baixo** no diagrama. `mychess-core` não
   depende de nenhum crate interno.
2. **Nenhum crate abaixo de `mychess-gui` depende de iced, winit, rfd** ou de
   qualquer toolkit gráfico.
3. A GUI só usa `mychess-app` e `mychess-core`. Se ela precisar de algo de
   `db`/`engine`, o `app` expõe um view-model ou reexporta o tipo.
4. Ao criar um crate: adicione a linha na tabela acima **e** em `ALLOWED_DEPS`
   (`xtask/src/main.rs`), ou use a skill `/novo-crate`. O `cargo xtask arch`
   falha se houver crate sem regra, regra para crate inexistente ou dependência
   proibida, inclusive as opcionais (features) e as específicas de plataforma.

### 3.0 Slots de banco e Clipbase em `mychess-app` (M3-4a)

`AppState.bases` (`Bases`) guarda 9 posições: 0 a 7 para arquivos (mostrados
como 1 a 8) e 8 para a Clipbase, sempre presente. Cada `BaseSlot` tem a fonte
(`BaseSource`: `Si4Database`, `PgnDatabase` ou `Clipbase`, atrás de
`SharedSource`), o `Filter`, o `ListState`, o número da partida atual e, quando
não é o slot atual, a partida que estava em edição (`parked`). O editor
(`GameSession`) é um só: trocar de slot guarda a partida (e o cursor e o
"alterada") no slot que sai e restaura a do slot que entra.

- Abrir um arquivo e ler uma partida são `Effect` (`OpenBase`, `LoadBaseGame`),
  executados em `spawn_blocking`; o `update` nunca abre arquivo. O progresso e
  o cancelamento do carregamento vão por `OpenControl`.
- `SharedSource` é `Arc<RwLock<_>>`: é o único estado compartilhado mutável da
  camada (ADR 0007, emenda da 4c), porque um banco é grande demais para ir
  dentro de um `Effect` e a leitura de um registro exige `&mut`. `read()`
  (índice e nomes) e `lock()` (registro, Clipbase) duram um registro ou um
  bloco de 16 384 partidas; a `view` usa `try_read()` e nunca espera. O
  `update` altera a Clipbase em operações curtas, e o job de cópia a
  acrescenta em `perform`.
- A Clipbase é incremental: copiar uma partida custa a sua codificação, não o
  tamanho da Clipbase (`NameBase::append_name` no `mychess-db`); `push_many`
  serve a cópia em lote.
- Sair (`Quit`) pergunta também pelas partidas alteradas guardadas em outros
  slots, trazendo cada slot ao editor; um banco que chega com uma pergunta
  aberta é instalado mas não ativado.
- O progresso da abertura é um atômico (`AppState::base_opening`); a GUI lê por
  uma `Subscription` periódica.
- A lista de partidas lê só o índice e a base de nomes (em memória):
  `Bases::list_page(slot, primeira, quantidade) -> Vec<GameRow>`.
- `AppState::open_pgn_in_slots` liga a migração do M2 (D503): "Abrir" e os
  arquivos recentes abrem um slot em vez do `GameChooser`.

#### Ações da lista (M3-4c)

- `SharedSource` é um `RwLock`: `read()` (índice e nomes) deixa a `view` e uma
  busca longa lerem ao mesmo tempo; `lock()` (escrita) só para ler registros de
  partidas.
- `ListState` guarda por slot: posição, texto da busca, "ignorar maiúsculas",
  a âncora (`selected`) e o conjunto `selection`, e a ordenação. A visão
  ordenada (`ViewOrder`) é o vetor de todas as partidas na ordem da coluna
  mais o cache `visible` (as do filtro); `BaseSlot::nth_visible`,
  `position_of` e `visible_range` respondem na ordem que a lista mostra, com
  ou sem ordenação.
- `BasesMessage::List(ListMessage)` traz as ações. As curtas (selecionar,
  remover do filtro, resetar, negar, "Set filter") são puras no `update`
  (`state/list_flow.rs`). As longas são `Effect`: `SearchList` (Find e Filter,
  com `+`), `SortList`, `CopyToClipbase` e `ToggleDeleted` (abre um
  `Si4Writer` de vida curta, grava as marcas, solta-o e reabre o
  `Si4Database` do slot). Um job por vez (`AppState::list_job`, com
  `OpenControl` para progresso e cancelamento); o resultado só substitui o
  filtro/ordem ao terminar.
- Colunas: `ListColumn` (22), `ColumnSetup` (alinhamento e largura) e
  `GameListPrefs` em `[game_list]` das preferências (global).

#### Navegador de partidas (M3-4e)

- `AppState.browser: Option<BrowserState>` (`bases/browser.rs`): as partidas
  lidas como `Game` com o nó de cada ply da linha principal (a posição vem do
  `mychess-core`, a `view` não lê registro), o ply e a orientação de cada
  tabuleiro, o modo (uma partida ou grade) e o `RequestId` da leitura. Conta em
  `modal_open` (`other_modal_open` exclui o navegador).
- `BasesMessage::Browse(BrowseMessage)`; `Effect::ReadBrowserGames` lê em
  `spawn_blocking` (bloqueio de escrita por partida, progresso e cancelamento
  por `OpenControl`) e `Effect::BrowserTick` é o temporizador do autoplay. A
  resposta de uma leitura de navegador fechado ou reaberto é descartada.
- Carregar no tabuleiro principal usa o fluxo `LoadGame` com
  `BrowserLanding { ply, flipped, close_browser, browser }` em
  `Replacement::FromBase`; só ao entrar no editor o app ajusta
  `board_flipped` (o shell deriva a orientação dele) e fecha o navegador que pediu.
  A geração do autoplay é global (`flow.next_browser_generation`).
- GUI: `windows/browser.rs` (camada modal, `BrowserUi` com um `BoardCaches` por
  partida, teclas em `key_message`) e `shell/browser_glue.rs`.

#### Seletor de bancos e cópia entre bancos (M3-4d)

- `ListMessage::CopyGames { from, to, scope }` copia o filtro (`CopyScope::Filter`,
  arrastar uma aba sobre outra) ou a seleção (`CopyScope::Selection`, arrastar
  linhas sobre uma aba) para o fim de outro banco. Recusas (`CopyRefusal`) e
  resultado saem como `BasesNotice` (`CopyRefused`, `FilterCopied`,
  `CopyStopped`) que nomeiam origem e destino (`CopyEnd`).
- Com a preferência `editing.confirm_copy` ligada e destino que não seja a
  Clipbase, o `update` guarda um `CopyPrompt` (`AppState::copy_prompt`, conta
  como `modal_open`) e espera `ListMessage::AnswerCopy`; a resposta revalida tudo.
- `Effect::CopyGames` roda `copy_games_to` (`bases/jobs.rs`) em `spawn_blocking`:
  lê as partidas uma a uma (`lock()` por partida), codifica com `PreparedGame` e
  grava em lotes de 5 000: `Clipbase::push_many` (limite validado antes de ler)
  ou, para `.si4`, um `Si4Writer` de vida curta com `add_prepared_batch`; depois
  reabre o `Si4Database` e devolve a fonte nova (`CopyOutcome`/`Reloaded`). O
  estado do slot de destino só muda quando a resposta chega: fonte e carimbo
  trocados, filtro cresce com `append(1)`.
- Fechar um slot do job (ou sair) não solta o job: ele fica `closing` até a
  resposta (D588/D589); "Copiar para a Clipbase" usa o mesmo job e preserva as
  flags da origem.
- A GUI guarda só o arraste (`GameListUi::drag`, `TabDrag`) e o destaque da aba
  alvo; o menu de contexto das abas (`MenuTarget::Tab`) usa `BasesMessage`/
  `ListMessage` já existentes mais `PrefChange::ConfirmCopy`.

### 3.1 Chess960 em `mychess-core`

O `Position` guarda o `CastlingMode` do `shakmaty`; o núcleo foi estendido na
fatia 5 do M1 ([`specs/m1-5-chess960-fechamento.md`](specs/m1-5-chess960-fechamento.md)),
entregue em 2026-10-21:

- FEN em dois modos de leitura (`FenMode::{Standard, Chess960, Auto}`; o padrão
  é `Auto`, que só usa Chess960 quando os direitos de roque não cabem no xadrez
  padrão) e duas notações de saída (X-FEN por padrão, Shredder opcional).
- Roque como `Move::Castle { king_from, rook_from }`; UCI "rei toma a própria
  torre" no modo 960; SAN continua `O-O`/`O-O-O`.
- Posição inicial pela numeração de Scharnagl (`chess960_start(n)`, `0..=959`,
  518 = posição padrão) e o inverso `chess960_number()`. `FenMode::Auto` só
  escolhe Chess960 quando todas as letras `K/Q/k/q` resolvem para uma torre e o
  padrão não consegue expressá-las; `Position ==`/`Hash` não distinguem o modo
  (use `variant()`), e `Game::same_content` compara a variante.
- No PGN, a partida 960 sai com `Variant "Chess960"`, `SetUp` e `FEN`; o ECO
  não classifica partida 960.
- O SI4 **não** muda nesta fase: a decisão entre "Chess960 só em PGN" e
  "estender o SI4 de forma compatível" é do M3 (registro técnico na D62 da
  especificação e em `roteiro.md`).

### 3.2 Engines em `mychess-engine` (M4-1)

- **Uma sessão por engine**: `Engine::start`/`launch` lança o processo e uma
  tarefa de sessão (`uci::session`) que o possui. `Engine` (clonável) só envia
  `EngineCommand` por `try_send` (nunca bloqueia); `EngineEvents` entrega
  `EngineEvent` (`Ready`, `SearchStarted`, `Info`, `BestMove`, `Warning`,
  `Exited`). A GUI e o `mychess-app` não conhecem o protocolo.
- **Máquina de estados** `Handshake → Configuring → Ready ⇄ Searching →
  Stopping`, mais `Barrier` (esperar `readyok`). Comandos fora de `Ready` entram
  numa fila que coalesce (o último `Search` vence). O `bestmove` é a barreira
  entre buscas; todo evento de busca leva um `SearchId`, e o que sobra de uma busca
  trocada nunca chega ao consumidor.
- **Camadas internas**: `uci/parse.rs` (texto → dados, sem pânico, sem posição),
  `uci/convert.rs` (valida lances contra a posição e converte a pontuação para o
  ponto de vista das brancas), `uci/format.rs` (`position`/`go`), `wire.rs`
  (leitor e escritor sobre qualquer `AsyncRead`/`AsyncWrite`), `process.rs`
  (`CommandPlan` puro + `tokio::process`, `kill_on_drop`), `coalesce.rs` (no
  máximo um `info` por linha MultiPV a cada 50 ms), `log.rs` (anel de 5000
  entradas), `option.rs` (tipos e validação das opções).
- **Testes**: `testing` (feature `testing`) tem a engine falsa em memória
  (`ScriptedEngine`), o par manual (`ManualEngine`) e o binário
  `mychess-fake-engine` (mesma lógica em stdin/stdout). Nada de Stockfish no CI.
- A fatia 2 (XBoard) reaproveita `protocol.rs`, `engine.rs`, `wire.rs`,
  `process.rs`, `log.rs` e `coalesce.rs`, e acrescenta `xboard/` com a sua
  própria sessão (`Handshake → Ready ⇄ Searching → Stopping`; modos Analyze
  e Go; `ping`/`pong` como barreira) e a engine falsa XBoard
  (`testing/fake_xboard.rs`). `detect_protocol` tenta UCI e depois XBoard.
  Os auxiliares de fila de eventos das duas sessões ainda são duplicados
  (dívida registrada nas specs).

## 4. Organização interna de um crate

```
crates/mychess-xxx/
├── Cargo.toml          # versões via workspace; [lints] workspace = true
├── src/
│   ├── lib.rs          # documentação do crate + API pública (pub use)
│   ├── error.rs        # enum(s) de erro com thiserror
│   └── <conceito>/     # módulos por conceito do domínio, não por "tipo de arquivo"
├── tests/              # testes de integração (API pública)
│   └── fixtures/       # arquivos de teste com licença compatível (bytes preservados)
└── benches/            # benchmarks (criterion), quando houver caminho crítico
```

- Visibilidade mínima: `pub(crate)` por padrão; `pub` só para a API do crate.
- Toda API pública tem rustdoc (o lint `missing_docs` cobra).
- Tipos do domínio no lugar de primitivos soltos (`Square`, `GameNumber`,
  `NameId`...), com construtores que validam (*parse, don't validate*).

## 5. Concorrência e assincronia

- **Thread da UI**: só `view` e `update`, que devem ser rápidos (alvo < 1 ms).
- **Runtime**: tokio (feature `tokio` do iced). Processos de engine usam
  `tokio::process` dentro do `mychess-engine`.
- **CPU pesada** (importar PGN, buscar em milhões de partidas, compactar):
  `rayon` dentro do `mychess-db`, chamado via `spawn_blocking` a partir do
  `perform`.
- **Operações longas** reportam progresso (como as barras de progresso do Scid)
  e aceitam cancelamento por um token passado pelo `app`.
- **Engines**: um processo por engine; a leitura de stdout vira um stream de
  eventos; o comando `stop` e a troca de posição são enviados por canal.

## 6. Erros e diagnóstico

- Bibliotecas: `thiserror`, com erros específicos por crate/módulo. **Entrada
  inválida nunca causa panic.** Arquivo corrompido, PGN malformado ou engine que
  responde lixo produzem `Err`.
- `panic!`/`expect` só para violação de invariante interna, com a invariante
  escrita na mensagem.
- `mychess-app` converte erros em estado de UI estruturado e traduzível (não em
  `String` pronta).
- `anyhow` apenas nos `main` (gui, cli) e no `xtask`.
- Logs com `tracing` (filtro via `RUST_LOG`) e arquivo de log no diretório de
  dados do usuário.

## 7. Persistência e configuração

- Diretórios por plataforma com `directories` (XDG no Linux, `%APPDATA%` no
  Windows, `~/Library/Application Support` no macOS). Nunca caminhos fixos.
- Preferências em TOML (`serde`), com campo de versão e migração tolerante:
  campo ausente recebe o padrão, campo desconhecido é ignorado.
- Bancos: SI4 (compatível com Scid vs PC) e PGN. Um formato nativo próprio fica
  para uma ADR futura, se o SI4 limitar alguma funcionalidade.

## 8. Formatos de arquivo

| Formato | Uso | Crate | Compatibilidade |
|---|---|---|---|
| SI4: `.si4` (índice), `.sn4` (nomes), `.sg4` (partidas) | Banco de partidas | `mychess-db` | Byte a byte com Scid vs PC 4.27 ([ADR 0005](adr/0005-compatibilidade-si4.md)) |
| `.stc` | Cache da árvore de aberturas | `mychess-db` | Ler; gravar se viável |
| PGN / PGN compactado (gzip) | Importar, exportar, abrir como banco | `mychess-pgn` | Tolerante na leitura, estrito na escrita (ver 8.1) |
| EPD | Posições com operações | `mychess-pgn` | Padrão EPD por default; dialeto do Scid (`\s`, `\\`) opcional. [`formatos/epd.md`](formatos/epd.md) |
| `.eco` (texto) | Classificação de aberturas | `mychess-core` (análise e classificação), `mychess-pgn` (arquivo) | Formato do `scid.eco`. [`formatos/eco.md`](formatos/eco.md) |
| `.ssp` | *Spelling* (correção de nomes de jogadores, eventos, locais, rodadas) | `mychess-db` | Formato do Scid |
| Polyglot `.bin` | Livros de abertura | `mychess-book` | Especificação Polyglot |
| Syzygy `.rtbw/.rtbz` | Tablebases | `mychess-tablebase` | Via `shakmaty-syzygy` |

O mapa de onde cada formato é lido e gravado no código do Scid está em
[`referencia-scid.md`](referencia-scid.md).

### 8.1 PGN: tolerante na leitura, estrito na escrita

**Leitura** (`mychess_pgn::read`, [ADR 0006](adr/0006-leitor-pgn-proprio.md)):

1. `GameChunker` corta o fluxo de bytes (puro ou gzip, detectado pelos bytes
   `1F 8B`; com ou sem BOM; `
`, `
` ou `
`) em `RawGame`s sem interpretá-los.
2. `RawGame::parse` é uma função pura que devolve `ParsedGame` (partida,
   diagnósticos, posição no arquivo); o chamador pode paralelizar e manter a ordem.
3. `PgnReader` encadeia as duas como iterador.

Nada no *conteúdo* é fatal: lance ilegal, token estranho, tag malformada e
lixo entre partidas viram `Diagnostic` (severidade, tipo, linha, coluna,
deslocamento) e o leitor segue. `PgnError` é só para erro de E/S e de gzip. O
perfil padrão de `ReadOptions` reproduz o do Scid (nomes de jogadores
normalizados, comentários com espaços colapsados, `;` mantido como comentário,
`SanMode::Tolerant`); cada ponto é uma opção.

**Escrita** (`mychess_pgn::write`): `WriteOptions` tem duas predefinições.

| Predefinição | Comportamento |
|---|---|
| `WriteOptions::default()` | PGN **estrito**: tags completas, NAG numérico (`$14`), `1. e4 e5`, 80 colunas, UTF-8 sem BOM, `\n`. |
| `WriteOptions::scid_export()` | Padrões do diálogo de exportação do Scid: variações indentadas, 75 colunas, lance nulo convertido em comentário, Latin-1. |

`write_game`/`game_to_string` não escrevem BOM; o BOM (`Encoding::Utf8Bom`), a
linha em branco entre partidas e o `flush` ficam com `PgnWriter`. Ler o que se
escreveu devolve a mesma partida, e a segunda escrita é idêntica à primeira.

**Limites contra entrada hostil** (todos configuráveis em `ReadOptions`/
`EpdOptions`; estourar um limite vira diagnóstico ou erro, nunca pânico):

| Entrada | Limite padrão |
|---|---|
| Uma partida PGN | 16 MiB (`max_game_bytes`); acima disso a partida é pulada (`Termination::Oversize`) |
| Nós de uma partida | 1 000 000 (`max_nodes_per_game`) |
| Tags de uma partida | 256 (`max_tags_per_game`); nome de tag 1 024 bytes, valor 64 KiB |
| Comentário | 1 MiB (`max_comment_len`) |
| Diagnósticos por partida | 100 (`max_diagnostics_per_game`) |
| Linha EPD | 1 MiB (ignorada com `LineTooLong`) |
| EPD em gzip descomprimido | 256 MiB (`max_decompressed_bytes`) |
| Arquivo `.eco` descomprimido | 16 MiB |
| Entrada do `.eco` | 1 MiB de texto, 1 024 meios-lances; 10 000 diagnósticos guardados |

Um `.pgn.gz` hostil custa tempo de CPU, não memória: a saída é lida em blocos
e cada partida é limitada por `max_game_bytes`.

## 9. Interface gráfica

Detalhes e convenções de código em `.claude/rules/gui-iced.md`. Direção geral:

- **Janela principal com painéis encaixáveis** (`pane_grid`): tabuleiro,
  notação, lista de partidas, árvore, engines, informações do jogador.
  Layouts salvos e restaurados. Janelas secundárias (multijanela do iced) para
  ferramentas que fazem sentido destacadas.
- **Tabuleiro** como widget próprio (canvas ou `advanced::Widget`): arrastar e
  soltar, clique-clique, setas e casas coloridas (marcações do Scid), animação,
  coordenadas, virar o tabuleiro.
- **Lista de partidas virtualizada**: só as linhas visíveis são construídas, o
  que é obrigatório para bancos de milhões de partidas.
- **Notação** com `rich_text`/spans clicáveis (lances, variações, comentários).
- **Gráficos** (avaliação, rating, estatísticas) desenhados em `canvas`.
- **Temas** claro/escuro e temas de tabuleiro; peças em SVG; fontes embutidas.
- **i18n** com Fluent (`assets/i18n/<idioma>/*.ftl`), en-US como fallback e
  pt-BR desde o início. O domínio não produz texto traduzido: devolve dados e
  enums, e a GUI traduz.

## 10. Assets e licenças

- Todo asset (peças, sons, fontes, ícones) precisa de licença compatível com a
  GPL-3.0-or-later, registrada em `assets/LICENSES.md` com origem e autor.
- **Bancos e livros que vêm com o Scid vs PC não podem ser copiados para o
  repositório**: os autores pedem contato antes de qualquer reutilização (ver
  `reference/scid_vs_pc-4.27/COPYING`). Eles servem apenas para testes locais
  de compatibilidade (`cargo xtask compat`).

## 11. Testes

Pirâmide e convenções em `.claude/rules/testes.md`. Resumo:

- unitários junto ao código; integração em `tests/`;
- propriedades (`proptest`) para *round-trips* (FEN, SAN, PGN, SI4);
- perft para geração de lances; snapshots (`insta`) para saídas textuais;
- compatibilidade com o Scid (`compat_*`, ignorados por padrão, exigem a
  referência local);
- GUI: lógica testada no `mychess-app`; widgets com `iced_test` (headless);
- benchmarks (`criterion`) nos caminhos críticos.

## 12. Empacotamento (M9)

`cargo build --profile dist` mais empacotador multiplataforma (por exemplo,
`cargo-packager`): `.deb`/`.rpm`/AppImage no Linux, `.msi` no Windows,
`.app`/`.dmg` no macOS. Inclui assinatura de código para Windows e macOS.

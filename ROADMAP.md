# Roteiro do MyChess

Plano para chegar às funcionalidades do Scid vs PC 4.27, em marcos. É um
resumo público; o estado muda com frequência.

Legenda: `[x]` implementado, `[ ]` a fazer ou ainda não concluído (a nota diz o
que falta), `[~]` fora do escopo por decisão do dono
([ADR 0011](docs/adr/0011-funcionalidades-fora-do-escopo.md)).

**Leia com cuidado:** `[x]` significa implementado e coberto por testes
automatizados e revisão automatizada, **não** validado por uso real. Várias
partes ainda esperam teste manual: Lichess e FICS contra os servidores reais,
engines reais, tabuleiros eletrônicos (Novag e DGT), placa de som, tablebases
Syzygy reais, bancos grandes e o visual dos painéis. Veja o
[`README.md`](README.md).

## M0: Fundação (concluído)
- [x] Workspace Cargo em camadas, lints rigorosos, toolchain fixado
- [x] `cargo xtask` (`ci`, `arch`, `test`, `doc`, `deny`, `compat`) e CI em Linux, Windows e macOS
- [x] Arquitetura, decisões (ADRs) e mapa do código do Scid

## M1: Núcleo de xadrez
- [x] Tipos do domínio sobre o `shakmaty`: posição, lances legais, fim de jogo
- [x] SAN (leitura tolerante), FEN, UCI e lance nulo
- [x] Árvore da partida: variações aninhadas, comentários, NAGs
- [x] Comandos em comentários (`[%draw]`, `[%arrow]`, `[%csl]`, `[%cal]`, `[%clk]`, `[%emt]`, `[%eval]`)
- [x] Cabeçalhos, datas parciais, Elo, ECO
- [x] Leitura de PGN em streaming e tolerante (BOM, CRLF, Latin-1/UTF-8, `.pgn.gz`); falta CP850 e notação de peças em outros idiomas
- [x] Escrita de PGN com as opções do Scid; a leitura de PGN anotado está abaixo da meta de desempenho
- [x] EPD: leitura e escrita
- [x] ECO: ler `scid.eco` e classificar partidas
- [x] Chess960 no núcleo e no PGN (extra: não existe no Scid vs PC 4.27 sem patch); no formato SI4 segue sem decisão
- [x] Testes de perft, de ida e volta e de compatibilidade com o `scid.eco`

## M2: Aplicativo mínimo
- [x] Janela com painéis, tema claro e escuro, en-US e pt-BR, preferências em TOML
- [x] Tabuleiro: peças SVG, arrastar e soltar, promoção, virar, último lance, animação, temas
- [x] Setas e casas coloridas no tabuleiro
- [x] Entrada de lances por teclado com autocompletar
- [x] Navegação: início, fim, passo a passo, ir para o lance, autoplay, variações
- [x] Painel de notação: lances clicáveis, variações recuadas, comentários, NAGs, menu de contexto
- [x] Editor de comentários, paleta e atalhos de NAG
- [x] Desfazer/refazer e modo tentativa
- [x] Configurar posição; copiar e colar FEN e PGN
- [x] Abrir e salvar PGN; informações da partida
- [x] Remover comentários e variações; truncar a partida
- [x] Mapa central de atalhos de teclado
- [x] Interface moderna: notação em tabela, ícones, faixas de jogador, abas; faltam alguns detalhes do Scid (modo "mini" das informações, "Limpar marcações", Chess960 na configuração de posição)
- [x] Painéis com botão de fechar e abas dinâmicas por arrasto; falta arrastar uma aba individual e a conferência visual

## M3: Bancos de dados
- [x] Formato SI4 documentado em `docs/formatos/si4.md`
- [x] Leitura de SI4 v400
- [x] Escrita de SI4: criar, adicionar e substituir partidas, flags, tipo e descrição do banco
- [x] Atualização de bancos `.si3` antigos
- [x] PGN e `.pgn.gz` abertos como banco somente leitura (em memória, até 5 milhões de partidas)
- [x] 8 slots de banco e Clipbase, seletor de bancos com arraste de partidas
- [x] Lista de partidas com 22 colunas, ordenação, busca rápida, seleção múltipla e navegador de partidas
- [x] Filtro com E/OU/negar
- [x] Busca por cabeçalho com todos os campos do Scid; buscas salvas (a janela de buscas salvas na interface fica para depois)
- [x] Importação de PGN para banco, com relatório de erros, progresso e cancelamento
- [x] Exportação de partidas e do filtro em PGN
- [x] Menu Arquivo: importar e exportar PGN com progresso e cancelamento
- [x] Marcadores com pastas, arquivos recentes, banco somente leitura
- [x] `mychess-cli`: importar, mesclar, informações, ordenar, compactar e exportar

## M4: Engines e análise
- [x] UCI: opções, `go`/`stop`, MultiPV, *ponder*
- [x] XBoard/CECP: detecção de recursos e modo de análise
- [x] Lista de engines persistida; várias engines simultâneas; atalhos F2 a F4
- [x] Janela de análise: linhas MultiPV, adicionar lance ou linha, travar posição, excluir lances, log do protocolo
- [x] Barra de avaliação e setas da variante principal
- [~] "Semáforo" MultiPV (retirada decidida, ainda não executada)
- [x] Anotação automática: por tempo ou profundidade, limiar de erro, `[%eval]`, em lote sobre o filtro
- [~] Gráfico de avaliação (barras e linha contínua): retirada decidida, ainda não executada; o gráfico de tempo fica
- [x] Análise de ameaça, *shoot-out* e *automove*
- [x] Relatório da partida estilo chess.com: classificação dos lances, precisão por lado, revisão guiada dos erros
- [x] Revisão animada com a seta do melhor lance

## M5: Aberturas
- [x] Árvore de aberturas: estatísticas, ordenação, ajustar filtro, melhores partidas, treino
- [x] Cache da árvore (`.stc`) compatível com o Scid
- [x] Máscaras de árvore (`.stm` do Scid); ainda não conferidas com arquivos reais do Scid
- [x] Livros Polyglot: janela com dois livros, ajuste de pesos, adicionar e remover linhas, criar e mesclar livros nativamente, exportar ramo para PGN
- [x] Navegador ECO, classificação do banco, busca por nome
- [x] Identificar abertura e encontrar novidade

## M6: Buscas avançadas e informações
- [x] Busca por posição (exata, peões, colunas, material; em variações; espelhada); falta teste manual com bancos grandes
- [x] Busca por material e padrões
- [x] Busca por sequência de lances com curinga
- [x] Busca de táticas e padrões, sem CQL ([ADR 0009](docs/adr/0009-busca-de-padroes-sem-cql.md)); falta teste manual e benchmarks
- [x] Localizador de jogadores e de torneios
- [ ] Informações do jogador: implementado, em revisão; sem fotos (falta decodificador de imagens)
- [ ] *Spelling* (`.ssp`): implementado, em revisão; falta testar com o `spelling.ssp` real
- [ ] Tabela de torneio (todos contra todos, suíço, eliminatório): implementada, em revisão; falta mesclar partida, título e país, e desempenho com centenas de jogadores
- [x] Estatísticas e gráficos do filtro, gráfico de rating (conferência visual pendente)
- [x] Rastreador de peças (conferência visual pendente)

## M7: Manutenção, relatórios e exportação
- [x] Manutenção de bancos: estatísticas, flags em lote, duplicatas, verificação e reparo, tags, editor de nomes, ordenação e compactação, com janela e menu Banco (teste manual pendente)
- [x] Classificar ECO de todas as partidas
- [x] Relatório de abertura, com janela (teste manual pendente)
- [x] Relatório de jogador: janela, estatísticas, aberturas, oponentes, salvar em texto, HTML e LaTeX (teste manual pendente)
- [ ] Exportação (HTML com visualizador, LaTeX, EPD, lista de partidas, imagem do tabuleiro): o núcleo está feito; faltam a aplicação, a interface e o visualizador HTML
- [x] Janela EPD (teste manual pendente)
- [x] Mesclar partida: diálogo com prévia, lote, e entradas no navegador, na árvore e nas tabelas de torneio (teste manual pendente)

## M8: Treino, jogo, online e hardware
- [x] Táticas: mate em N e "encontre o melhor lance"
- [x] Treinador de aberturas
- [x] Cálculo de variações (exercício de Stoyko)
- [x] Jogo tático contra engine UCI com força limitada
- [x] Jogo sério contra engine: relógios, livro, *ponder*, coach
- [x] Torneios entre engines: todos contra todos, carrossel, partidas em paralelo, adjudicação
- [x] Jogar no Lichess (Board API); falta testar contra o servidor real
- [x] Tablebases Syzygy (local e online) e treino de finais; testadas só com tabelas de 3 peças
- [x] FICS: console, *seeks*, observar e examinar; falta testar contra o servidor real
- [~] Xadrez por correspondência (Xfcc, relés, e-mail)
- [x] Tabuleiros eletrônicos (Novag Citrine e DGT); falta testar com hardware real
- [x] Sons e anúncio de lances, com pacotes de sons
- [x] Desistir e revanche em todos os modos de partida
- [ ] Lichess: torneios Arena; só especificado

## M9: Acabamento e distribuição
- [x] Cor de realce selecionável
- [x] Menu Jogar, reorganização dos menus e menu em faixa
- [x] Janela de aparência (tema, tabuleiro, peças, jogadores); falta revisão visual humana
- [ ] Layouts de janelas salvos, tela cheia, painéis destacáveis
- [ ] Os 19 idiomas do Scid em Fluent (hoje: en-US e pt-BR)
- [ ] Ajuda integrada: manual em Markdown, janela com índice e `F1` contextual prontos; falta a tradução en-US (M9-5d)
- [ ] Dica do dia e opções de linha de comando
- [ ] Bandeiras de países e fotos de jogadores
- [ ] Distribuição e primeira execução: pasta do usuário com engine, livro e base de exemplo, modo portátil, instalador do Windows, AppImage, `.deb` (sem binários para macOS por enquanto)
- [ ] Associação de arquivos e ícones do aplicativo
- [ ] Acessibilidade
- [ ] Benchmarks comparativos com o Scid vs PC

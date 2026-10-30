# Tablebases (spec M8-8). O Scid as tem em `tcl/tools/tablebase.tcl` (Nalimov
# e online); o MyChess usa arquivos Syzygy e a tablebase do Lichess.
action-toggle-tablebase-pane = Mostrar ou ocultar o painel de tablebases
menu-tools-tablebase = Tablebases
pane-tablebase = Tablebases
pane-close-tablebase = Fechar o painel de tablebases
file-dialog-tablebase-dir = Escolha uma pasta com arquivos de tablebase Syzygy

tablebase-source-local = Local (Syzygy)
tablebase-source-online = Online (Lichess)
tablebase-position-result = Posição: { $result }
tablebase-counts = Vitórias { $wins }, empates { $draws }, derrotas { $losses }, desconhecidos { $unknown }
tablebase-no-moves = O jogo acabou nesta posição.

tablebase-col-move = Lance
tablebase-col-result = Resultado
tablebase-col-dtz = DTZ
tablebase-col-dtm = DTM
tablebase-dtz-tip = Distância ao zeramento: meios-lances até a próxima captura ou lance de peão, mantendo o resultado. Não é o número de lances até o mate.
tablebase-dtm-tip = Distância ao mate. O Syzygy não tem esse valor; só a fonte online o dá, para algumas posições.
tablebase-mate-in = #{ $moves }
tablebase-best-tip = Melhor lance

tablebase-result-win = Vitória
tablebase-result-maybe-win = Vitória?
tablebase-result-cursed-win = Vitória amaldiçoada
tablebase-result-draw = Empate
tablebase-result-blessed-loss = Derrota abençoada
tablebase-result-maybe-loss = Derrota?
tablebase-result-loss = Derrota
tablebase-result-unknown = Desconhecido
tablebase-result-cursed-win-tip = Uma vitória, mas a regra dos 50 lances a transforma em empate.
tablebase-result-blessed-loss-tip = Uma derrota, mas a regra dos 50 lances a transforma em empate.
tablebase-result-maybe-tip = A distância está arredondada: o relógio dos 50 lances decide.

tablebase-status-not-configured = Nenhuma tablebase está ligada. Ligue os arquivos Syzygy locais ou a tablebase online abaixo.
tablebase-status-locked = As tablebases não estão disponíveis durante uma partida online.
tablebase-status-loading = Lendo as pastas de tablebases...
tablebase-status-waiting = Consultando a posição...
tablebase-status-stale = Mostrando a posição anterior.

tablebase-issue-no-tables = Nenhum arquivo Syzygy foi achado nas pastas.
tablebase-issue-too-many-pieces = { $pieces } peças: a tablebase cobre até { $max }.
tablebase-issue-too-few-pieces = Só dois reis: nada a consultar.
tablebase-issue-castling = Ainda há direito de roque: as tablebases não cobrem essa posição.
tablebase-issue-missing-table = A tabela { $material } não está disponível.
tablebase-issue-corrupt = A tabela { $material } está corrompida.
tablebase-issue-network = Sem conexão com o servidor de tablebases.
tablebase-issue-timed-out = A tablebase demorou demais para responder.
tablebase-issue-rate-limited = O servidor pede uma pausa: tente de novo em { $seconds } s.
tablebase-issue-server = O servidor de tablebases falhou.
tablebase-issue-invalid = Não foi possível entender a resposta do servidor.
tablebase-issue-internal = A tablebase falhou de modo inesperado.

tablebase-config = Configurações
tablebase-config-tip = Mostra ou oculta as configurações das tablebases
tablebase-config-local = Tablebases locais (arquivos Syzygy)
tablebase-config-online = Tablebase online (Lichess)
tablebase-config-online-privacy = Ao ligar, a posição (FEN) é enviada a tablebase.lichess.ovh. Nada é enviado enquanto estiver desligada.
tablebase-config-folders = Pastas com arquivos .rtbw e .rtbz
tablebase-config-folder-placeholder = Digite ou cole uma pasta
tablebase-config-add = Adicionar
tablebase-config-browse = Procurar...
tablebase-config-remove = Remover
tablebase-config-reload = Ler de novo
tablebase-config-dir-counts = { $wdl } WDL, { $dtz } DTZ
tablebase-config-dir-unreadable = não encontrada
tablebase-config-dir-uppercase =
    { $count ->
        [one] { $count } arquivo tem a extensão em maiúsculas (.RTBW ou .RTBZ) e é ignorado: renomeie para minúsculas
       *[other] { $count } arquivos têm a extensão em maiúsculas (.RTBW ou .RTBZ) e são ignorados: renomeie para minúsculas
    }
tablebase-config-dir-bad =
    { $count ->
        [one] { $count } arquivo inutilizável
       *[other] { $count } arquivos inutilizáveis
    }
tablebase-config-summary = Arquivos: WDL { $wdl }, DTZ { $dtz }; maior tabela: { $pieces } peças
tablebase-config-help = As tabelas não vêm com o programa. Baixe-as em tablebase.lichess.ovh/tables (3-4-5 peças: cerca de 1 GB; 6 peças: cerca de 150 GB) e escolha a pasta aqui.

tablebase-train = Treinar
tablebase-train-tip = Joga esta posição contra a tablebase; os resultados ficam ocultos
tablebase-train-stop = Parar o treino
tablebase-train-new = Nova posição
tablebase-train-new-tip = Troca a partida por uma posição aleatória deste material e treina
tablebase-train-material-placeholder = Material, por exemplo KRvKP
tablebase-train-go = Ir
tablebase-train-status =
    Treino com { $side }: { $moves ->
        [one] { $moves } lance
       *[other] { $moves } lances
    }, { $slips ->
        [one] { $slips } escorregada
       *[other] { $slips } escorregadas
    }. Posição inicial: { $grade }.
tablebase-train-thinking = A tablebase está respondendo...
tablebase-train-your-move = Sua vez.
tablebase-side-white = as brancas
tablebase-side-black = as pretas
tablebase-grade-win = vitória
tablebase-grade-draw = empate
tablebase-grade-loss = derrota
tablebase-verdict-converted = Você converteu a vitória.
tablebase-verdict-held = Você segurou o empate.
tablebase-verdict-escaped = Você conseguiu mais do que a posição valia.
tablebase-verdict-lost-as-expected = A posição estava perdida e a tablebase ganhou.
tablebase-verdict-failed = Você deixou o resultado escapar.
tablebase-verdict-inconclusive = O treino terminou sem resultado.
tablebase-end-checkmate = Xeque-mate.
tablebase-end-stalemate = Afogamento.
tablebase-end-insufficient-material = Empate: não há material para dar mate.
tablebase-end-fifty-moves = Empate pela regra dos 50 lances.
tablebase-end-repetition = Empate por repetição.
tablebase-end-out-of-range = A posição saiu da tablebase.
tablebase-outcome-summary =
    { $moves ->
        [one] { $moves } lance
       *[other] { $moves } lances
    }, { $slips ->
        [one] { $slips } escorregada
       *[other] { $slips } escorregadas
    }.
tablebase-dismiss = Fechar

tablebase-notice-busy = Termine antes a partida, o torneio, o treinador ou a janela que estiver aberta
tablebase-notice-not-at-end = Vá ao fim da linha para treinar
tablebase-notice-nothing-to-train = Ligue uma tablebase e use uma posição que não tenha acabado
tablebase-notice-bad-material = Isso não é um material como KRvKP
tablebase-notice-folder-not-saved = O nome dessa pasta não pode ser guardado nas configurações
tablebase-notice-too-many-folders = A lista de pastas está cheia

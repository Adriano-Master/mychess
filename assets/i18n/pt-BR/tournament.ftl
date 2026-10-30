# O torneio entre engines (spec M8-2b). Os textos do Scid estao em
# `comp.tcl` (Computer Tournament) e `english.tcl`.

action-tournament-engines = Torneio entre engines
menu-play-tournament = Torneio entre engines...
menu-play-tournament-resume = Retomar um torneio...
menu-play-tournament-panel = Painel do torneio
pane-tournament = Torneio

tournament-title = Torneio entre engines
tournament-yes = Sim
tournament-no = Não
tournament-start = Iniciar
tournament-resume = Retomar um torneio...
tournament-new = Novo torneio...
tournament-none = Não há torneio. Inicie um novo ou retome um salvo.

tournament-section-participants = Participantes
tournament-section-tournament = Torneio
tournament-section-time = Tempo
tournament-section-opening = Aberturas
tournament-section-engines = Engines
tournament-section-adjudication = Adjudicação
tournament-section-failures = Falhas
tournament-section-recording = Gravação

tournament-add = Adicionar engine
tournament-duplicate = Duplicar
tournament-remove = Remover
tournament-configure-engines = Configurar engines...
tournament-no-engine = Não há engine cadastrada. Adicione engines no diálogo Engines.
tournament-engine-gone = (engine ausente)
tournament-options-hint = Opções: Nome=Valor; Nome=Valor

tournament-name = Nome
tournament-site = Local
tournament-scheduling = Formato
tournament-sched-round-robin = Todos contra todos
tournament-sched-carousel = Carrossel
tournament-sched-gauntlet = Um contra todos
tournament-tested = Engine testada
tournament-cycles = Partidas por par

tournament-time-mode = Limite
tournament-time-clock = Relógio com incremento
tournament-time-movetime = Tempo por lance
tournament-time-depth = Profundidade fixa
tournament-time-nodes = Nós fixos
tournament-base = Tempo base
tournament-increment = Incremento
tournament-movetime = Tempo por lance
tournament-leeway = Tolerância
tournament-depth = Profundidade
tournament-nodes = Nós
tournament-depth-limit = Teto por lance
tournament-margin = Margem do watchdog
tournament-unit-seconds = segundos
tournament-unit-percent = %
tournament-unit-plies = meios-lances
tournament-unit-cp = centipeões
tournament-unit-mb = MB
tournament-estimate = { $games ->
    [0] { $games } partida, no máximo { $time }
    [one] { $games } partida, no máximo { $time }
   *[other] { $games } partidas, no máximo { $time }
}

tournament-opening = As partidas começam de
tournament-open-initial = Posição inicial
tournament-open-current = Posição atual
tournament-open-current-first = Posição atual, só a 1ª partida
tournament-open-book = Livro
tournament-open-suite = Suíte de aberturas
tournament-book-file = Arquivo do livro
tournament-book-pick = Lance do livro
tournament-book-weighted = Sorteio por peso
tournament-book-best = Melhor lance
tournament-book-limit = Meios-lances do livro (vazio: sem limite)
tournament-suite-file = Arquivo da suíte
tournament-suite-order = Ordem
tournament-suite-sequential = Em ordem
tournament-suite-random = Aleatória
tournament-suite-plies = Meios-lances tirados de cada partida
tournament-pair-openings = Mesma abertura nas duas partidas de um par
tournament-seed = Semente (0: uma nova)
tournament-choose-file = Escolher arquivo...
tournament-clear-file = Limpar
tournament-no-file = Nenhum arquivo escolhido
tournament-suite-dialog-title = Escolher uma suíte de aberturas
tournament-suite-filter = Arquivos EPD e PGN

tournament-ponder = Pensar no tempo do adversário (ponder)
tournament-restart-each = Reiniciar as engines antes de cada partida
tournament-hash = Hash (vazio: o salvo)
tournament-threads = Threads (vazio: o salvo)

tournament-adj-resign = Adjudicar partida ganha
tournament-adj-draw = Adjudicar partida empatada
tournament-adj-max-plies = Empate em um número de meios-lances
tournament-adj-cp = Nota
tournament-adj-moves = Lances seguidos
tournament-adj-after = Depois do lance
tournament-adj-plies = Meios-lances
tournament-adj-both = As duas engines devem concordar
tournament-adj-tablebase = Adjudicar pela tablebase Syzygy
tournament-adj-tb-pieces = Até este número de peças
tournament-unit-pieces = peças

tournament-time-loss = Partida perdida por tempo
tournament-time-loss-loss = Conta como derrota
tournament-time-loss-replay = É jogada de novo
tournament-retries = Vezes que uma partida quebrada é repetida
tournament-max-failures = Falhas antes de o torneio esperar uma decisão

tournament-comment-score = Comentário da nota
tournament-score-eval = [%eval]
tournament-score-pct = Entre colchetes
tournament-score-text = Texto simples
tournament-score-none = Nenhum
tournament-comment-clock = Relógio em cada lance
tournament-comment-time = Tempo de cada lance
tournament-claim-draws = Empate por repetição e 50 lances

tournament-resume-title = Retomar um torneio
tournament-resume-none = Não há torneio por terminar.
tournament-resume-progress = { $played } de { $total } partidas

tournament-tab-live = Ao vivo
tournament-tab-pairings = Pareamentos
tournament-tab-standings = Classificação
tournament-tab-cross = Cruzada
tournament-tab-log = Registro

tournament-pause = Pausar
tournament-continue = Continuar
tournament-stop-after = Parar depois desta partida
tournament-abort = Encerrar
tournament-leave = Sair
tournament-close = Fechar
tournament-restart = Novo torneio...
tournament-open-pgn = Abrir PGN como banco
tournament-skip-current = Pular partida
tournament-adj-white = Brancas ganham
tournament-adj-black = Pretas ganham
tournament-adj-draw-result = Empate
tournament-progress = { $done } de { $total } partidas
tournament-summary-winners = Vencedores: { $winners }

tournament-phase-running = Jogando
tournament-phase-pausing = Pausando depois do lance
tournament-phase-paused = Em pausa
tournament-phase-decision = Esperando uma decisão
tournament-phase-stopped = Parado depois de uma partida
tournament-phase-finished = Concluído
tournament-phase-aborted = Encerrado

tournament-live-none = Nenhuma partida em andamento.
tournament-live-preparing = Iniciando as engines...
tournament-live-game = Partida { $number }, tentativa { $attempt }

tournament-to-front = Para a frente
tournament-skip = Pular
tournament-redo = Jogar de novo
tournament-status-pending = Pendente
tournament-status-playing = Em andamento
tournament-status-played = { $result } ({ $how })
tournament-status-replaced = Substituída: { $result }
tournament-status-skipped = Pulada
tournament-status-withdrawn = Retirada

tournament-term-normal = normal
tournament-term-time = perda por tempo
tournament-term-rules = infração das regras
tournament-term-abandoned = abandonada
tournament-term-adjudication = adjudicação
tournament-term-unterminated = inacabada

tournament-col-rank = #
tournament-col-name = Nome
tournament-col-played = J
tournament-col-wins = V
tournament-col-draws = E
tournament-col-losses = D
tournament-col-points = Pts
tournament-col-percent = %
tournament-col-sb = SB
tournament-col-withdrawn = retirado

tournament-export = Exportar
tournament-export-csv = CSV
tournament-export-text = Texto
tournament-export-markdown = Markdown
tournament-export-html = HTML
tournament-export-save = Salvar...
tournament-export-copy = Copiar
tournament-file-standings = classificacao
tournament-file-cross = cruzada
tournament-save-dialog-title = Salvar a tabela

tournament-confirm-abort-title = Encerrar o torneio?
tournament-confirm-abort = A partida em curso é descartada, sem resultado. As partidas já jogadas ficam, e o torneio pode ser retomado depois.
tournament-confirm-adjudicate-title = Decidir a partida?
tournament-confirm-adjudicate = A partida em curso termina agora: { $result }.
tournament-confirm-skip-title = Pular a partida?
tournament-confirm-skip = A partida em curso é descartada, sem resultado, e jogada mais tarde.
tournament-confirm-close-title = Sair do torneio?
tournament-confirm-close = As engines são encerradas. O torneio fica salvo e pode ser retomado depois.
tournament-confirm-quit-title = Sair com um torneio em andamento?
tournament-confirm-quit = As engines são encerradas e a partida em curso se perde. O torneio fica salvo e pode ser retomado depois.

tournament-decision-title = Uma engine continua falhando
tournament-decision-body = { $name } falhou vezes demais. O que fazer?
tournament-decision-retry = Tentar de novo
tournament-decision-withdraw = Retirar
tournament-decision-replace = Substituir
tournament-decision-replace-with = Substituir por
tournament-decision-later = Depois
tournament-decision-reopen = Responder...

tournament-status-progress = Torneio { $name }: { $done } de { $total } partidas

tournament-fail-start = não iniciou
tournament-fail-died = parou
tournament-fail-hung = não respondeu
tournament-fail-time = perdeu por tempo

tournament-log-started = O torneio começou.
tournament-log-game-started = Partida { $number } iniciada (tentativa { $attempt }).
tournament-log-game-finished = Partida { $number }: { $result } ({ $how }).
tournament-log-engine-failed = { $name } falhou: { $reason }.
tournament-log-discarded = A partida { $number } foi descartada.
tournament-log-withdrawn = { $name } saiu do torneio.
tournament-log-replaced = Outra engine ocupou o lugar de { $name }.
tournament-log-suite-wrapped = A suíte de aberturas recomeçou (partida { $number }).
tournament-log-skipped = A partida { $number } foi pulada.
tournament-log-redo = A partida { $number } será jogada de novo.
tournament-log-paused = Em pausa.
tournament-log-resumed = Continuou.
tournament-log-stopped = Parado depois de uma partida.
tournament-log-aborted = O torneio foi encerrado.
tournament-log-finished = O torneio terminou.

tournament-notice-started = Torneio iniciado.
tournament-notice-finished = Torneio concluído. Vencedores: { $winners }
tournament-notice-paused = Torneio em pausa.
tournament-notice-resumed = O torneio continua.
tournament-notice-stopped = Torneio parado depois da partida.
tournament-notice-aborted = Torneio encerrado.
tournament-notice-engine-failed = { $name } falhou: { $reason }.
tournament-notice-too-many-failures = { $name } falhou vezes demais: o torneio espera uma decisão.
tournament-notice-not-applicable = Isso não se aplica agora.
tournament-notice-game-not-saved = Não foi possível gravar uma partida no PGN do torneio.
tournament-notice-state-not-saved = Não foi possível salvar o estado do torneio.
tournament-notice-destination-read-only = O banco de destino é somente leitura: as partidas vão só para o PGN.
tournament-notice-play-busy = Termine antes a partida contra a engine.
tournament-notice-already-running = Já há um torneio em andamento.
tournament-notice-busy = Uma anotação ou um modo de treino está ativo.
tournament-notice-missing-engines = Engines não encontradas: { $names }
tournament-notice-book-unreadable = Não foi possível ler o livro: as partidas seguem sem ele.
tournament-notice-suite-unreadable = Não foi possível ler a suíte de aberturas.
tournament-notice-resume-failed = Não foi possível ler o torneio salvo.
tournament-notice-tablebase-unavailable = A regra da tablebase está ligada, mas nenhuma tabela Syzygy pôde ser aberta: as partidas seguem sem ela.
tournament-notice-exported = Tabela exportada.
tournament-notice-export-failed = Não foi possível gravar a tabela.

tournament-error-too-few = São necessários pelo menos dois participantes.
tournament-error-too-many = Participantes demais (no máximo 32).
tournament-error-empty-name = O participante { $number } não tem nome.
tournament-error-duplicate-name = O nome { $name } é usado duas vezes.
tournament-error-cycles = O número de partidas por par não é válido.
tournament-error-tested = Escolha a engine testada do um contra todos.
tournament-error-time = O controle de tempo não é válido ({ $what }).
tournament-error-watchdog = A margem do watchdog não é válida ({ $what }).
tournament-error-opening-data = As aberturas precisam de mais dados ({ $what }).
tournament-error-start-fen = A posição do tabuleiro não pode iniciar uma partida.
tournament-error-book-limit = O número de meios-lances do livro não é válido.
tournament-error-seed = A semente não é válida.
tournament-error-title-long = O nome ou o local é longo demais.
tournament-error-adjudication = Uma regra de adjudicação não é válida ({ $what }).
tournament-error-failure-limit = O limite de falhas não é válido.
tournament-error-too-many-games = A agenda tem partidas demais.
tournament-error-book-missing = O livro não foi encontrado: { $path }
tournament-error-suite = Não foi possível ler a suíte de aberturas: { $path }

# M8-2c: partidas em paralelo, Elo e LOS, relatório de tempo, destino extra, XBoard.
tournament-concurrency = Partidas ao mesmo tempo
tournament-concurrency-hint = aconselhado { $recommended }, no máximo { $max } nesta máquina
tournament-estimate-parallel = { $games ->
    [0] { $games } partida, { $lanes } ao mesmo tempo, no máximo { $time }
    [one] { $games } partida, { $lanes } ao mesmo tempo, no máximo { $time }
   *[other] { $games } partidas, { $lanes } ao mesmo tempo, no máximo { $time }
}
tournament-destination = Copiar as partidas para
tournament-dest-none = Só o PGN
tournament-dest-slot = Base atual
tournament-dest-clipbase = Clipbase

tournament-tab-ratings = Elo
tournament-tab-time = Tempo

tournament-col-elo = Elo
tournament-col-margin = Margem
tournament-col-los = LOS %
tournament-col-performance = Desempenho
tournament-col-sample = Amostra
tournament-col-moves = Lances
tournament-col-mean = Média
tournament-col-slowest = Maior
tournament-col-overhead = Overhead
tournament-col-overruns = Excesso
tournament-col-forfeits = Bandeiras
tournament-col-clock-used = Relógio %
tournament-col-verdict = Veredito

tournament-sample-empty = sem partidas
tournament-sample-very-uncertain = muito incerta
tournament-sample-low = baixa
tournament-sample-fair = razoável

tournament-time-verdict-none = sem dados
tournament-time-verdict-ok = respeita o relógio
tournament-time-verdict-overruns = passa do tempo
tournament-time-verdict-forfeits = perdeu no tempo

tournament-ratings-note = O Elo é relativo aos adversários enfrentados (0 é um aproveitamento de 50%), com margem de 95%. O LOS é a probabilidade de ser o mais forte. Com poucas partidas os números dizem pouco.
tournament-time-note = O tempo de resposta vai do início da busca ao lance. O overhead é o excesso médio sobre um tempo fixo por lance. Uma engine XBoard não tem milissegundos: com menos de um segundo por lance ela recebe um segundo inteiro, e por isso pode aparecer como passando do tempo. Uma partida refeita não desfaz os números da primeira.

tournament-file-ratings = elo
tournament-file-time = tempo

tournament-notice-unsupported-time = { $name } fala XBoard, que não tem limite de nós. Escolha outro limite.
tournament-notice-concurrency-reduced = Partidas ao mesmo tempo reduzidas de { $from } para { $to }, o que esta máquina aguenta.
tournament-notice-destination-failed = Não foi possível copiar uma partida para a base escolhida. O PGN do torneio a tem.

tournament-error-concurrency = As partidas ao mesmo tempo devem ficar entre 1 e 16.
tournament-notice-destination-not-reopened = A partida foi copiada para o arquivo da base escolhida, mas a base não pôde ser aberta de novo. Feche-a e abra-a para ver a partida.
tournament-notice-destination-unavailable = A base escolhida não está disponível (foi fechada ou trocada, ou o torneio foi retomado): as partidas vão só para o PGN.

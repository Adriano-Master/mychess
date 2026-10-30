# O jogo contra a engine (spec M8-1b). Os textos do Scid estão em
# `english.tcl` (ComputerGame, UCIGame) e `sergame.tcl`.

action-play-engine = Jogar contra a engine

play-title = Jogar contra a engine
play-submit = Jogar
play-engine = Engine
play-no-engine = Não há engine UCI cadastrada. Adicione uma no diálogo de Engines para jogar.
play-open-engines = Abrir Engines...
play-configure-engine = Configurar engine...
play-configure-engine-tip = Abre o diálogo de Engines para adicionar ou configurar engines
play-color = Você joga de
play-color-white = Brancas
play-color-black = Pretas
play-color-random = Aleatório
play-section-time = Controle de tempo
play-section-start = Posição inicial
play-section-book = Livro de aberturas
play-section-strength = Força da engine
play-section-record = Gravação
play-store-clock = Gravar o relógio nos comentários dos lances
play-mode = Modo
play-mode-timebonus = Relógio com incremento
play-mode-movetime = Tempo por lance
play-mode-depth = Profundidade fixa
play-mode-nodes = Nós fixos
play-white-minutes = Tempo das Brancas
play-white-bonus = Incremento das Brancas
play-black-minutes = Tempo das Pretas
play-black-bonus = Incremento das Pretas
play-depth = Profundidade
play-nodes = Nós
play-movetime = Tempo por lance
play-unit-minutes = minutos
play-unit-bonus = segundos por lance
play-unit-plies = meios-lances
play-unit-nodes = nós
play-unit-seconds = segundos
play-range = ({ $min } a { $max })
play-start = Começar de
play-start-new = Partida nova
play-start-current = Posição atual
play-start-opening = Abertura específica
play-openings-pick = Abertura
play-openings-empty = Nenhuma linha válida ainda. Escreva uma por linha abaixo.
play-openings-help = Uma linha por fileira, no formato "Nome: lances", por exemplo "Siciliana: 1.e4 c5 2.Nf3 d6". A engine joga os lances da linha.
play-book-use = Jogar com livro
play-book-file = Arquivo do livro
play-book-choose = Escolher arquivo...
play-book-none = Nenhum arquivo escolhido
play-book-selection = Lance do livro
play-book-weighted = Sorteio ponderado
play-book-best = Melhor lance
play-book-dialog-title = Escolher um livro de aberturas
play-book-filter = Livros Polyglot
play-strength-elo = Limitar a força (UCI_Elo)
play-strength-skill = Skill Level
play-strength-none = Esta engine não declara opções de força.
play-strength-unknown = A engine declara estas opções ao iniciar; só são usadas se ela as suportar.

play-status-your-turn = Sua vez
play-status-engine-thinking = A engine está pensando...
play-status-question = Aguardando sua resposta
play-status-engine-lost = A engine parou de responder
play-status-paused = Partida em pausa
play-status-over = Fim da partida
play-status-closed = Partida encerrada
play-versus = Contra { $name }
play-resign = Desistir
play-resign-tip = Abandona a partida; ela é salva como derrota
play-restart = Reiniciar
play-restart-tip = Recomeça com as mesmas configurações
play-quit = Sair
play-quit-tip = Sai da partida sem resultado e sem salvar
play-resume = Continuar
play-resume-tip = Continua a partida em pausa
play-take-back = Desfazer lance
play-take-back-tip = Desfaz o seu último lance e a resposta da engine
play-offer-draw = Oferecer empate
play-offer-draw-tip = Propõe empate à engine
play-yes = Sim
play-no = Não

play-ask-book-title = Sair do livro?
play-ask-book = Seu lance não está no livro, que ainda tem lances para esta posição. Continuar assim?
play-ask-line-title = Sair da linha?
play-ask-line = Seu lance sai da linha de abertura. Continuar assim?

play-end-title = Fim da partida
play-end-won = Você ganhou.
play-end-lost = Você perdeu.
play-end-draw = Você empatou.
play-end-no-result = A partida terminou sem resultado.
play-end-result = Resultado:
play-end-save = Salvar partida
play-end-close = Fechar
play-end-read-only = O banco atual é somente leitura: a partida é salva num arquivo PGN.

play-lost-title = Engine perdida
play-lost-message = A engine não responde mais. Continue com outra engine ou encerre a partida.
play-lost-switch = Continuar com { $name }
play-lost-end = Encerrar partida

play-notice-already-running = Já há uma partida contra a engine em andamento.
play-notice-no-engine = A engine não está disponível.
play-notice-not-uci = Só engines UCI podem jogar.
play-notice-chess960-unsupported = A engine não suporta Chess960 (sem a opção UCI_Chess960).
play-notice-busy = Termine antes a anotação, o treino ou os diálogos abertos.
play-notice-locked = A partida não pode ser editada enquanto a engine joga.
play-notice-unsaved-game = Salve ou descarte antes a partida do editor.
play-notice-book-unreadable = Não foi possível ler o livro; jogando sem ele.
play-notice-bad-opening = A abertura específica tem um lance ilegal.
play-notice-illegal-move = Lance ilegal.
play-notice-not-your-turn = Não é a sua vez.
play-notice-move-taken-back = O lance foi desfeito.
play-notice-nothing-to-take-back = Não há lance para desfazer agora.
play-notice-draw-not-allowed = Não é possível oferecer empate agora.
play-notice-draw-declined = A engine recusou o empate.
play-notice-engine-lost = A engine parou de responder; a partida está em pausa.
play-notice-unsaved = A partida está no editor com o resultado; salve para guardá-la.
play-notice-end-checkmate = Xeque-mate.
play-notice-end-stalemate = Afogamento: empate.
play-notice-end-material = Empate: nenhum dos lados pode dar mate.
play-notice-end-fifty = Empate pela regra dos 50 lances.
play-notice-end-repetition = Empate por tripla repetição.
play-notice-end-timeout = Derrota por tempo.
play-notice-end-timeout-draw = Empate: o tempo acabou, mas o adversário não pode dar mate.
play-notice-end-resignation = Desistência.
play-notice-end-draw-agreed = Empate por acordo.
play-notice-end-engine-failure = A engine jogou um lance ilegal ou nenhum; a partida parou sem resultado.

play-confirm-resign-title = Desistir?
play-confirm-resign = Você abandona a partida e ela é salva como derrota.
play-confirm-restart-title = Reiniciar?
play-confirm-restart = A partida em andamento é abandonada e uma nova começa com as mesmas configurações.
play-confirm-quit-title = Sair da partida?
play-confirm-quit = A partida é deixada sem resultado e não é salva.
play-status-starting = Iniciando a engine...
play-start-dropped = A partida não foi iniciada. Clique em Jogar de novo quando quiser.
play-restart-dropped = A partida não foi reiniciada. Clique em Reiniciar de novo quando quiser.

# Ponder e coach (spec M8-1d).
play-section-help = Ajuda e raciocínio
play-ponder = Deixar a engine pensar no meu tempo (ponder)
play-ponder-unsupported = A engine precisa declarar a opção Ponder; senão isto é ignorado.
play-coach = O coach acompanha meus lances (uma segunda engine)
play-coach-engine = Engine do coach
play-coach-no-engine = Cadastre uma segunda engine UCI na janela Engines (diferente da adversária) para usar o coach.
play-coach-depth = Profundidade do coach
play-coach-seconds = Tempo do coach por lance
play-coach-hint = Mostrar o lance melhor quando o meu foi fraco
play-coach-numbers = Mostrar a perda em peões
play-pondering = Pensando no seu tempo
play-coach-line = Coach: { $remark }
play-coach-busy = avaliando o seu lance...
play-coach-wait = observando
play-coach-lost = indisponível (a engine parou)
play-coach-best = melhor lance
play-coach-good = boa jogada
play-coach-inaccuracy = imprecisão
play-coach-mistake = erro
play-coach-blunder = erro grave
play-coach-loss = { $word } (-{ $loss })
play-coach-better = { $text }, o melhor era { $san }
play-notice-ponder-unsupported = A engine não tem a opção Ponder; jogando sem ponder.
play-notice-coach-unavailable = O coach precisa de uma engine UCI diferente da adversária; jogando sem ele.
play-notice-coach-lost = A engine do coach parou; a partida segue sem ele.
play-notice-coach-no-take-back = Desfazer lance fica desligado enquanto o coach acompanha.

# O jogo tático (spec M8-7).
menu-play-tactical = Jogo tático...
action-tactical-game = Jogo tático
play-tactical-title = Jogo tático
play-tactical-section-strength = Força (Elo)
play-tactical-section-start = Posição inicial
play-tactical-elo = Elo da engine
play-tactical-adjust = Mudar o Elo depois de cada partida
play-tactical-adjust-step = Passo
play-tactical-plan-unknown = A força é definida quando a partida começa, pelas opções que a engine declara.
play-tactical-plan-elo = A engine se limita com UCI_Elo { $elo }.
play-tactical-plan-elo-clamped = A engine só aceita UCI_Elo de { $min } a { $max }: será usado { $elo }.
play-tactical-plan-skill = Sem UCI_Elo: será usado Skill Level { $level } (aproximado).
play-tactical-plan-depth = Sem opção de força: toda busca é cortada na profundidade { $depth } (aproximado).
play-tactical-stats-none = Nenhuma partida ainda.
play-tactical-stats =
    { $count ->
        [0] Nenhuma partida
        [one] { $count } partida
       *[other] { $count } partidas
    }. Vitórias: { $wins }, empates: { $draws }, derrotas: { $losses } ({ $percent }%). Maior sequência: { $streak }. Maior Elo vencido: { $elo }.
play-tactical-reset = Zerar o placar
play-tactical-reset-confirm = Confirmar?
play-tactical-start-new = Partida nova
play-tactical-start-current = Posição atual
play-tactical-start-opening = Abertura específica
play-tactical-start-problem = Problema do treino de táticas
play-tactical-start-problem-none = Abra um problema das táticas antes para usar a posição dele.
play-tactical-start-fen = FEN
play-tactical-start-fen-hint = Cole uma FEN
play-tactical-fen-bad = Esta não é uma FEN válida.
play-tactical-fen-ended = Não há o que jogar nesta posição.
play-tactical-start-file = Posição sorteada de um arquivo
play-tactical-start-base = Posição sorteada da base atual
play-tactical-file-choose = Escolher...
play-tactical-file-none = Nenhum arquivo escolhido.
play-tactical-line =
    Tático: { $plan } · placar { $wins }-{ $draws }-{ $losses }
play-tactical-line-elo = engine com Elo { $elo }
play-tactical-line-skill = Skill Level { $level } (cerca de Elo { $elo })
play-tactical-line-depth = profundidade até { $depth } (cerca de Elo { $elo })
play-tactical-next-elo = Elo da próxima partida: de { $from } para { $to }.
play-notice-tactical-elo-clamped = O Elo está fora do que a engine aceita; será usado o valor mais próximo.
play-notice-tactical-depth-limited = A engine não tem opção de força: a busca é cortada numa profundidade, então a força é só aproximada.
play-notice-tactical-pick-failed = Não foi possível sortear uma posição dessa fonte.
play-notice-tactical-pick-empty = Essa fonte não tem partidas.
play-notice-tactical-bad-position = Não há o que jogar nessa posição.
play-status-picking = Sorteando uma posição...

# Desistir e Revanche para todos os modos de jogo (spec M8-12)
action-resign-game = Desistir da partida
action-rematch = Revanche
menu-game-resign = Desistir
menu-game-rematch = Revanche
game-cmd-resign = Desistir
game-cmd-resign-tip = Abandona a partida em andamento
game-cmd-rematch = Revanche
game-cmd-rematch-tip = Joga de novo com as cores trocadas
play-rematch-dropped = A revanche não começou. Clique em Revanche de novo quando quiser.

# Premove contra a engine (D4452)
play-premove = Premove (lance antecipado)
play-premove-tip = Enquanto a engine pensa, arraste uma peça sua: o lance é jogado quando a engine responder, se ainda for legal. Esc ou clique direito cancela.
play-premove-set = Pré-lance marcado (Esc cancela)

# O painel de análise (spec M4-4): uma seção por engine em execução. Os
# equivalentes dos textos do Scid estão nos comentários (`portbr.tcl`).

pane-analysis = Análise
menu-view-pane-analysis = Análise
action-toggle-analysis-pane = Mostrar ou ocultar a janela de análise

analysis-empty = Nenhuma engine em execução. Inicie uma em Ferramentas > Engines...

# ---- estado de uma seção ----
analysis-state-starting = iniciando...
analysis-state-restarting = reiniciando...
analysis-state-stopping = parando...
analysis-state-stopped = parada
analysis-state-checkmate = (xeque-mate)
analysis-state-stalemate = (afogamento)
analysis-state-failed = falhou: { $reason }

# ---- botões (rótulo e dica; as dicas são as do Scid) ----
analysis-tip-start = Iniciar engine
analysis-tip-stop = Parar engine
analysis-btn-add-move = + Lance
analysis-tip-add-move = Adicionar lance (botão direito: adicionar pontuação)
analysis-btn-add-line = + Linha
analysis-tip-add-line = Adicionar variante (botão direito: segunda linha)
analysis-btn-add-all = + Todas
analysis-tip-add-all = Adicionar todas as variantes (botão direito: só a segunda linha; Ctrl+clique: só os primeiros lances)
analysis-tip-add-this-line = Adicionar esta linha
analysis-lines = Linhas
analysis-tip-lines = Número de linhas
analysis-btn-lock = Travar
analysis-btn-locked = Travada
analysis-tip-lock = Travar a engine na posição atual
analysis-tip-unlock = Travada em { $description }. Clique para destravar.
analysis-btn-exclude = Excluir
analysis-tip-exclude = Excluir lance
analysis-tip-excluded = Lances excluídos: { $moves }
analysis-btn-priority = CPU baixa
analysis-tip-priority = Prioridade baixa de CPU (reinicia a engine)
analysis-btn-info = Info
analysis-tip-info = Mostrar informações da engine
analysis-btn-display-hidden = Oculto
analysis-btn-display-nowrap = Sem quebra
analysis-btn-display-wrap = Com quebra
analysis-tip-display = Como as linhas aparecem: ocultas, sem quebra, com quebra (clique do meio)
analysis-btn-stats = Atualizar
analysis-tip-stats = Pedir as estatísticas à engine
analysis-btn-log = Log
analysis-tip-log = Log da engine
analysis-tip-close = Fechar esta engine
analysis-unsupported = Não suportado por esta engine.
analysis-unavailable-locked = Indisponível enquanto a posição está travada.
analysis-unavailable-no-lines = Ainda não há linhas.

# ---- linhas ----
analysis-click-to-see = Clique aqui para ver os lances
analysis-history-line = { $depth } [{ $score }]  { $moves } ({ $time } s)
analysis-locked-hint = Travada em { $description }
analysis-lock-start = posição inicial
analysis-lock-file-game = { $file }, partida { $game }, { $move }
analysis-lock-game = partida { $game }, { $move }
analysis-lock-vardepth =  (profundidade de variante { $depth })

# ---- linhas de informação ----
analysis-info-main = { $score }  Prof.: { $depth }  Nós: { $nodes }  Tempo: { $time } s
analysis-info-extra = NPS: { $nps }  Hash: { $hash }  Carga: { $load }  TB: { $tb }  Atual: { $current }

# ---- caixa de exclusão ----
analysis-exclude-title = Excluir lances
analysis-exclude-help = Lances em SAN, separados por espaço ou vírgula. Deixe vazio para limpar.
analysis-exclude-placeholder = ex.: Nf3, d4

# ---- log da engine ----
analysis-log-auto = Auto
analysis-log-update = Atualizar
analysis-log-find = Buscar
analysis-log-save = Salvar como...
analysis-log-close = Fechar
analysis-log-empty = O log está vazio.
analysis-log-save-title = Salvar o log da engine
analysis-log-filter = Arquivos de log

# ---- avisos da barra de status ----
analysis-notice-at-game-end = Esta posição é o fim da partida: use Adicionar lance ou Adicionar pontuação.
analysis-notice-line-truncated = A linha tem um lance que não pode ser jogado; { $count ->
    [one] { $count } lance foi adicionado.
   *[other] { $count } lances foram adicionados.
}
analysis-notice-exclude-invalid = "{ $token }" não é um lance legal aqui.
analysis-notice-exclude-all = Pelo menos um lance deve sobrar para a busca.
analysis-notice-exclude-unsupported = Esta engine não pode excluir lances.
analysis-notice-multipv-unsupported = Esta engine não tem a opção MultiPV.
analysis-notice-locked = Indisponível enquanto a análise está travada.
analysis-notice-log-saved = O log da engine foi salvo.

# ---- M4-5: setas, barra de avaliação e semáforo ----
analysis-btn-arrows = Setas
analysis-tip-arrows = Mostrar as setas desta engine no tabuleiro (clique de novo para ocultar)
analysis-arrow-mode-arrows = Setas
analysis-arrow-mode-sequence = Sequência
analysis-arrow-seq-both = Sequência: os dois lados
analysis-arrow-seq-white = Sequência: lances das brancas
analysis-arrow-seq-black = Sequência: lances das pretas
analysis-arrow-seq-len = Lances na sequência
action-toggle-eval-bar = Mostrar ou ocultar a barra de avaliação
action-toggle-traffic-lights = Mostrar ou ocultar o semáforo MultiPV
action-toggle-arrow-numbers = Numerar ou não as setas da engine

# ---- M4-6 e M4-8: anotação e modos da engine ----
analysis-btn-annotate = Anotar
analysis-tip-annotate = Anotar a partida com esta engine
analysis-tip-annotate-stop = Parar a anotação (as edições são mantidas)
analysis-btn-threat = Ameaça
analysis-tip-threat = Análise de ameaça: passa a vez (lance nulo) e analisa a posição (clique de novo para sair)
analysis-tip-threat-check = Não há ameaça em xeque
analysis-btn-finish-game = Terminar partida
analysis-tip-finish-game = A engine joga pelos dois lados até o fim da partida
analysis-btn-training = Treino
analysis-tip-training = A engine responde a cada lance seu
analysis-threat-label = Ameaça do adversário:
analysis-training-thinking = Pensando.....
analysis-training-your-move = Sua vez.....
analysis-training-title = Tempo de reflexão da engine
analysis-training-help = A engine responde a cada lance seu depois deste tempo (0,1 a 300 s; outros valores são ajustados).
analysis-training-seconds = Segundos por lance
analysis-training-start = Iniciar
analysis-tip-commandeered = Indisponível enquanto a engine joga
analysis-mode-conflict = Outro modo, o autoplay ou uma anotação estava em andamento: só um roda por vez.
analysis-mode-trial = Saia do modo tentativa primeiro.
analysis-mode-shootout-busy = A engine está jogando a partida: pare-a antes de editar.
analysis-mode-ended-checkmate = Parou: xeque-mate.
analysis-mode-ended-stalemate = Parou: afogamento.
analysis-mode-ended-insufficient-material = Parou: material insuficiente.
analysis-mode-ended-fifty-moves = Parou: regra dos 50 lances.
analysis-mode-ended-repetition = Parou: tripla repetição.
analysis-mode-ended-no-move = Parou: a engine não deu lance.
analysis-mode-ended-engine-error = Parou: a engine falhou ou deu lances que não podem ser jogados.
analysis-mode-ended-user-stopped = Parou.
analysis-mode-ended-game-changed = Parou: a partida foi alterada.

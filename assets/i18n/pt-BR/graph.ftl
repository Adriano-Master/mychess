# O gráfico de pontuação e de tempo (spec M4-7). Os equivalentes dos textos
# do Scid estão nos comentários (`english.tcl`).

pane-graph = Gráfico da partida
menu-view-pane-graph = Gráfico da partida
menu-tools-graph = Gráfico de pontuação
action-toggle-graph-pane = Mostrar ou ocultar o gráfico de pontuação

# Auto, Score, Time, Score Combo, Time Combo
graph-kind-auto = Automático
graph-kind-score = Pontuação
graph-kind-time = Tempo
graph-kind-score-combo = Pontuação combinada
graph-kind-time-combo = Tempo combinado

# GraphOptionsBar "Highlight Current Move"
graph-highlight = Destacar o lance atual
graph-highlight-tip = Marca no gráfico a barra do lance mostrado no tabuleiro.
# GraphOptionsWhite / GraphOptionsBlack ("Invert White" / "Invert Black"):
# a opção nomeia o lado a jogar na posição avaliada.
graph-invert-white = Inverter brancas
graph-invert-white-tip = Inverte as pontuações escritas depois dos lances das pretas (posições com as brancas a jogar), para engines que dão a nota do ponto de vista de quem joga.
graph-invert-black = Inverter pretas
graph-invert-black-tip = Inverte as pontuações escritas depois dos lances das brancas (posições com as pretas a jogar), para engines que dão a nota do ponto de vista de quem joga.

graph-empty = Sem pontuações nem tempos nesta partida
graph-hover-score = { $move } { $value }
graph-hover-time = { $move } { $value } s
graph-style-line = Linha contínua
graph-style-line-tip = Desenha as pontuações como uma curva com a vantagem preenchida; desmarcada, uma barra por lance como no Scid.

# ---- classes do relatório da partida (M4-10a) ----
report-class-brilliant = Brilhante
report-class-great = Excelente
report-class-best = Melhor
report-class-excellent = Ótimo
report-class-good = Bom
report-class-book = Livro
report-class-inaccuracy = Imprecisão
report-class-mistake = Erro
report-class-miss = Chance perdida
report-class-blunder = Capivarada

# Painel do relatório da partida (especificação M4-10b)
menu-tools-report = Relatório da partida
pane-report = Relatório
report-side-white = Brancas
report-side-black = Pretas
report-accuracy = Precisão
report-preset-fast = Rápido
report-preset-standard = Padrão
report-preset-deep = Profundo
report-generate = Gerar relatório
report-regenerate = Analisar de novo
report-clear = Limpar
report-empty = Analise a partida inteira com a engine para ver como cada lance se compara ao melhor. A partida não é alterada.
report-running = Analisando a partida com a engine... a partida não pode ser editada enquanto isso.
report-partial = Relatório parcial: { $done } de { $total } lances analisados.
report-moments = Lances para rever
report-moments-filtered = Lances da classe escolhida
report-no-moments = Nenhum lance para mostrar.
report-moment-loss = -{ $loss } pts
report-estimate-note = A precisão e as classes são estimativas de modelos públicos de probabilidade de vitória; não coincidem com as de nenhum site.
report-progress = Analisando... lance { $done } de { $total }
report-progress-starting = Analisando... iniciando a engine
annotate-notice-report-stopped = Relatório interrompido após { $plies } de { $total } lances; o relatório parcial foi mantido.
report-write = Gravar na partida
report-write-tip = Grava as avaliações, as marcas dos lances e a melhor linha de cada falha na partida (um passo de desfazer).
report-from-game = Usar as avaliações da partida
report-from-game-tip = Monta o relatório com os comentários [%eval] que a partida já tem, sem engine. Assim o melhor lance não é conhecido.
annotate-notice-report-written = Relatório gravado na partida: { $moves } lances julgados, { $variations ->
    [one] { $variations } variante
   *[other] { $variations } variantes
} adicionada(s).
annotate-notice-report-no-evals = A partida não tem avaliações ([%eval]) para montar um relatório.
annotate-notice-report-continued-other-engine = A engine que começou o relatório não existe mais: a análise continuou com outra, então as avaliações podem diferir.
annotate-notice-review-move-ignored = Procure o melhor lance no tabuleiro: adicionar lances e variantes fica pausado durante a revisão.
annotate-notice-report-write-failed = Não foi possível gravar o relatório na partida.
annotate-notice-report-kept = Execução cancelada após { $plies } lances; o relatório anterior, de { $kept } lances, foi mantido.
report-review-start = Rever os lances de:
report-review-both = Os dois
report-review-title = Momento { $index } de { $total }
report-review-looking-best = Foi jogado { $played }, { $class }: perde cerca de { $loss } pontos. O melhor lance era { $best }.
report-review-looking = Foi jogado { $played }, { $class }: perde cerca de { $loss } pontos.
report-review-trying = Ache no tabuleiro um lance melhor que { $played }.
report-review-hint = Esse não. Dica: mova a peça que está em { $square }.
report-review-solved = Correto! { $best } é o melhor lance.
report-review-revealed = O melhor lance era { $best }.
report-review-try = Tentar de novo
report-review-show-best = Ver o melhor lance
report-review-previous = Anterior
report-review-next = Próximo
report-review-exit = Sair
report-review-arrow = Mostrar a seta do melhor lance

# Relatório: configuração, continuar, lado do usuário, tempo restante, marcas no tabuleiro (fecho do M4-10)
report-configure = Configurar...
report-continue = Continuar análise
report-continue-tip = Analisa só os lances que a execução interrompida não alcançou; os já analisados são mantidos.
report-progress-eta = Analisando... lance { $done } de { $total }, faltam cerca de { $time }
report-user-side = Joguei de:
report-user-none = Não definido
report-user-ask = Escolha o seu lado para ver primeiro a sua precisão e os seus erros.
report-side-you = { $side } (você)
report-review-keys = As teclas Esquerda e Direita vão ao momento anterior e ao próximo.
report-config-title = Configurar relatório
report-config-limit-label = Limite de cada busca
report-config-limits = Classes de lance: perda de probabilidade de vitória, em pontos
report-config-limit = { $class }: até
report-config-last = Acima do último limite o lance é { $class }.
report-config-marks = Mostrar a classe do lance na sua casa do tabuleiro
report-config-defaults = Restaurar padrões
report-config-note = Brilhante, Excelente, Livro e Chance perdida têm regras próprias e não mudam aqui.

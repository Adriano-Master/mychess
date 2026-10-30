# Anotação automática (spec M4-6): o diálogo "Configurar anotação", o
# progresso e os avisos.

annotate-title = Configurar anotação
annotate-start = Iniciar
annotate-stop = Parar
annotate-engine = Engine
annotate-engine-default = Automática
annotate-control = Controle do lance
annotate-control-depth = Profundidade
annotate-control-time = Tempo
annotate-xboard-note = Esta engine só pode ser limitada por tempo.
annotate-depth = Profundidade por lance
annotate-seconds = Segundos por lance
annotate-threshold = Limiar
annotate-cutoff = Corte
annotate-add-score = Adicionar pontuação
annotate-score-all = Todos os lances
annotate-score-blunders = Erros/não melhor
annotate-score-variations = Variantes
annotate-score-no = Não
annotate-add-vars = Adicionar variantes
annotate-vars-blunders = Quando o lance é um erro
annotate-vars-notbest = Quando o lance não é o melhor
annotate-vars-all = Todos os lances
annotate-vars-no = Não
annotate-missed-mates = Mates perdidos/mais curtos
annotate-end = No fim da partida
annotate-side = Qual lado
annotate-side-all = Ambos
annotate-side-white = Brancas
annotate-side-black = Pretas
annotate-score-format = Formato da pontuação
annotate-format-plain = +1.5
annotate-format-bracket = [% +1.5]
annotate-format-eval = [%eval +1.5]
annotate-use-book = Usar livro
annotate-book-needs-book = Escolha um livro de abertura em Ferramentas > Livros de abertura
annotate-opening-only = Só erros de abertura
annotate-opening-moves = Lances da abertura
annotate-batch = Anotar todas as partidas do filtro (grava no banco)
annotate-add-tag = Acrescentar tag Annotator
annotate-variations = Processar variantes
annotate-exercises = Marcar exercícios táticos
annotate-informant-show = Configurar valores informantes...
annotate-informant-hide = Ocultar valores informantes
annotate-informant-defaults = Padrões
annotate-informant-interesting = Lance interessante (!?)
annotate-informant-poor = Lance fraco (?)
annotate-informant-blunder = Erro grave (??)
annotate-informant-dubious = Lance duvidoso (?!)
annotate-informant-slight = Vantagem pequena (+=)
annotate-informant-moderate = Vantagem moderada (+/-)
annotate-informant-decisive = Vantagem decisiva (+-)
annotate-informant-won = Partida ganha (++-)
annotate-estimate-moves =
    { $moves ->
        [one] { $moves } lance a anotar
       *[other] { $moves } lances a anotar
    }
annotate-estimate-time = Tempo estimado: { $minutes } min { $seconds } s
annotate-progress = Anotando... lance { $done } de { $total }
annotate-progress-starting = Anotando... iniciando a engine
annotate-progress-batch = Anotando partida { $game } de { $games }... lance { $done } de { $total }
annotate-progress-batch-starting = Anotando partida { $game } de { $games }... iniciando

annotate-notice-already-running = Já há uma anotação em andamento.
annotate-notice-no-engine = Nenhuma engine está em execução e nenhuma está definida para F2.
annotate-notice-trial-mode = Saia do modo tentativa antes de anotar.
annotate-notice-nothing-to-annotate = Não há lance a anotar a partir daqui.
annotate-notice-busy = A partida não pode ser editada durante a anotação. Pressione Esc para parar.
annotate-notice-finished =
    Anotação concluída: { $plies ->
        [one] { $plies } lance
       *[other] { $plies } lances
    }, { $variations ->
        [one] { $variations } variante
       *[other] { $variations } variantes
    }.
annotate-notice-cancelled = Anotação interrompida após { $plies ->
    [one] { $plies } lance
   *[other] { $plies } lances
}; as edições foram mantidas.
annotate-notice-game-changed = A partida mudou durante a anotação; ela parou após { $plies ->
    [one] { $plies } lance
   *[other] { $plies } lances
}.
annotate-notice-engine-failed = A engine falhou; a anotação parou após { $plies ->
    [one] { $plies } lance
   *[other] { $plies } lances
}.
annotate-notice-no-score = A engine não deu pontuação; a anotação parou após { $plies ->
    [one] { $plies } lance
   *[other] { $plies } lances
}.
annotate-notice-batch-unavailable = A anotação em lote precisa de um banco Scid editável (.si4) no slot atual.
annotate-notice-batch-dirty = Salve ou descarte a partida atual antes de anotar em lote.
annotate-notice-batch-finished = Lote concluído: { $games ->
    [one] { $games } partida anotada e gravada
   *[other] { $games } partidas anotadas e gravadas
}, { $skipped } sem lances.
annotate-notice-batch-stopped = Lote interrompido: { $games } de { $total } partidas anotadas e gravadas.

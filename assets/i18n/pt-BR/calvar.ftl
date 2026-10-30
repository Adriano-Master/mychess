# Cálculo de variantes, o exercício de Stoyko (spec M8-6). Os textos do Scid
# estão em `english.tcl` (Calvar, ConfigureCalvar, ToolsTrainCalvar,
# DoneWithPosition).

action-calvar-trainer = Cálculo de variantes
menu-play-calvar = Cálculo de variantes...

calvar-title = Cálculo de variantes
calvar-title-white = Cálculo de variantes: jogam as brancas
calvar-title-black = Cálculo de variantes: jogam as pretas

calvar-status-entering = Calcule de cabeça e digite as linhas que encontrar. Termine cada uma com a sua avaliação.
calvar-status-ready = Você já pode apertar "Terminei" ou dar mais linhas.
calvar-status-evaluating = A engine está avaliando as suas linhas...
calvar-status-waiting-root = Esperando a engine terminar a análise própria...
calvar-status-failed-engine = A engine não conseguiu avaliar o exercício.
calvar-status-failed-timed-out = A engine demorou demais e foi dispensada.
calvar-status-failed-time-up = O tempo acabou e nenhuma linha foi dada.

calvar-engine-thinking = Engine { $name }: pensando...
calvar-engine-ready = Engine { $name }: pronta
calvar-lines-given = Linhas: { $count } de { $required }
calvar-time-limit = limite de { $count } s
calvar-line-in-progress = Linha em andamento: { $line }
calvar-no-line-in-progress = Digite os lances da próxima linha
calvar-entry-placeholder = Lances, por exemplo: Nf3 d5 d4
calvar-entry-tip = Digite um ou mais lances em SAN ou UCI e aperte Enter, com as letras das peças em inglês (N, B, R, Q, K). O tabuleiro não se mexe.
calvar-rejected = Lance ilegal aqui: { $text }
calvar-end-line = Termine a linha com a sua avaliação:
calvar-eval-tip = Terminar a linha: { $symbol } ({ $meaning })
calvar-eval-black-winning = as pretas estão ganhando
calvar-eval-black-better = as pretas estão claramente melhor
calvar-eval-black-slightly = as pretas estão um pouco melhor
calvar-eval-equal = igualdade
calvar-eval-white-slightly = as brancas estão um pouco melhor
calvar-eval-white-better = as brancas estão claramente melhor
calvar-eval-white-winning = as brancas estão ganhando

calvar-undo = Voltar lance
calvar-undo-tip = Desfaz o último lance da linha em andamento
calvar-done = Terminei
calvar-done-tip = Pede à engine que avalie as suas linhas
calvar-close = Fechar
calvar-close-tip = Encerra o exercício e devolve a sua partida
calvar-remove-line = Apagar
calvar-remove-line-tip = Tira esta linha do exercício
calvar-add-to-game = Adicionar à partida
calvar-add-to-game-tip = Acrescenta à partida, como variantes, as suas linhas e os lances perdidos
calvar-new = Novo exercício
calvar-new-tip = Fecha este resultado e prepara outro exercício

calvar-result-score = Nota: { $score } de 100
calvar-result-found = Bons lances encontrados: { $found } de { $total }
calvar-result-all-found = Você considerou todos os bons lances.
calvar-result-crucial = Lance crucial perdido: { $move } ({ $eval })
calvar-result-missed = Outros bons lances que você não considerou: { $moves }
calvar-result-parts = Candidatos { $candidates }%, avaliações { $accuracy }%, linhas { $quality }%{ $penalty ->
    [0] {""}
   *[other] , menos { $penalty } pelo lance crucial
}
calvar-result-line = { $line }: você disse { $yours }, a engine diz { $engine } ({ $eval }); { $plies } lances seguem a engine; { $verdict }
calvar-result-line-unknown = { $line }: você disse { $yours }; a engine não conseguiu avaliar esta linha
calvar-verdict-sound = a linha se sustenta
calvar-verdict-unsound = a linha perde terreno
calvar-verdict-weak = e o primeiro lance não está entre os bons lances

calvar-stats = Exercícios: { $done }, nota média { $mean }, melhor { $best }; bons lances encontrados { $found } de { $good }
calvar-recent = Últimas notas: { $scores }
calvar-no-history = Nenhum exercício feito ainda

calvar-position = Posição
calvar-position-current = Posição atual da partida
calvar-position-fen = FEN
calvar-fen-placeholder = Cole um FEN aqui
calvar-engine = Engine
calvar-engine-auto = Automática
calvar-engine-none = Nenhuma engine UCI na lista: cadastre uma em Ferramentas > Engines
calvar-lines-label = Linhas pedidas
calvar-multipv-label = Linhas da engine no início (MultiPV)
calvar-root-time-label = Tempo da engine no início (segundos)
calvar-line-time-label = Tempo de cada busca de uma linha (segundos)
calvar-time-limit-label = Limite de tempo
calvar-off = sem
calvar-minutes = { $count } min
calvar-follow-board = Mostrar no tabuleiro as peças da linha
calvar-follow-board-tip = Desligado: o tabuleiro fica parado e você calcula de cabeça
calvar-reset = Limpar histórico
calvar-reset-confirm = Clique de novo para confirmar
calvar-reset-tip = Apaga os contadores e a lista de exercícios
calvar-cancel = Cancelar
calvar-start = Iniciar

calvar-notice-busy = Termine antes a partida, o torneio, o modo de análise ou o treino que estiver ativo
calvar-notice-no-engine = Nenhuma engine UCI da lista pode ser usada
calvar-notice-bad-fen = O FEN não é válido
calvar-notice-no-moves = A posição não tem lance legal
calvar-notice-illegal-move = Um lance foi recusado: não é legal na linha que você está calculando
calvar-notice-use-keyboard = Esse lance não é legal na linha que você está calculando: digite-o
calvar-notice-line-open = Termine antes a linha em andamento com a sua avaliação
calvar-notice-need-more-lines = Dê antes as linhas que foram pedidas
calvar-notice-empty-line = Não há lance na linha em andamento
calvar-notice-too-many-lines = O exercício já tem o máximo de linhas
calvar-notice-line-too-long = A linha já tem o máximo de lances
calvar-notice-locked = O tabuleiro fica travado enquanto o exercício está ativo
calvar-notice-timed-out = O pedido demorou demais e foi abandonado
calvar-notice-engine-failed = A engine não respondeu
calvar-notice-time-up = O tempo acabou e não havia linha para avaliar
calvar-notice-store-not-saved = Não foi possível gravar o histórico
calvar-notice-store-unreadable = O histórico gravado estava ilegível e foi posto de lado
calvar-notice-store-newer = O histórico gravado é de uma versão mais nova do programa e não será alterado
calvar-notice-cannot-add = Nada a acrescentar: as linhas já estão na partida, ou a posição é a do último lance
calvar-notice-game-unreadable = Não foi possível ler a partida
calvar-notice-no-problem = O treino de táticas não tem problema no tabuleiro
calvar-notice-no-review = Esse exercício não tem linhas guardadas para rever

calvar-origin-game = Posição da partida { $number }
calvar-origin-tactic = Posição do problema de táticas
calvar-origin-review = Revisão do exercício de { $date } (somente leitura)
calvar-reading-game = Lendo a partida...
calvar-history-title = Exercícios anteriores
calvar-history-row = { $date }: { $score } pontos, { $found } de { $good } bons lances, { $lines } linhas
calvar-history-review = Rever
calvar-history-review-tip = Mostra as linhas, as avaliações e a análise guardadas deste exercício, sem iniciar uma engine
calvar-history-old = (sem linhas guardadas)
calvar-from-tactics = Calcular variantes
calvar-from-tactics-tip = Encerra o treino e calcula variantes a partir da posição deste problema

# O treino de táticas: "mate em N" e "encontre o melhor lance" (spec M8-4).
# Os textos do Scid estão em `english.tcl` (MateFound, BestSolutionNotFound,
# MateNotFound, ShorterMateExists, WinWonGame, ShowSolution, ...).

action-tactics-trainer = Treino de táticas
menu-group-training = Treino
menu-play-tactics = Treino de táticas...

tactics-title = Treino de táticas
tactics-title-mate = Mate em { $n }
tactics-title-best = Encontre o melhor lance

tactics-status-closed = Nenhum problema no tabuleiro
tactics-status-opening = Lendo os problemas...
tactics-status-searching = Procurando o próximo problema... ({ $count } partidas examinadas)
tactics-status-judging = Conferindo o seu lance...
tactics-status-solving-mate = Sua vez: encontre o mate em { $n }
tactics-status-solving-best = Sua vez: encontre o melhor lance
tactics-status-exhausted = Não há mais problemas aqui: todos foram resolvidos ou pulados
tactics-feedback-correct = Certo! Continue: mate em { $n }
tactics-feedback-no-mate = Esse não: o mate forçado desapareceu
tactics-feedback-longer-mate = Existe um mate mais curto
tactics-feedback-not-best = Não é o melhor lance
tactics-result-perfect = Resolvido de primeira!
tactics-result-mistakes = Resolvido, depois de alguns erros
tactics-result-help = Resolvido, com dica
tactics-result-gave-up = A solução está no tabuleiro
tactics-result-no-attempts = Acabaram as tentativas: a solução está no tabuleiro
tactics-result-time-up = O tempo acabou: a solução está no tabuleiro

tactics-notice-busy = Termine antes a partida, o torneio ou o modo de análise em andamento
tactics-notice-unsaved-game = Salve ou descarte antes a partida do editor
tactics-notice-no-source = Escolha um arquivo PGN (ou use a base atual)
tactics-notice-source-unreadable = Não foi possível ler o arquivo
tactics-notice-source-upgrade = A base está no formato antigo SI3: converta-a antes
tactics-notice-source-corrupt = Os arquivos da base estão danificados ou são de outra versão
tactics-notice-source-unfinished = A base tem uma transação inacabada: abra-a como base e repare-a antes
tactics-notice-no-engine = Nenhuma engine da lista pode dar a segunda opinião
tactics-notice-opinion-unavailable = A engine não respondeu: o lance foi julgado só pela partida
tactics-notice-no-engine-to-play = Nenhuma engine UCI da lista pode jogar a partida
tactics-notice-parked-unsaved = Feche antes o treino: a partida guardada tem alterações não salvas
tactics-notice-no-games = A fonte não tem partidas
tactics-notice-locked = O tabuleiro fica travado enquanto um problema está aberto
tactics-notice-timed-out = O pedido demorou demais e foi descartado
tactics-notice-inconclusive = Não consegui verificar este lance: tente outro ou veja a solução
tactics-notice-no-hint = Não há dica para esta posição
tactics-notice-store-not-saved = Não foi possível salvar a pontuação
tactics-notice-store-unreadable = A pontuação salva estava ilegível e foi separada
tactics-notice-store-newer = A pontuação salva é de uma versão mais nova do programa e não será alterada
tactics-notice-source-changed = A base foi alterada ou fechada, então o treino terminou

tactics-stats = Resolvidos { $solved } de { $shown } ({ $percent }%), sequência { $streak } (melhor { $best }), média { $time }
tactics-hint-piece = Dica: mova a peça em { $square }
tactics-time-limit = limite { $count } s
tactics-attempts = tentativas { $count }/{ $max }

tactics-hint = Dica
tactics-hint-tip = Mostra qual peça mover e, depois, o próprio lance
tactics-show-solution = Ver solução
tactics-show-solution-tip = Desistir e ver a solução no tabuleiro
tactics-retry = Tentar de novo
tactics-retry-tip = Monta o mesmo problema outra vez
tactics-continue = Continuar
tactics-continue-tip = Continuar jogando contra a engine a partir desta posição
tactics-next = Próximo problema
tactics-next-tip = Vai ao próximo problema (pula este se ele não foi resolvido)
tactics-close = Fechar
tactics-close-tip = Encerra o treino e devolve a sua partida

tactics-mode = Treino
tactics-mode-mate = Mate em N
tactics-mode-best = Melhor lance
tactics-source = Problemas de
tactics-source-file = Arquivo (SI4 ou PGN)
tactics-source-base = Base atual (filtro)
tactics-choose-file = Escolher...
tactics-file-dialog-title = Escolha o arquivo com os problemas
tactics-file-filter = Bases do Scid e arquivos PGN
tactics-second-opinion = Perguntar à engine quando meu lance não é o da partida
tactics-flagged-only = Só partidas com a bandeira Tática (bases)
tactics-beyond-limit =
    { $count ->
        [0] Nenhum problema de mate além do limite
        [one] { $count } problema de mate em mais de { $limit ->
            [one] { $limit } lance
           *[other] { $limit } lances
        } foi ignorado: aumente "Mate mais longo"
       *[other] { $count } problemas de mate em mais de { $limit ->
            [one] { $limit } lance
           *[other] { $limit } lances
        } foram ignorados: aumente "Mate mais longo"
    }
tactics-scan-mates = Procurar mates nas partidas sem marcadores
tactics-no-file = Nenhum arquivo escolhido
tactics-order = Ordem
tactics-order-sequential = Em sequência
tactics-order-random = Aleatória
tactics-mate-min = Mate mais curto
tactics-mate-max = Mate mais longo
tactics-difficulty-min = Dificuldade mínima
tactics-difficulty-max = Dificuldade máxima
tactics-elo-min = Elo mínimo
tactics-elo-max = Elo máximo
tactics-attempts-label = Tentativas por problema
tactics-time-limit-label = Limite de tempo (segundos)
tactics-include-solved = Mostrar os problemas já resolvidos
tactics-hints = Permitir dicas
tactics-accept-longer = Aceitar um mate até dois lances mais longo
tactics-reset = Zerar pontuação
tactics-reset-confirm = Clique de novo para confirmar
tactics-reset-tip = Apaga a pontuação deste modo e os problemas resolvidos desta fonte
tactics-off = sem
tactics-cancel = Cancelar
tactics-start = Iniciar

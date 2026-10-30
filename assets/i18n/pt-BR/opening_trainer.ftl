# O treinador de aberturas (spec M8-5). Os textos do Scid estão em
# `english.tcl` (Openingtrainer, PlayerBestMove, OpponentBestMove,
# RemoveGames, OnlyFlaggedLines, resetStats, MaxPly, Movenotinrepertoire,
# EndOfVar, ...).

action-opening-trainer = Treinador de aberturas
menu-play-opening-trainer = Treinador de aberturas...

opening-title = Treinador de aberturas
opening-title-white = Treinador de aberturas - Brancas
opening-title-black = Treinador de aberturas - Pretas

opening-side = Lado a treinar
opening-side-white = Brancas
opening-side-black = Pretas
opening-side-both = Ambos
opening-source = Repertório
opening-source-typed = Pelo tipo da base
opening-source-base = Base atual
opening-source-file = Arquivo...
opening-choose-file = Escolher...
opening-file-dialog-title = Escolha o repertório (livro, máscara ou partidas)
opening-file-filter = Livros, máscaras, bases do Scid e arquivos PGN
opening-no-file = Nenhum arquivo escolhido
opening-uses = Usa: { $names }
opening-uses-none = Nenhuma base do tipo "Aberturas para ..." está aberta
opening-depth = Profundidade máxima (meios-lances)
opening-strict = Permitir só os melhores lances
opening-opponent-best = O adversário joga os melhores lances
opening-no-repeat = Não repetir a linha
opening-only-flagged = Só linhas marcadas
opening-show-candidates = Mostrar os lances candidatos
opening-only-marked = Em máscara: só os lances marcados "Para treinar" ou verdes
opening-reset = Zerar estatísticas
opening-reset-confirm = Clique de novo para confirmar
opening-reset-tip = Apaga a pontuação deste repertório
opening-start = Iniciar
opening-cancel = Cancelar

opening-status-closed = Nenhuma linha no tabuleiro
opening-status-building = Lendo o repertório...
opening-status-user-turn = Sua vez
opening-status-line-complete = Fim da linha: o repertório não tem mais lances aqui
opening-status-line-depth = Fim da linha: profundidade máxima alcançada
opening-status-exhausted = Todas as linhas que podiam ser sorteadas foram jogadas nesta sessão

opening-notice-busy = Termine antes a partida, o torneio ou o treino em andamento
opening-notice-no-repertoire = Nenhuma base do tipo "Aberturas para ..." está aberta
opening-notice-no-source = Escolha um arquivo de repertório (ou use a base atual)
opening-notice-source-unreadable = Não foi possível ler o arquivo
opening-notice-source-upgrade = A base está no formato antigo SI3: converta-a antes
opening-notice-source-corrupt = Os arquivos da base estão danificados ou são de outra versão
opening-notice-source-unfinished = A base tem uma transação inacabada: abra-a como base e repare-a antes
opening-notice-book-unreadable = Não foi possível ler o livro
opening-notice-mask-unreadable = Não foi possível ler a máscara
opening-notice-empty-repertoire = O repertório não tem lance para o lado a treinar
opening-notice-one-side-empty = O repertório só tem lances de um dos lados: treinando esse
opening-notice-truncated = O repertório foi cortado por um limite (partidas ou posições)
opening-notice-locked = O tabuleiro fica travado enquanto é a sua vez
opening-notice-timed-out = O pedido demorou demais e foi descartado
opening-notice-no-hint = Não há dica para esta posição
opening-notice-dubious = Esse lance está no repertório, mas marcado como duvidoso
opening-notice-out-of-repertoire = Lance fora do repertório
opening-notice-store-not-saved = Não foi possível salvar a pontuação
opening-notice-store-unreadable = A pontuação salva estava ilegível e foi separada
opening-notice-store-newer = A pontuação salva é de uma versão mais nova do programa e não será alterada
opening-notice-source-changed = A base foi alterada ou fechada enquanto o repertório era lido

opening-session = Linhas { $lines } (perfeitas { $perfect }), acertos { $good }, duvidosos { $dubious }, fora { $out }
opening-candidates = Repertório: { $moves }
opening-report = Posições { $positions }, nunca jogadas { $never }, jogadas { $played }, vencidas { $due }, aprendidas { $learned } - acertos { $good }, duvidosos { $dubious }, fora { $out }
opening-hint-piece = Dica: mova a peça em { $square }

opening-hint = Dica
opening-hint-tip = Mostra qual peça mover e, depois, o próprio lance
opening-retry = Repetir linha
opening-retry-tip = Recomeça a mesma linha
opening-next = Próxima linha
opening-next-tip = Sorteia outra linha (Enter no fim de uma linha)
opening-report-button = Relatório
opening-report-tip = Mostra quanto do repertório você sabe
opening-close = Fechar
opening-close-tip = Encerra o treino e traz de volta a sua partida

opening-notice-game-not-restored = Não foi possível devolver a sua partida anterior: o editor tem outra partida com alterações não salvas e nenhum slot está livre para guardá-la

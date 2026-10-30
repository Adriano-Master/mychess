# Relatório de jogador (spec M7-3): avisos do aplicativo, o diálogo e as
# palavras dos documentos exportados. Os rótulos `preport-label-*` são
# preenchidos por `mychess_app::player_report::player_labels`; `{ $n }` recebe
# o texto literal `{n}` (o nome ou os lances), trocado ao gravar o documento.
# As palavras comuns com o relatório de abertura são as `oprep-label-*`.

preport-title = Relatório do jogador
menu-tools-player-report = Relatório do jogador...
preport-dialog-title = Gerar relatório de jogador
preport-player = Jogador:
preport-color = Cor:
preport-color-white = com as peças brancas
preport-color-black = com as peças pretas
preport-start = Posição inicial:
preport-start-beginning = Início da partida
preport-start-position = Posição atual
preport-beginning = Começando com
preport-after = depois de { $moves }
preport-openings = Aberturas
preport-clipbase = Esvaziar a Clipbase e copiar para ela as partidas encontradas
preport-clipbase-confirm = A Clipbase será esvaziada. Continuar?
preport-overwrite-confirm = Substituir o arquivo “{ $name }”?
preport-change = Alterar...
preport-matching = Correspondência:
preport-matching-exact = Exata
preport-matching-prefix = Começa com
preport-matching-contains = Contém
preport-none = Nenhuma partida de { $name }
preport-pov = do ponto de vista de { $name }
preport-no-games = Nenhuma partida no banco atual

preport-err-unavailable = A base está sendo recarregada; tente de novo em instantes
preport-err-name = Digite o nome de um jogador
preport-err-no-report = Não há relatório de jogador para usar
preport-stale = O relatório está desatualizado
preport-err-busy = Outra operação do relatório está em andamento
preport-err-failed = Não foi possível fazer o relatório de jogador
preport-err-position = A posição tem peças demais para a busca
preport-err-line = Os lances do relatório não são legais
preport-partial = O relatório foi interrompido
preport-filter-set = O filtro agora tem as partidas escolhidas
preport-saved = Relatório salvo
preport-err-save = Não foi possível salvar o relatório
preport-err-save-base = Esse arquivo é um banco aberto; escolha outro nome
preport-err-view = Não foi possível abrir o navegador
preport-err-clipbase = A Clipbase não pode ser esvaziada para receber as próprias partidas
preport-err-no-name = A partida do tabuleiro não tem nome de jogador

preport-label-player = Jogador
preport-label-with-white = com as peças brancas
preport-label-with-black = com as peças pretas
preport-label-after = depois de { $n }
preport-label-openings = Aberturas
preport-label-most-frequent-opponents = Oponentes mais frequentes
preport-label-wins = Vitórias
preport-label-draws = Empates
preport-label-losses = Derrotas
preport-label-pov = do ponto de vista de { $n }; as linhas "Todas as partidas" contam o banco inteiro, não só as partidas de { $n }
preport-label-none = Nenhuma partida de { $n }
preport-label-player-rating = Rating do jogador
preport-label-opponents-rating = Rating dos oponentes
preport-label-player-perf = Desempenho do jogador
preport-label-opponents-perf = Desempenho dos oponentes

# A janela e o diálogo (fatia c, M7-3c).
preport-ui-recent = Jogadores recentes:
preport-ui-white-of = Brancas: { $name }
preport-ui-black-of = Pretas: { $name }
preport-ui-close = Fechar
preport-ui-update = Atualizar
preport-ui-save = Salvar
preport-ui-save-title = Salvar o relatório do jogador
preport-ui-save-choose = Salvar como...
preport-ui-save-dialog = Salvar o relatório do jogador
preport-ui-view-html = Visualizar HTML
preport-ui-stop = Parar
preport-ui-progress = Gerando o relatório do jogador
preport-ui-part = Parte do relatório
preport-ui-format = Formato
preport-ui-part-full = Relatório completo
preport-ui-part-compact = Relatório compacto
preport-ui-part-table = Só a tabela teórica
preport-ui-fmt-text = Texto
preport-ui-fmt-html = HTML
preport-ui-fmt-latex = LaTeX
preport-ui-score = { $name } fez +{ $wins } ={ $draws } -{ $losses } ({ $percent }%) em { $games } partidas
preport-ui-report-line = { $games } partidas em { $database }
preport-ui-opponents-hint = Clique num oponente para ver as informações do jogador
preport-ui-opponent-row = { $games } partidas, { $percent }%
preport-ui-opponent-row-year = { $games } partidas, { $percent }% ({ $first })
preport-ui-opponent-row-years = { $games } partidas, { $percent }% ({ $first }-{ $last })
preport-ui-rating-line = { $rating }: { $average } ({ $games } partidas); { $perf }: { $performance } ({ $percent }% contra { $against })
preport-ui-game-elo = ({ $elo })
preport-ui-game-ref = #{ $n }
preport-ui-eco-row = { $games } partidas, { $percent }%
preport-ui-games = Partidas do relatório
preport-ui-games-shown = As primeiras { $shown } de { $total } partidas estão listadas
preport-ui-theory = Tabela teórica
preport-ui-theory-row = { $count } partidas, { $percent }%
preport-ui-themes = Temas posicionais
preport-ui-endgames = Finais
preport-ui-all = Todas as partidas
preport-ui-info = Relatório do jogador
preport-ui-info-tip = Gera um relatório das partidas deste jogador
preport-ui-white = Brancas
preport-ui-black = Pretas

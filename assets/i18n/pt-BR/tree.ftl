# O painel da árvore de aberturas (spec M5-2). Os equivalentes dos textos do
# Scid estão em `english.tcl` 531-589.

pane-tree = Árvore de aberturas
action-toggle-tree-pane = Mostrar ou ocultar a árvore de aberturas

tree-col-move = Lance
tree-col-games = Partidas
tree-col-score = Score
tree-col-draws = Empates
tree-col-elo = Elo
tree-col-perf = Perf
tree-col-year = Ano
tree-col-eco = ECO
tree-total = TOTAL

# Percentuais: parte inteira e décimos, com o separador decimal do idioma.
tree-percent = { $whole },{ $tenth }%
tree-percent-whole = { $value }%

tree-refresh = Atualizar
tree-refresh-tip = Atualizar a tabela quando a posição mudar
tree-adjust = Ajustar filtro
tree-adjust-tip = Mostrar na lista só as partidas que chegam a esta posição
tree-sort = Ordenar
tree-sort-alphabetical = Alfabética
tree-sort-eco = Código ECO
tree-sort-frequency = Frequência
tree-sort-score = Score
tree-mode = Busca
tree-mode-slow = Lenta
tree-mode-fast = Rápida
tree-mode-fast-then-slow = Rápida, depois lenta
tree-short = Exibição curta
tree-close = Fechar a árvore

tree-searching = Buscando...
tree-loading = Buscando nas partidas...
tree-unavailable = Base não disponível
tree-empty = Nenhuma partida chega a esta posição
tree-issue-too-many-moves = Mais de 60 lances foram jogados a partir desta posição
tree-issue-unsupported = Esta posição não é suportada pela base
tree-issue-filter-size = A base mudou durante a busca
tree-issue-internal = A busca falhou
tree-issue-read = Não foi possível ler a base: { $reason }
tree-unreadable = { $count ->
        [one] { $count } partida não pôde ser lida
       *[other] { $count } partidas não puderam ser lidas
    }

# Spec M5-2b e M5-2c: melhores partidas, treino, trava, abrir base como árvore.
tree-training = Treino
tree-training-tip = A árvore responde aos seus lances com um lance sorteado entre as partidas
tree-lock = Travar
tree-lock-tip = Mantém a árvore nesta base quando você troca para outra
tree-target = Base: { $name }
tree-target-clipbase = Base: Clipbase
tree-open-base = Abrir base como árvore
tree-best = Melhores partidas
tree-best-result = Resultado
tree-best-sort = Ordem
tree-best-max = Partidas
tree-best-results-all = Todos
tree-best-results-white = 1-0
tree-best-results-black = 0-1
tree-best-results-decisive = 1-0 e 0-1
tree-best-results-draw = Empates
tree-best-results-unfinished = Sem resultado
tree-best-order-rating = Melhores (Elo)
tree-best-order-database = Ordem da base
tree-best-columns = Colunas
tree-best-load = Carregar
tree-best-browse = Navegar
tree-best-copy = Copiar
tree-best-empty = Nenhuma partida para mostrar
tree-best-col-moves = Lances
file-dialog-open-tree-base = Abrir uma base como árvore
file-filter-databases = Bancos e arquivos PGN

# Spec M5-3b: o cache das árvores (arquivo `.stc`). Scid: TreeFile* em `english.tcl`.
tree-cache = Cache
tree-cache-save = Salvar cache
tree-cache-fill-base = Preencher com a base
tree-cache-fill-game = Preencher com a partida atual
tree-cache-fill-lines = Preencher com as linhas padrão
tree-cache-info = Informações do cache
tree-cache-size = Tamanho do cache
tree-cache-autosave = Salvar o cache automaticamente
tree-cache-autosave-tip = Salva o cache quando a árvore fecha e quando o programa termina
tree-cache-busy-save = Salvando o cache...
tree-cache-busy-info = Lendo o cache...
tree-cache-busy-fill = Preenchendo o cache: { $done } de { $total }
tree-cache-busy-fill-unknown = Preenchendo o cache...
tree-cache-cancel = Cancelar
tree-cache-dismiss = Dispensar
tree-cache-saved = Cache salvo ({ $bytes } bytes)
tree-cache-saved-skipped = Cache salvo ({ $bytes } bytes); { $skipped } árvores ficaram de fora
tree-cache-filled = Cache preenchido: { $searches } buscas, { $hits } já estavam no cache
tree-cache-info-text = Cache: { $used } de { $capacity } árvores. Reserva: { $backup } de { $backup_capacity }.
tree-cache-info-lowest = Menor árvore: { $games } partidas.
tree-cache-info-file = Arquivo: { $bytes } bytes.
tree-cache-info-no-file = Sem arquivo.
tree-cache-failed-not-scid = Só bases Scid (.si4) têm arquivo de cache
tree-cache-failed-busy = Outra operação do cache está em andamento
tree-cache-failed-discarding = O arquivo de cache antigo ainda está sendo removido; tente de novo daqui a pouco
tree-cache-failed-no-base = Não há base para trabalhar
tree-cache-failed-stale = A base mudou; o cache foi descartado
tree-cache-failed-cancelled = Cancelado
tree-cache-failed-illegal-line = Uma linha padrão não é legal
tree-cache-failed-file = Não foi possível usar o arquivo do cache: { $reason }
tree-cache-failed-search = Uma busca falhou: { $reason }
tree-cache-failed-internal = A operação do cache falhou

# Máscaras da árvore (spec M5-4b)
tree-mask-open-failed = Não foi possível abrir a máscara { $path }: { $reason }
tree-mask-save-failed = Não foi possível salvar a máscara { $path }: { $reason }
tree-mask-saved = Máscara salva
tree-mask-warnings = { $count ->
        [one] A máscara foi lida com { $count } aviso
       *[other] A máscara foi lida com { $count } avisos
    }
tree-mask-no-mask = Abra um arquivo de máscara primeiro
tree-mask-busy = Outro trabalho da máscara está em andamento; pare-o primeiro
tree-mask-not-standard = As máscaras valem só para o xadrez padrão
tree-mask-filled = Máscara preenchida: { $moves ->
        [one] { $moves } lance
       *[other] { $moves } lances
    } de { $games ->
        [one] { $games } partida
       *[other] { $games } partidas
    } ({ $skipped } puladas)
tree-mask-fill-cancelled = Preenchimento interrompido: { $moves ->
        [one] { $moves } lance
       *[other] { $moves } lances
    } de { $games ->
        [one] { $games } partida
       *[other] { $games } partidas
    } foram incluídos ({ $skipped } puladas)
tree-mask-fill-unavailable = Não há partidas para preencher a máscara
tree-mask-full = A máscara está cheia: parte dos dados ficou de fora
tree-mask-cannot-show = Essa posição não pode ser mostrada
tree-mask-issue-not-a-mask = o arquivo não é uma máscara de árvore
tree-mask-issue-foreign = o arquivo tem mais do que uma máscara; nada foi executado
tree-mask-issue-syntax = o arquivo está malformado
tree-mask-issue-too-large = o arquivo é grande demais
tree-mask-issue-read-only = o arquivo é somente leitura
tree-mask-issue-not-found = o arquivo não foi encontrado
tree-mask-issue-denied = permissão negada
tree-mask-issue-io = não foi possível usar o arquivo
tree-mask-issue-internal = algo falhou dentro do programa
tree-mask-unsaved-title = Salvar a máscara?
tree-mask-unsaved-body = A máscara { $name } foi alterada. Salvar antes?
tree-mask-unsaved-body-unnamed = A máscara nova foi alterada. Salvar antes?
tree-mask-game-too-large = A partida é grande demais para preencher a máscara

# Máscaras da árvore, interface (spec M5-4c). Os nomes dos marcadores seguem
# o Scid (`english.tcl` 600-609).
tree-mask-menu = Máscara
tree-mask-menu-tip = Abrir, editar e preencher uma máscara da árvore
tree-mask-new = Nova
tree-mask-open = Abrir...
tree-mask-open-recent = Abrir recente
tree-mask-remove-recent = Remover da lista
tree-mask-save = Salvar
tree-mask-save-as = Salvar como...
tree-mask-close = Fechar
tree-mask-display = Exibir máscara
tree-mask-search = Buscar na máscara
tree-mask-fill-line = Preencher com a linha
tree-mask-fill-game = Preencher com a partida
tree-mask-fill-filter = Preencher com o filtro
tree-mask-show-markers = Mostrar marcadores
tree-mask-show-tooltips = Mostrar dicas
tree-mask-auto-load = Carregar a última máscara ao abrir a árvore
tree-mask-title = Máscara: { $name }
tree-mask-title-dirty = Máscara: { $name } • alterada
tree-mask-unnamed = (máscara nova)
tree-mask-col-comment = Comentário
tree-mask-only-row-tip = Este lance está só na máscara
tree-mask-illegal-move = (ilegal)
tree-mask-illegal-move-tip = Este lance é ilegal nesta posição

tree-mask-marker-include = Incluir
tree-mask-marker-exclude = Excluir
tree-mask-marker-main-line = Linha principal
tree-mask-marker-bookmark = Marcador
tree-mask-marker-white = Branco
tree-mask-marker-black = Preto
tree-mask-marker-new-line = Linha nova
tree-mask-marker-to-verify = A verificar
tree-mask-marker-to-train = Para treinar
tree-mask-marker-dubious = Duvidosa
tree-mask-marker-to-remove = Para remover
tree-mask-marker-other = Outro marcador
tree-mask-marker-none = Sem marcador
tree-mask-color-white = Branca
tree-mask-color-green = Verde
tree-mask-color-yellow = Amarela
tree-mask-color-blue = Azul
tree-mask-color-red = Vermelha
tree-mask-color-other = Outra cor
tree-mask-nag-brilliant = !! Brilhante
tree-mask-nag-good = ! Boa
tree-mask-nag-interesting = !? Interessante
tree-mask-nag-dubious = ?! Duvidosa
tree-mask-nag-poor = ? Fraca
tree-mask-nag-blunder = ?? Erro grave
tree-mask-nag-unclear = ~ Obscura
tree-mask-nag-none = Sem NAG

tree-mask-ctx-add = Adicionar à máscara: { $move }
tree-mask-ctx-remove = Remover da máscara: { $move }
tree-mask-ctx-comment-move = Comentar o lance
tree-mask-ctx-comment-position = Comentar a posição
tree-mask-ctx-add-move = Adicionar este lance à máscara
tree-mask-ctx-no-moves = Nenhum lance legal
tree-mask-ctx-marker-1 = Marcador 1
tree-mask-ctx-marker-2 = Marcador 2
tree-mask-ctx-color = Cor
tree-mask-ctx-nag = NAG
tree-mask-ctx-apply-line = Aplicar à linha

tree-mask-fill-line-title = Preencher a máscara com a linha
tree-mask-fill-line-body = Inclui na máscara os lances da linha, do início da partida até o cursor. Quais lances?
tree-mask-side-all = Todos os lances
tree-mask-side-white = Lances das Brancas
tree-mask-side-black = Lances das Pretas

tree-mask-progress-open = Abrindo a máscara...
tree-mask-progress-save = Salvando a máscara...
tree-mask-progress-fill = Preenchendo a máscara: partida { $done } de { $total }
tree-mask-progress-fill-unknown = Preenchendo a máscara: { $done } partidas
tree-mask-progress-search = Buscando na máscara...
tree-mask-progress-outline = Montando a exibição...
tree-mask-stop = Parar
tree-mask-stopping = Parando...

tree-mask-comment-move-title = Comentário do lance { $move }
tree-mask-comment-position-title = Comentário da posição
tree-mask-comment-hint = { $shortcut } salva; Esc cancela.
tree-mask-comment-placeholder = Digite um comentário...
tree-mask-comment-ok = OK

tree-mask-search-title = Buscar na máscara
tree-mask-search-nag = NAG
tree-mask-search-marker-1 = Marcador 1
tree-mask-search-marker-2 = Marcador 2
tree-mask-search-color = Cor
tree-mask-search-move-text = Texto no comentário do lance
tree-mask-search-position-text = Texto no comentário da posição
tree-mask-search-run = Buscar
tree-mask-search-close = Fechar
tree-mask-search-go = Ir para a posição
tree-mask-search-col-move = Lance
tree-mask-search-col-comment = Comentário
tree-mask-search-col-fen = FEN
tree-mask-search-position-row = (posição)
tree-mask-search-summary = Posições { $positions_found }/{ $positions_total }, lances { $moves_found }/{ $moves_total }
tree-mask-search-truncated = Só as primeiras { $limit } linhas estão listadas.
tree-mask-search-empty = Nada encontrado.
tree-mask-search-hint = Clique duas vezes numa linha para abrir uma partida nova nessa posição.

tree-mask-outline-title = Máscara
tree-mask-outline-expand = Expandir tudo
tree-mask-outline-collapse = Recolher tudo
tree-mask-outline-nags = Mostrar NAGs e marcadores
tree-mask-outline-comments = Mostrar comentários
tree-mask-outline-refresh = Atualizar
tree-mask-outline-truncated = A exibição foi cortada: há linhas demais para mostrar todas.
tree-mask-outline-empty = A máscara não tem lances a partir desta posição.
tree-mask-outline-building = Montando a exibição...
tree-mask-outline-hint = Clique duas vezes numa linha para abrir uma partida nova nessa posição.
tree-mask-outline-toggle = Expandir ou recolher

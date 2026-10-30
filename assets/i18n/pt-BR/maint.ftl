# A manutenção dos bancos (spec M7-1): a janela, as confirmações, os avisos e
# a limpeza geral. O pt-BR é novo: o `portbr.tcl` não traz estas chaves.

menu-tools-maintenance = Manutenção...
menu-tools-name-editor = Editor de nomes...
menu-tools-repair-base = Reparar banco...
menu-db-compact = Compactar...
menu-db-sort = Ordenar...
menu-db-twins = Procurar duplicatas...
menu-db-cleaner = Limpeza geral...
menu-db-check = Verificar partidas...
menu-db-type = Tipo do banco...
menu-db-description = Descrição...
menu-db-flags = Flags personalizadas...
menu-db-tags = Tags extras...
menu-db-spelling-file = Arquivo de ortografia
maint-title = Manutenção do banco
maint-database = Banco:
maint-games = Partidas:
maint-deleted = Apagadas:
maint-in-filter = No filtro:
maint-marked = Marcadas:
maint-years = Anos:
maint-ratings = Ratings:
maint-type = Tipo do banco
maint-description = Descrição
maint-custom-flags = Flags personalizadas
maint-change = Alterar
maint-flags = Flags
maint-delete-current = Apagar a partida atual
maint-undelete-current = Desapagar a partida atual
maint-mark-current = Marcar a partida atual
maint-unmark-current = Desmarcar a partida atual
maint-delete-filter = Apagar as partidas do filtro
maint-undelete-filter = Desapagar as partidas do filtro
maint-mark-filter = Marcar as partidas do filtro
maint-unmark-filter = Desmarcar as partidas do filtro
maint-delete-all = Apagar todas as partidas
maint-undelete-all = Desapagar todas as partidas
maint-mark-all = Marcar todas as partidas
maint-unmark-all = Desmarcar todas as partidas
maint-spell = Correção de nomes
maint-operations = Operações no banco
maint-compact = Compactar banco
maint-sort = Ordenar banco
maint-eco = Classificar ECO
maint-add-elo = Adicionar Elo
maint-duplicates = Apagar partidas duplicadas
maint-autoload = Jogo inicial
maint-tags = Tags extras
maint-tag-strip = Remover tag
maint-check = Verificar partidas
maint-cleaner = Limpeza geral
maint-strip-comments = Remover comentários
maint-strip-variations = Remover variantes
maint-drop-deleted = Tirar do filtro as partidas apagadas
maint-cleaner-help = O Cleaner executa todas as ações marcadas no banco atual. Valem as configurações atuais de classificação ECO e de duplicatas.
maint-cleaner-confirm = Depois de iniciada, a manutenção só pode ser interrompida entre as etapas ou dentro das que aceitam parar. Pode demorar muito. Executar as funções escolhidas?
maint-notice-read-only = Este banco é somente leitura
maint-notice-pgn = Um arquivo PGN não pode ser editado: salve-o antes como banco
maint-notice-clipbase = A Clipbase não aceita esta operação
maint-notice-no-current-game = Nenhuma partida deste banco está no tabuleiro
maint-notice-nothing-to-do = Não há nada a fazer
maint-notice-invalid-value = O valor não é válido
maint-notice-bad-sort = Uma ordenação precisa de um a quinze critérios
maint-notice-repair-open = O banco está aberto: feche-o antes de repará-lo
maint-notice-repair-not-si4 = Escolha o arquivo .si4 do banco a reparar
maint-notice-no-pair = Não há par de partidas duplicadas para mostrar
maint-notice-flags-done =
    { $changed ->
        [one] { $changed } partida alterada
       *[other] { $changed } partidas alteradas
    }
maint-notice-header-done = O cabeçalho do banco foi alterado
maint-notice-twins-found =
    { $marked ->
        [one] { $marked } partida duplicada encontrada; { $protected } mantidas pela proteção
       *[other] { $marked } partidas duplicadas encontradas; { $protected } mantidas pela proteção
    }
maint-notice-twins-none = Nenhuma partida duplicada foi encontrada
maint-notice-twins-marked =
    { $marked ->
        [one] { $marked } partida duplicada marcada para apagar
       *[other] { $marked } partidas duplicadas marcadas para apagar
    }
maint-notice-interrupted = A operação foi interrompida; o que foi feito permanece
maint-notice-check-clean =
    { $examined ->
        [one] Nenhum problema encontrado em { $examined } partida
       *[other] Nenhum problema encontrado em { $examined } partidas
    }
maint-notice-check-problems =
    { $problems ->
        [one] { $problems } partida tem problema
       *[other] { $problems } partidas têm problema
    }
maint-notice-compacted = Compactação feita: { $games_removed } partidas e { $names_removed } nomes removidos
maint-notice-sorted =
    { $moved ->
        [one] O banco foi ordenado; { $moved } partida mudou de lugar
       *[other] O banco foi ordenado; { $moved } partidas mudaram de lugar
    }
maint-notice-filter-undeleted =
    { $removed ->
        [one] { $removed } partida apagada saiu do filtro
       *[other] { $removed } partidas apagadas saíram do filtro
    }
maint-notice-stripped = { $changed } partidas regravadas, { $skipped } deixadas como estavam
maint-notice-tags-done = { $changed } partidas regravadas, { $skipped } deixadas como estavam
maint-notice-tag-filtered =
    { $matched ->
        [one] O filtro é a { $matched } partida que tem a tag
       *[other] O filtro são as { $matched } partidas que têm a tag
    }
maint-notice-names-done = Alterados { $edits } nomes em { $games } partidas
maint-notice-names-refused = O editor de nomes recusou o pedido
maint-notice-repaired = O banco foi reparado ({ $games } partidas). As cópias dos arquivos originais foram mantidas.
maint-notice-repair-failed = O reparo falhou. Os arquivos originais estão intactos; as cópias feitas foram mantidas.
maint-notice-cleaner-done =
    { $failed ->
        [one] A limpeza geral terminou; { $failed } etapa falhou
       *[other] A limpeza geral terminou; { $failed } etapas falharam
    }
maint-notice-tags-shared =
    { $count ->
        [one] { $count } tag compartilhada
       *[other] { $count } tags compartilhadas
    }
maint-notice-nothing-to-share = As duas partidas não têm o que compartilhar
maint-notice-backups-kept = { $count } arquivos foram copiados antes do trabalho e foram mantidos
maint-notice-reports-stale = Os relatórios abertos mostram dados antigos
maint-notice-busy = Outra operação está em andamento na lista de partidas. Tente de novo quando terminar
maint-confirm-mark-twins =
    { $count ->
        [one] { $count } partida duplicada será marcada para apagar. Continuar?
       *[other] { $count } partidas duplicadas serão marcadas para apagar. Continuar?
    }
maint-confirm-undelete-first =
    { $count ->
        [one] { $count } partida apagada perderá a marca antes da busca. Continuar?
       *[other] { $count } partidas apagadas perderão a marca antes da busca. Continuar?
    }
maint-confirm-undelete-all =
    { $count ->
        [one] Desapagar a { $count } partida apagada do banco?
       *[other] Desapagar as { $count } partidas apagadas do banco?
    }
maint-confirm-compact =
    { $count ->
        [one] { $count } partida será apagada para sempre. Tem certeza?
       *[other] { $count } partidas serão apagadas para sempre. Tem certeza?
    }
maint-confirm-sort =
    { $count ->
        [one] Ordenar regrava para sempre o índice de { $count } partida. Continuar?
       *[other] Ordenar regrava para sempre o índice de { $count } partidas. Continuar?
    }
maint-confirm-strip =
    { $count ->
        [one] { $count } partida será alterada e isto não pode ser desfeito. Continuar?
       *[other] { $count } partidas serão alteradas e isto não pode ser desfeito. Continuar?
    }
maint-confirm-tag-add =
    { $count ->
        [one] A tag será acrescentada a { $count } partida. Continuar?
       *[other] A tag será acrescentada a { $count } partidas. Continuar?
    }
maint-confirm-tag-strip =
    { $count ->
        [one] Remover todas as ocorrências da tag de { $count } partida? Isto não pode ser desfeito.
       *[other] Remover todas as ocorrências da tag de { $count } partidas? Isto não pode ser desfeito.
    }
maint-confirm-names =
    { $count ->
        [one] Os nomes de até { $count } partida serão alterados para sempre. Continuar?
       *[other] Os nomes de até { $count } partidas serão alterados para sempre. Continuar?
    }
maint-confirm-repair = O índice e o arquivo de nomes serão refeitos. Cópias dos originais são feitas ao lado deles e mantidas. Continuar?
maint-confirm-cleaner = A limpeza geral executa as etapas escolhidas e algumas não podem ser desfeitas. Continuar?
maint-confirm-share-tags =
    { $count ->
        [one] { $count } tag será copiada entre as duas partidas. Continuar?
       *[other] { $count } tags serão copiadas entre as duas partidas. Continuar?
    }
maint-confirm-weak-twins = Os critérios são fracos. Continuar?
maint-confirm-backup = Copiar antes os três arquivos do banco
maint-weak-no-moves = Os lances não são comparados e cores, evento, local, ano e mês não são todos exigidos: muitas partidas que não são duplicatas podem ser marcadas.
maint-weak-few-fields = Menos de dois entre local, rodada e ano são exigidos: os critérios são fracos.
maint-cleaner-step-players = Ortografia dos jogadores
maint-cleaner-step-events = Ortografia dos eventos
maint-cleaner-step-sites = Ortografia dos locais
maint-cleaner-step-rounds = Ortografia das rodadas
maint-cleaner-step-eco = Classificação ECO
maint-cleaner-step-elo = Adicionar Elo
maint-cleaner-step-twins = Marcar partidas duplicadas
maint-cleaner-step-compact-names = Compactar o arquivo de nomes
maint-cleaner-step-compact-games = Compactar o arquivo de partidas
maint-cleaner-step-tree = Preencher o cache da árvore
maint-cleaner-status-done = Feito
maint-cleaner-status-skipped = Pulada
maint-cleaner-status-interrupted = Interrompida
maint-cleaner-status-failed = Falhou
maint-cleaner-detail-no-spelling = Nenhum arquivo de ortografia carregado
maint-cleaner-detail-no-elo = O arquivo de ortografia não tem ratings
maint-cleaner-detail-no-eco-book = Nenhum livro ECO carregado
maint-cleaner-detail-stale = Os arquivos mudaram fora do programa
maint-cleaner-detail-failed = A etapa falhou
maint-cleaner-detail-stopped-before = Interrompida antes desta etapa
maint-check-cause-index = A entrada do índice não pode ser lida
maint-check-cause-record = O registro da partida não pode ser lido
maint-check-cause-decode = A partida não pode ser decodificada
maint-check-cause-names = Falta um nome da partida
maint-check-cause-illegal-move = Um lance da partida não é legal
maint-repair-backup-kept = Cópia do original mantida em { $path }

# ---- window M7-1c: labels of the forms and the window ----
maint-ui-refresh = Atualizar
maint-ui-close = Fechar
maint-ui-stop = Parar
maint-ui-working = Trabalhando...
maint-ui-counting = Contando o banco...
maint-ui-no-base = Nenhum banco está aberto. O reparo de um banco fechado está disponível abaixo.
maint-ui-info = Banco
maint-ui-edit = Editar
maint-ui-apply = Aplicar
maint-ui-confirm = Confirme
maint-ui-state = Estado
maint-ui-state-rw = leitura e escrita
maint-ui-state-ro = somente leitura (nesta sessão)
maint-clipbase = Clipbase
maint-count-percent = { $count } ({ $percent }%)
maint-range = { $low }-{ $high } ({ $mean })
maint-range-empty = -
maint-flag-entry = { $letter }: { $name }
maint-more = mais { $count }
maint-scope-all = Todas as partidas
maint-scope-filter = Partidas do filtro
maint-open-checker = Verificador de duplicatas
maint-repair = Reparar banco
maint-names-editor = Editor de nomes
maint-spell-players = Jogadores
maint-spell-events = Eventos
maint-spell-sites = Locais
maint-spell-rounds = Rodadas
maint-flag-delete = Apagada
maint-flag-white-opening = Abertura das Brancas
maint-flag-black-opening = Abertura das Pretas
maint-flag-middlegame = Meio-jogo
maint-flag-endgame = Final
maint-flag-novelty = Novidade
maint-flag-pawn-structure = Estrutura de peões
maint-flag-tactics = Tática
maint-flag-kingside = Ala do rei
maint-flag-queenside = Ala da dama
maint-flag-brilliancy = Lance brilhante
maint-flag-blunder = Erro grave
maint-flag-user = Usuário
maint-flag-custom = Personalizada { $number }
maint-flag-custom-described = { $description } ({ $number })
maint-compact-name-file = Arquivo de nomes
maint-compact-game-file = Arquivo de partidas
maint-compact-players = Jogadores
maint-compact-events = Eventos
maint-compact-sites = Locais
maint-compact-rounds = Rodadas
maint-compact-names-col = Nomes
maint-compact-unused = Não usados
maint-compact-games-col = Partidas
maint-compact-size = Tamanho (kb)
maint-compact-current = Estado atual
maint-compact-after = Depois da compactação
maint-compact-names = Compactar o arquivo de nomes
maint-compact-games = Compactar o arquivo de partidas
maint-compact-names-clean = Não há nomes não usados: o arquivo de nomes já está totalmente compactado.
maint-compact-games-clean = O arquivo de partidas já está totalmente compactado.
maint-sort-criteria = Critérios
maint-sort-none = (nenhum)
maint-sort-add = Acrescentar critério
maint-sort-common = Ordenações comuns
maint-sort-common-1 = Ano, mês, local, evento, data, rodada
maint-sort-common-2 = Ano, evento, data, rodada
maint-sort-common-3 = ECO, rating
maint-sort-remove-last = Tirar o último
maint-sort-clear = Limpar
maint-sort-reverse = Inverter a ordem
maint-sort-compact-after = Compactar as partidas depois
maint-sort-note = Ordenar regrava o índice para sempre. O arquivo de partidas fica fora de ordem até ser compactado, o que deixa as buscas mais lentas.
maint-sort-go = Ordenar
maint-sort-key-date = Data
maint-sort-key-year = Ano
maint-sort-key-event = Evento
maint-sort-key-site = Local
maint-sort-key-round = Rodada
maint-sort-key-white = Brancas
maint-sort-key-black = Pretas
maint-sort-key-eco = ECO
maint-sort-key-result = Resultado
maint-sort-key-length = Duração
maint-sort-key-rating = Rating
maint-sort-key-welo = Elo das Brancas
maint-sort-key-belo = Elo das Pretas
maint-sort-key-country = País
maint-sort-key-month = Mês
maint-sort-key-deleted = Apagada
maint-sort-key-eventdate = Data do evento
maint-sort-key-variations = Variantes
maint-sort-key-comments = Comentários
maint-twins-note = Para serem duplicatas, as partidas devem ter os mesmos jogadores e os demais critérios abaixo. É melhor corrigir a grafia dos nomes antes.
maint-twins-criteria = Critérios
maint-twins-names-exact = Nomes exatos dos jogadores
maint-twins-names-four = Só as quatro primeiras letras
maint-twins-colors = Mesmas cores
maint-twins-event = Mesmo evento
maint-twins-site = Mesmo local
maint-twins-round = Mesma rodada
maint-twins-year = Mesmo ano
maint-twins-month = Mesmo mês
maint-twins-day = Mesmo dia
maint-twins-result = Mesmo resultado
maint-twins-eco = Mesmo código ECO
maint-twins-moves = Mesmos lances
maint-twins-which-games = Quais partidas
maint-twins-scope-all = Todas as partidas
maint-twins-scope-filter = Partidas do filtro
maint-twins-when-deleting = Ao apagar
maint-twins-skip-short = Ignorar partidas curtas (menos de 5 lances)
maint-twins-undelete-first = Desapagar todas antes (isso apaga as marcas de apagar que você fez)
maint-twins-set-filter = Ajustar o filtro para as duplicatas a apagar
maint-twins-keep-commented = Manter sempre partidas com comentários
maint-twins-keep-variations = Manter sempre partidas com variantes
maint-twins-which-delete = Qual apagar
maint-twins-delete-shorter = A partida mais curta
maint-twins-delete-lower = O menor número de partida
maint-twins-delete-higher = O maior número de partida
maint-twins-defaults = Padrões
maint-twins-go = Buscar duplicatas
maint-checker-title = Verificador de duplicatas
maint-checker-position = Par { $position } de { $total }
maint-checker-game = Partida { $game }
maint-checker-deleted = Apagada
maint-checker-comparing = Comparando as partidas...
maint-checker-too-large = As partidas são grandes demais para comparar.
maint-checker-share = Compartilhar tags...
maint-checker-undelete-all = Desapagar todas as partidas
maint-checker-delete-twins = Apagar duplicatas...
maint-cleaner-go = Executar
maint-cleaner-report = Relatório
maint-cleaner-step-unknown = Etapa desconhecida
maint-check-all = Verificar todas as partidas
maint-check-filter = Verificar as partidas do filtro
maint-check-summary = { $examined } partidas examinadas, { $problems } com problema
maint-check-item = Partida { $game }: { $cause }
maint-strip-title = Remover comentários ou variantes
maint-strip-note = Remover comentários ou variantes não pode ser desfeito.
maint-strip-go = Remover
maint-tags-list = Listar as tags
maint-tags-filter = Filtrar pela tag
maint-tags-interrupted = A busca foi interrompida: a lista e as contagens não estão corretas.
maint-tags-add = Adicionar uma tag
maint-tags-name = Nome da tag
maint-tags-value = Valor
maint-tags-add-go = Adicionar a tag
maint-tags-note = O nome só tem letras, dígitos e sublinhados. Adicionar tags faz o arquivo de partidas crescer: o espaço só volta ao compactar.
maint-names-type = Tipo de nome a editar
maint-names-kind-player = Jogador
maint-names-kind-event = Evento
maint-names-kind-site = Local
maint-names-kind-round = Rodada
maint-names-kind-rating = Rating
maint-names-kind-date = Data
maint-names-kind-event-date = Data do evento
maint-names-games = Partidas a editar
maint-names-scope-tournament = Só as partidas do torneio atual
maint-names-replace = Substituir
maint-names-with = Por
maint-names-player = Jogador
maint-names-elo = Rating
maint-names-old-date = Substituir (AAAA.MM.DD, * para qualquer)
maint-names-new-date = Por (AAAA.MM.DD)
maint-names-pattern = Casar por padrão (* e ?)
maint-names-go = Substituir
maint-autoload-number = Número da partida (0 para nenhuma)
maint-autoload-none = Nenhuma
maint-autoload-first = Primeira
maint-autoload-current = Atual
maint-autoload-last = Última
maint-repair-note = Escolha o arquivo .si4 de um banco que não esteja aberto. O índice e o arquivo de nomes são refeitos; cópias dos originais são feitas e mantidas.
maint-repair-none = Nenhum arquivo escolhido
maint-repair-choose = Escolher arquivo...
maint-repair-dialog-title = Escolha o banco a reparar
maint-repair-file-filter = Bancos do Scid (.si4)
maint-repair-drop = Também descartar as entradas cujo registro não pode ser lido
maint-repair-go = Reparar
maint-type-0 = Desconhecido
maint-type-1 = Banco temporário
maint-type-2 = Clipbase
maint-type-3 = Arquivo PGN
maint-type-4 = Minhas partidas
maint-type-5 = Banco grande
maint-type-6 = Xadrez por correspondência
maint-type-7 = Xadrez de computador
maint-type-8 = Índice de partidas
maint-type-9 = Coleção de jogador
maint-type-10 = Torneio
maint-type-11 = Internet
maint-type-12 = Partidas de GMs
maint-type-13 = Partidas de MIs
maint-type-14 = Partidas relâmpago
maint-type-15 = Tática
maint-type-16 = Finais
maint-type-17 = Aberturas para as Brancas
maint-type-18 = Aberturas para as Pretas
maint-type-19 = Aberturas para qualquer cor
maint-type-20 = Teoria: 1.c4
maint-type-21 = Teoria: 1.d4
maint-type-22 = Teoria: 1.d4 d5
maint-type-23 = Teoria: Gambito da Dama Recusado
maint-type-24 = Teoria: Eslava
maint-type-25 = Teoria: 1.d4 Cf6
maint-type-26 = Teoria: 1.e4
maint-type-27 = Teoria: Siciliana
maint-type-28 = Teoria: Siciliana 1.e4 c5 2.Cf3 d6 3.d4
maint-type-29 = Teoria: Caro-Kann
maint-type-30 = Teoria: Francesa
maint-type-31 = Teoria: Jogos Abertos
maint-custom-note = Cada flag tem uma descrição de até 8 caracteres; a primeira letra aparece na lista de partidas.

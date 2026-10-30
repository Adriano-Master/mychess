# Jogadores, torneios e arquivo de ortografia (spec M6-3, fatia c).
menu-group-lichess = Lichess
menu-group-fics = FICS
menu-group-players = Jogadores
menu-stats-find-players = Localizador de jogadores...
menu-stats-find-tournaments = Localizador de torneios...
menu-stats-player-info = Informações do jogador...
menu-stats-spell = Corrigir nomes (ortografia)
menu-stats-spell-player = Nomes de jogadores...
menu-stats-spell-event = Nomes de eventos...
menu-stats-spell-site = Nomes de locais...
menu-stats-spell-round = Nomes de rodadas...
menu-stats-add-elo = Adicionar Elo às partidas...
menu-file-load-spelling = Carregar arquivo de ortografia...
menu-file-reload-spelling = Recarregar arquivo de ortografia
menu-file-unload-spelling = Descarregar arquivo de ortografia
menu-file-cancel-spelling-load = Cancelar a carga do arquivo de ortografia
action-player-finder = Mostrar ou ocultar o localizador de jogadores
action-tournament-finder = Mostrar ou ocultar o localizador de torneios
action-player-info = Informações do jogador
file-dialog-open-spelling = Abrir arquivo de ortografia
file-filter-spelling = Arquivos de ortografia (.ssp, .ssp.gz)

# Partes comuns das janelas.
ptools-close = Fechar
ptools-stop = Parar
ptools-defaults = Padrões
ptools-update = Atualizar
ptools-progress = { $done } de { $total }
ptools-from-to = até

# Localizador de jogadores.
pfinder-title = Localizador de jogadores
pfinder-title-file = Localizador de jogadores: { $file }
pfinder-title-count = { $count ->
    [one] { $file }, 1 jogador
   *[other] { $file }, { $count } jogadores
}
pfinder-col-rank = #
pfinder-col-games = Partidas
pfinder-col-oldest = Mais antiga
pfinder-col-newest = Mais recente
pfinder-col-country = País
pfinder-col-elo = Elo
pfinder-col-name = Nome
pfinder-games = Partidas
pfinder-player = Jogador
pfinder-elo = Elo
pfinder-list-size = Tamanho da lista
pfinder-country = País
pfinder-country-tip = no: sem coluna. yes: o país de todos. Ou o começo de um código, como BRA.
pfinder-elo-tip = O maior rating do banco. Alguns vêm do arquivo de ortografia (estimados).
pfinder-building = Lendo o banco de dados
pfinder-no-base = Abra um banco de dados para listar seus jogadores.
pfinder-empty = Nenhum jogador encontrado.
pfinder-shown = { $count ->
    [one] 1 jogador listado
   *[other] { $count } jogadores listados
}
finder-no-base = Não há banco de dados para listar.
finder-stale = O banco mudou enquanto a lista era feita; peça de novo.
finder-failed = Não foi possível fazer a lista.
tmt-filtered = { $games ->
    [one] O filtro contém a 1 partida do torneio.
   *[other] O filtro contém as { $games } partidas do torneio.
}

# Localizador de torneios.
tmt-title = Torneios
tmt-title-file = Torneios: { $file }
tmt-title-player = Torneios: { $player } - { $file }
tmt-col-rank = #
tmt-col-date = Data
tmt-col-players = Jogadores
tmt-col-games = Partidas
tmt-col-elo = Elo
tmt-col-event = Evento
tmt-col-site = Local
tmt-col-winner = Vencedor
tmt-col-second = Segundo
tmt-games = Partidas
tmt-players = Jogadores
tmt-mean-elo = Elo médio
tmt-date = Data
tmt-event = Evento
tmt-site = Local
tmt-player = Jogador
tmt-country = País
tmt-list-size = Tamanho da lista
tmt-none = Nenhum torneio encontrado.
tmt-no-base = Abra um banco de dados para buscar seus torneios.
tmt-running = Buscando os torneios
tmt-crosstable = Tabela do torneio
tmt-show-in-filter = Mostrar no filtro
tmt-load-first = Carregar a primeira partida
tmt-select-tip = Clique num torneio e escolha o que fazer com ele. Um clique duplo abre a tabela.
tmt-shown = { $count ->
    [one] 1 torneio
   *[other] { $count } torneios
}

# Informações do jogador.
pinfo-title = Informações do jogador
pinfo-title-name = Informações do jogador: { $name }
pinfo-empty = Escolha um jogador no localizador ou nas informações da partida.
pinfo-no-base = Abra um banco de dados para ver as estatísticas de um jogador.
pinfo-working = Contando as partidas
pinfo-stale = Atualizando...
pinfo-no-games = 0 partidas
pinfo-not-in-base = Este nome não está no banco de dados.
pinfo-no-spelling = Nenhum arquivo de ortografia foi carregado, então não há título, país nem biografia.
pinfo-approximate = Dados aproximados: o nome só começa como o do jogador.
pinfo-candidates = Possíveis correspondências: { $names }
pinfo-title-label = Título
pinfo-country-label = País
pinfo-born = Nascimento
pinfo-peak = Melhor rating
pinfo-peak-estimate = { $peak } (estimado)
pinfo-bio = Biografia
pinfo-fide = Perfil na FIDE
pinfo-games = { $count ->
    [one] 1 partida
   *[other] { $count } partidas
}
pinfo-dates = { $first }  até  { $last }
pinfo-filter-games = { $count ->
    [one] Filtro: 1 partida
   *[other] Filtro: { $count } partidas
}
pinfo-legend = Vitórias / Derrotas / Empates
pinfo-won = Vitórias
pinfo-lost = Derrotas
pinfo-drawn = Empates
pinfo-all = Todas as partidas
pinfo-filter = Partidas do filtro
pinfo-against = Contra { $name }
pinfo-total = Total
pinfo-white = Brancas
pinfo-black = Pretas
pinfo-openings-white = Aberturas mais comuns com as Brancas
pinfo-openings-black = Aberturas mais comuns com as Pretas
pinfo-rating-history = Histórico de rating
pinfo-lookup = Procurar nome
pinfo-tournaments = Torneios
pinfo-edit-ratings = Editar ratings
pinfo-edit-name = Editar nome
pinfo-later-tip = Disponível com o editor de nomes, numa versão futura.
pinfo-recent = Jogadores recentes
pinfo-no-player = Não há jogador para consultar.
pinfo-bad-link = Esse link não é da janela.
pinfo-no-fide = As notas deste jogador não têm perfil na FIDE.
pinfo-fide-failed = Não foi possível iniciar o navegador.
pinfo-filter-set = { $games ->
    [one] O filtro contém 1 partida.
   *[other] O filtro contém { $games } partidas.
}

# Arquivo de ortografia e corretor de nomes.
spell-title = Correção ortográfica
spell-title-kind = Correção ortográfica: { $kind }
spell-kind-player = nomes de jogadores
spell-kind-event = nomes de eventos
spell-kind-site = nomes de locais
spell-kind-round = nomes de rodadas
spell-col-apply = Aplicar
spell-col-old = Nome no banco
spell-col-new = Nome correto
spell-col-games = Partidas
spell-col-life = Vida
spell-ambiguous = Ambíguas
spell-ambiguous-tip = Mostra também os nomes que combinam com mais de um nome correto. Elas nunca vêm marcadas.
spell-surnames = Só sobrenome
spell-surnames-tip = Olha também os nomes que são só um sobrenome.
spell-select-all = Marcar todas
spell-select-none = Desmarcar todas
spell-find-placeholder = Buscar na lista
spell-copy-text = Copiar como texto
spell-rescan = Varrer de novo
spell-apply = Fazer correções
spell-ambiguous-mark = ambígua
spell-found = { $count ->
    [one] 1 correção encontrada ({ $kind }).
   *[other] { $count } correções encontradas ({ $kind }).
}
spell-ticked = { $ticked } de { $total } marcadas
spell-scanning = Procurando correções
spell-writing = Gravando as correções
spell-empty = Nenhuma correção foi encontrada.
spell-partial = A varredura foi interrompida: a lista está incompleta.
spell-read-only = Este banco de dados não pode ser alterado.
spell-confirm = Confirme as correções. Não é possível desfazer.
spell-confirm-yes = Fazer correções
spell-loaded = Arquivo de ortografia carregado: { $players } jogadores, { $events } eventos, { $sites } locais, { $rounds } rodadas.
spell-read-error = Não foi possível ler o arquivo de ortografia: { $reason }
spell-error-not-a-file = não é um arquivo
spell-error-too-large = é grande demais
spell-error-gzip = os dados compactados estão danificados
spell-error-unclosed-quote = o cabeçalho de seção da linha { $line } tem aspas sem fechar
spell-error-cancelled = a leitura foi cancelada
spell-not-loaded = Nenhum arquivo de ortografia foi carregado. Você pode carregar um no menu Arquivo.
spell-no-elo = O arquivo de ortografia não tem ratings Elo (linhas %Elo). Carregue um que tenha.
spell-no-base = Não há banco de dados para trabalhar.
spell-none-valid = Nenhuma correção válida foi encontrada.
spell-interrupted = Interrompido antes do fim.
spell-done = { $names ->
    [one] 1 nome ({ $kind }) que ocorre { $times } vezes no total foi corrigido.
   *[other] { $names } nomes ({ $kind }) que ocorrem { $times } vezes no total foram corrigidos.
}
spell-done-skipped = { $count ->
    [one] 1 troca de nome de jogador foi ignorada pela idade do jogador ou a data da partida.
   *[other] { $count } trocas de nome de jogador foram ignoradas pela idade do jogador ou a data da partida.
}
spell-not-found = { $count ->
    [one] 1 nome já não estava no banco de dados.
   *[other] { $count } nomes já não estavam no banco de dados.
}
spell-game-not-refreshed = As marcas da partida na tela ainda mostram os nomes antigos; carregue a partida de novo.

# Adicionar Elo.
spell-add-elo = Adicionar Elo às partidas
spell-elo-description = Adiciona o Elo dos jogadores às partidas, usando os dados do arquivo de ortografia (ratings).
spell-elo-overwrite = Sobrescrever ratings existentes
spell-elo-scope = Adicionar ratings a
spell-elo-all = Todas as partidas
spell-elo-filter = Partidas do filtro
spell-elo-plan = Conferir
spell-elo-planning = Calculando o que mudaria
spell-elo-planned = { $games ->
    [one] { $ratings } ratings seriam adicionados a 1 partida.
   *[other] { $ratings } ratings seriam adicionados a { $games } partidas.
}
spell-elo-add = Adicionar ratings
spell-elo-writing = Gravando os ratings
spell-elo-done = { $games ->
    [one] Foram adicionados { $ratings } ratings Elo a 1 partida.
   *[other] Foram adicionados { $ratings } ratings Elo a { $games } partidas.
}
spell-elo-nothing = Não há nada para adicionar.
menu-group-statistics = Estatísticas e gráficos

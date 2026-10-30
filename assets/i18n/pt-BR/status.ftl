status-ready = Pronto
status-turn-white = Brancas jogam ({ $move })
status-turn-black = Pretas jogam ({ $move })
status-check = Xeque
status-checkmate = Xeque-mate
status-stalemate = Afogamento
status-modified = Modificada
status-notice-illegal-move = Lance ilegal
status-notice-null-move-in-check = Lance nulo não é permitido em xeque
status-notice-prompt-pending = Responda primeiro ao diálogo aberto
status-notice-stale-node = Essa posição não existe mais
status-notice-no-move-here = Não há lance aqui
status-notice-not-a-variation = Isso não é uma variante
status-notice-not-in-variation = Fora de qualquer variante
status-notice-undo-disabled = Desfazer está desligado no modo tentativa
status-notice-unreadable-move = Lance ilegível: { $text }
status-notice-game-too-large = A partida é grande demais
status-notice-other = Operação não realizada
status-file-error = { $reason } ({ $path })
file-error-not-found = Arquivo não encontrado
file-error-permission-denied = Permissão negada
file-error-already-exists = O arquivo já existe
file-error-too-large = O arquivo excede o limite de { $limit } MiB
file-error-unsupported-encoding = Codificação de texto não suportada: { $encoding }
file-error-corrupt-gzip = O arquivo compactado está corrompido
file-error-no-games-found = Nenhuma partida foi encontrada no arquivo
file-error-encoding-lossy = A codificação do arquivo não comporta todos os caracteres da partida
file-error-io = Não foi possível ler ou gravar o arquivo
clipboard-error-empty = A área de transferência está vazia
clipboard-error-too-large = O texto da área de transferência excede o limite de { $limit } MiB
clipboard-error-unavailable = Não foi possível ler a área de transferência
clipboard-error-write-failed = Não foi possível gravar na área de transferência
paste-fen-error-empty = Não há posição para colar
paste-fen-error-invalid = A área de transferência não contém um FEN válido: { $excerpt }
app-notice-host-effect-not-handled = O programa não conseguiu executar esse pedido
setup-chess960-unsupported = Posições de Chess960 ainda não podem ser montadas à mão
prefs-issue-invalid-value = Valor inválido para { $key } nas preferências; foi usado o padrão
prefs-issue-clamped = O valor de { $key } nas preferências estava fora do limite e foi ajustado
prefs-issue-newer-version = As preferências vêm de uma versão mais nova ({ $found })
prefs-issue-unreadable = Não foi possível ler o arquivo de preferências; os padrões estão em uso
prefs-issue-save-failed = Não foi possível salvar as preferências
prefs-issue-no-config-dir = Não há pasta de configuração; as preferências não são salvas
shortcut-issue-unknown-action = Ação desconhecida nos atalhos: { $name }
shortcut-issue-invalid-binding = Tecla inválida para { $action }: { $text }
shortcut-issue-conflict = A tecla de { $action } já é usada por { $other }
status-move-entry = Digite o lance: [{ $text }] { $candidates }
status-altered = alterada
status-read-only = somente leitura
app-notice-save-in-progress = Já há um salvamento em andamento
file-error-needs-confirmation = O arquivo precisa de confirmação antes de ser substituído
status-move-entry-no-match = Nenhum lance legal começa com "{ $text }"
status-comment-too-long = O comentário é longo demais.
status-nothing-to-strip = Nada a remover.
status-nag-draft-discarded = As anotações digitadas não foram salvas: uma delas não é uma anotação conhecida.
base-notice-no-free-slot = Há bancos demais abertos; feche um antes
base-notice-open-in-progress = Já há um banco sendo aberto
base-notice-already-in-clipbase = Você já está na Clipbase
base-notice-clipbase-empty = A Clipbase não tem partidas para colar
base-error-si3-needs-upgrade = Este banco está no formato antigo SI3; atualize-o para SI4 primeiro
base-error-corrupt = Os arquivos do banco estão danificados ou têm uma versão não suportada
base-error-pending-journal = O banco tem uma operação inacabada e precisa ser reparado antes
base-error-unsupported-format = O arquivo não é um texto PGN
base-error-too-many-games = O arquivo tem mais partidas do que se pode abrir
base-error-too-large = As partidas ocupam memória demais
base-error-out-of-memory = Memória insuficiente para abrir o banco
base-error-no-games = O arquivo não tem partidas
base-error-cancelled = Abertura cancelada
base-error-game-unreadable = Não foi possível ler a partida
base-error-clipbase-limit = A Clipbase atingiu o limite de partidas
base-error-unencodable = A partida não pode ser guardada em um banco
base-error-unsupported-start = A partida não pode ser guardada em um banco porque começa de uma posição Chess960 (ou outra que o SI4 não comporta). Salve-a em PGN com Arquivo > Salvar como...
base-error-internal = Erro interno
base-notice-not-activated = O banco foi aberto mas não selecionado; responda antes à pergunta sobre a partida
base-notice-game-not-loaded = A partida não foi carregada porque há uma pergunta sobre a partida atual aberta; tente de novo
base-notice-list-busy = Uma busca, ordenação ou cópia ainda está em andamento; espere ou cancele
base-notice-not-found = Nenhuma partida encontrada; a lista voltou ao início
base-notice-filtered = { $removed } partidas saíram do filtro
base-notice-copied = { $count } partidas copiadas para a Clipbase
base-notice-read-only = Este banco não pode ser alterado (arquivo PGN, a Clipbase ou banco somente leitura)
base-notice-nothing-selected = Selecione uma ou mais partidas antes
base-notice-unreadable-entries = { $count } partidas não puderam ser lidas e saíram do filtro
base-error-already-open = O banco está aberto para gravação em outro lugar
base-error-read-only = O banco é somente leitura
base-error-stale = O banco mudou no disco; foi recarregado e nada foi gravado
base-notice-stale = O banco mudou durante a operação; o resultado foi descartado
base-notice-list-reset = O banco foi recarregado com outro número de partidas; filtro, seleção e ordenação foram reiniciados
base-error-name-limit = O banco não comporta mais nomes (jogadores, eventos, locais ou rodadas)
base-notice-copy-same = Não é possível copiar: a origem e o destino são o mesmo banco ({ $from })
base-notice-copy-source-closed = Não é possível copiar de { $from } para { $to }: o banco de origem não está aberto
base-notice-copy-target-closed = Não é possível copiar de { $from } para { $to }: o banco de destino não está aberto
base-notice-copy-target-read-only = Não é possível copiar de { $from } para { $to }: o destino é somente leitura
base-notice-copy-empty-filter = Não é possível copiar de { $from } para { $to }: o filtro da origem não tem partidas
base-notice-copy-nothing-selected = Não é possível copiar de { $from } para { $to }: nenhuma partida está selecionada
base-notice-filter-copied = { $count ->
    [one] Uma partida copiada
   *[other] { $count } partidas copiadas
} de { $from } para { $to }
base-notice-copy-cancelled = Cópia de { $from } para { $to } interrompida: { $count ->
    [one] uma partida foi copiada
   *[other] { $count } partidas foram copiadas
}
base-notice-copy-failed = A cópia de { $from } para { $to } falhou após { $count ->
    [one] uma partida
   *[other] { $count } partidas
}: { $reason }
base-notice-filter-copied-notes = { $count ->
    [one] Uma partida copiada
   *[other] { $count } partidas copiadas
} de { $from } para { $to }{ $truncated ->
    [0] {""}
    [one] ; um nome foi encurtado para caber no formato
   *[other] ; { $truncated } nomes foram encurtados para caber no formato
}{ $reset ->
    [0] {""}
   *[other] ; a lista do destino foi reiniciada porque o arquivo tinha outro número de partidas
}
base-notice-browser-limit = O navegador mostra só as primeiras { $shown } de { $asked } partidas

bookmark-notice-added = Marcador adicionado
bookmark-notice-cannot-add = Só uma partida de um banco aberto pode ser marcada
bookmark-notice-path-not-utf8 = O caminho deste banco não pode ser guardado num marcador
bookmark-notice-tree-full = Os marcadores estão cheios ou as pastas estão aninhadas demais
bookmark-notice-base-unavailable = Não foi possível abrir o banco do marcador "{ $title }"
bookmark-notice-game-not-found = Não há partida para o marcador "{ $title }"
bookmark-notice-dropped = { $count ->
    [one] Um marcador inválido foi descartado
   *[other] { $count } marcadores inválidos foram descartados
}
bookmark-notice-file-unreadable = O arquivo de marcadores não pôde ser lido e foi separado
bookmark-notice-newer-version = O arquivo de marcadores vem de uma versão mais nova; uma cópia de segurança foi mantida
bookmark-notice-save-failed = Não foi possível salvar os marcadores
base-error-exists = Já existe um banco com esse nome; nada foi criado
base-error-import-self = Um arquivo de origem é um dos arquivos do banco e foi ignorado
base-error-export-self = O destino é o arquivo de um banco aberto: escolha outro nome
status-chess960 = Chess960
status-chess960-number = Chess960 nº { $n }

# A janela do livro: um livro Polyglot, com um segundo para comparar (spec
# M5-6, M5-6c). Os
# equivalentes dos textos do Scid estão em `english.tcl` 148, 168, 333,
# 746-749.

pane-book = Livros de abertura
menu-tools-book = Livros de abertura
pane-close-book = Fechar o painel dos livros de abertura

book-title-first = Livro 1
book-title-second = Livro 2
book-compare = Comparar
book-compare-tip = Comparar com outro livro
book-compare-close-tip = Fechar a comparação
book-tools-title = Ferramentas
book-opening-issue-badge = Problema no livro de abertura
book-tools-show-tip = Mostrar as ferramentas (editar pesos, adicionar linhas, criar e mesclar livros)
book-tools-hide-tip = Ocultar as ferramentas
book-tools-hint = A tabela já pode ser editada: digite um peso, dobre, divida ou remova um lance.
book-group-edit = Editar esta posição
book-untitled = (sem arquivo)
book-changed = Alterado
book-empty-slot = Nenhum livro ainda. Abra um arquivo ou comece um novo.
book-entries = { $count ->
    [one] 1 registro
   *[other] { $count } registros
}
book-read-only = Somente leitura
book-is-opening = Livro de abertura

book-open = Abrir livro...
book-new = Novo livro
book-save = Salvar livro
book-save-as = Salvar livro como...
book-close-book = Fechar livro (o arquivo é mantido)
book-use-as-opening = Usar como livro de abertura
book-use-this = Usar este livro
book-use-first = Usar o livro 1
book-use-second = Usar o livro 2

book-col-move = Lance
book-col-weight = Peso
book-col-percent = %
book-no-moves = O livro não tem lance nesta posição.
book-not-standard = Um livro Polyglot só guarda posições do xadrez padrão.
book-game-move = A partida joga: { $san }
book-others = Outros lances
book-others-tip = Clique num lance para adicioná-lo ao livro com peso 1
book-weight-tip = Digite um peso de 1 a 65520 e tecle Enter
book-double = x2
book-half = /2
book-double-tip = Dobrar o peso
book-half-tip = Dividir o peso pela metade
book-remove-move-tip = Remover este lance do livro

book-add-game-move = Adicionar lance
book-add-line = Adicionar linha
book-remove-line = Remover linha
book-import-branch = Importar ramo
book-copy-from-other = Copiar do outro
book-add-game-move-tip = Adiciona o lance que a partida joga em seguida, com peso 1
book-add-line-tip = Adiciona a linha principal da partida a partir desta posição, com peso 1
book-remove-line-tip = Remove a linha principal da partida a partir desta posição
book-import-branch-tip = Adiciona todas as linhas da partida a partir desta posição, com as variantes
book-copy-tip = Troca os lances desta posição pelos do outro livro

book-confirm-close = O livro tem alterações não salvas. Fechar mesmo assim?
book-confirm-yes = Fechar e descartar
book-confirm-no = Manter

book-create-title = Criar a partir do filtro
book-create-games = { $count ->
    [one] 1 partida no filtro
   *[other] { $count } partidas no filtro
}
book-create-max-ply = Meios-lances por partida
book-create-min-games = Mínimo de partidas
book-create-min-score = Pontuação mínima (%)
book-create-only-white = Só Brancas
book-create-only-black = Só Pretas
book-create-uniform = Mesmo peso
book-create-make = Criar livro
book-create-into-first = No livro 1
book-create-into-second = No livro 2
book-create-stop = Parar
book-create-stop-tip = Parar a criação do livro; nada é guardado

book-merge-title = Mesclar livros
book-merge-keep-first = Manter o livro 1
book-merge-sum = Somar os pesos
book-merge-button = Mesclar no livro 1

book-opening-title = Livro de abertura da anotação
book-opening-none = Nenhum escolhido
book-opening-clear = Não usar nenhum
book-opening-loaded = { $name } (carregado)

book-notice-saved = O livro foi salvo.
book-notice-created = Livro criado: { $used } partidas usadas, { $skipped } deixadas de fora, { $entries } registros.
book-notice-merged = Livros mesclados: { $written } registros escritos, { $skipped } deixados de fora.
book-notice-added = { $count ->
    [one] 1 lance adicionado.
   *[other] { $count } lances adicionados.
}
book-notice-removed = { $count ->
    [one] 1 lance removido.
   *[other] { $count } lances removidos.
}
book-notice-copied = Os lances foram copiados.
book-notice-opening-loaded = O livro de abertura foi lido.
book-notice-create-cancelled = Interrompido. Nenhum livro foi criado.
book-dismiss = Dispensar

book-issue-not-found = O arquivo não existe.
book-issue-unreadable = O arquivo não pode ser lido como um livro Polyglot.
book-issue-too-short = O arquivo é curto demais para ser um livro.
book-issue-too-large = O arquivo é grande demais para um livro.
book-issue-read-only = O livro é somente leitura.
book-issue-no-path = O livro ainda não tem arquivo. Use Salvar como.
book-issue-write-failed = Não foi possível gravar o livro.
book-issue-unsupported = Chess960 e posições montadas não cabem num livro Polyglot.
book-issue-illegal-move = O lance não é legal aqui.
book-issue-zero-weight = O peso não pode ser zero. Remova o lance.
book-issue-weight-too-large = O peso não pode passar de 65520.
book-issue-too-many-moves = Uma posição pode ter no máximo 100 lances no livro.
book-issue-not-a-number = O peso deve ser um número inteiro.
book-issue-already-there = O lance já está no livro.
book-issue-nothing-to-add = Não há nada depois desta posição para usar.
book-issue-branch-too-big = O ramo tem linhas demais para importar.
book-issue-unsaved-changes = O livro tem alterações não salvas.
book-issue-no-book = Abra ou comece um livro primeiro.
book-issue-no-games = A base atual não tem partida no filtro.
book-issue-busy = O livro está ocupado com outra operação.
book-issue-cancelled = A operação foi interrompida.
book-issue-internal = A operação falhou.

book-dialog-open-title = Abrir um livro de abertura
book-dialog-save-title = Salvar o livro de abertura
book-dialog-filter = Livros de abertura (*.bin)

action-toggle-book-pane = Mostrar ou ocultar o painel dos livros de abertura

book-quit-title = Livros de abertura com alterações não salvas
book-quit-message = Alguns livros de abertura têm alterações não salvas. Salvá-los antes de sair?
book-quit-save = Salvar
book-quit-discard = Descartar
book-quit-cancel = Cancelar

book-export-title = Exportar ramo para PGN
book-export-hint = Grava as linhas do livro que partem da posição atual, uma partida por linha, com os lances de maior peso primeiro.
book-export-max-plies = Meios-lances por linha
book-export-max-lines = Máximo de linhas
book-export-min-weight = Peso mínimo (0: todos)
book-export-make = Exportar ramo para PGN…
book-export-first = Exportar livro 1…
book-export-second = Exportar livro 2…
book-export-tip = Exportar para um arquivo PGN as linhas deste livro que partem da posição atual
book-export-stop = Parar
book-export-stop-tip = Parar a exportação; o arquivo não é gravado
book-export-dialog-title = Exportar o ramo para PGN
book-export-dialog-filter = Arquivos PGN (*.pgn)
book-notice-exported = { $count ->
    [0] Nenhuma linha exportada.
    [one] 1 linha exportada.
   *[other] { $count } linhas exportadas.
}
book-notice-exported-truncated = { $count ->
    [0] Nenhuma linha exportada.
    [one] 1 linha exportada; o limite de linhas cortou o ramo.
   *[other] { $count } linhas exportadas; o limite de linhas cortou o ramo.
}
book-notice-export-cancelled = Interrompido. Nenhum arquivo foi gravado.
book-issue-empty-branch = O livro não tem lance nesta posição.
book-issue-protected-file = Esse arquivo pertence a uma base ou a um livro aberto. Escolha outro nome.
book-issue-file-exists = Já existe um arquivo com esse nome. Escolha outro nome.
book-notice-export-stopping = Interrompendo a exportação…
book-notice-export-finished = { $count ->
    [0] A exportação já tinha terminado; nenhuma linha foi gravada.
    [one] Tarde demais para parar: o arquivo foi gravado com 1 linha.
   *[other] Tarde demais para parar: o arquivo foi gravado com { $count } linhas.
}
book-issue-bad-name = O nome do arquivo não é um texto válido. Escolha outro nome.

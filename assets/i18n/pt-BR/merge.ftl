# Mesclar uma partida na atual (especificação M7-6, fatia c).

menu-edit-merge-game = Mesclar na partida atual...
action-merge-game = Mesclar a partida selecionada na partida atual
merge-button = Mesclar
merge-action = Mesclar na partida atual...
merge-games = Mesclar partidas

merge-title = Prévia da mesclagem
merge-title-batch = Mesclar partidas
merge-source-label = Partida a mesclar
merge-up-to = Até o lance (da partida mesclada)
merge-all = Todos
merge-note-extends = A posição coincide com o fim da partida: os lances serão acrescentados à linha principal.
merge-legend = Os lances que a mesclagem acrescenta estão realçados.
merge-preview-label = Partida depois da mesclagem
merge-ok = Mesclar
merge-cancel = Cancelar

# A descrição posta em comentário antes do primeiro lance mesclado.
merge-comment-line = { $white } -- { $black } { $result }
merge-comment-moves = { $count ->
    [one] ({ $count } lance)
   *[other] ({ $count } lances)
}
merge-comment-event = { $event }, { $year }

merge-batch-confirm = Mesclar { $count ->
    [one] uma partida
   *[other] { $count } partidas
} na partida atual?
merge-batch-summary = { $merged } a mesclar, { $skipped } ignoradas
merge-batch-skip-no-match = { $count } sem posição coincidente
merge-batch-skip-nothing = { $count } que terminam na posição coincidente
merge-batch-skip-same = { $count } é a partida atual
merge-batch-skip-unreadable = { $count } que não puderam ser lidas
merge-batch-done = Mesclagem concluída: { $merged } mescladas, { $skipped } ignoradas

merge-err-unable = Não foi possível mesclar a partida escolhida: { $reason }
merge-err-no-game = Nenhuma partida está selecionada
merge-err-closed = A base escolhida não está aberta.
merge-err-number = Número de partida inválido.
merge-err-self = A partida não pode ser mesclada nela mesma.
merge-err-empty = A partida atual não tem lances.
merge-err-decode = Erro ao decodificar a partida.
merge-err-nomatch = Nenhuma posição coincidente encontrada.
merge-err-nothing = A partida a mesclar não tem lances depois da posição coincidente.
merge-err-failed = Um lance da partida não pôde ser acrescentado.
merge-err-trial = Indisponível no modo de teste.
merge-err-batch-limit = Partidas demais para mesclar de uma vez.

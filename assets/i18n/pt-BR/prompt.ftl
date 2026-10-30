prompt-replace-title = Substituir lance?
prompt-replace-body = Já existe um lance nesta posição. Escolha o que fazer com o seu. “Substituir lance” descarta todos os lances seguintes.
prompt-replace-replace = Substituir lance
prompt-replace-new-main = Nova linha principal
prompt-replace-add-variation = Adicionar variante
prompt-replace-try = Tentar variante
prompt-promotion-title = Promover a
prompt-strip-title = Confirmar remoção
prompt-strip-comments = { $count ->
    [one] Remover 1 comentário desta partida?
   *[other] Remover { $count } comentários desta partida?
}
prompt-strip-variations = { $count ->
    [one] Remover 1 variante desta partida?
   *[other] Remover { $count } variantes desta partida?
}
prompt-truncate-after = { $count ->
    [one] Remover 1 lance após esta posição?
   *[other] Remover { $count } lances após esta posição?
}
prompt-truncate-from = { $count ->
    [one] Remover este lance?
   *[other] Remover este lance e os seguintes ({ $count } lances no total)?
}
prompt-truncate-before = { $count ->
    [one] Remover 1 lance antes desta posição? A partida começará aqui.
   *[other] Remover { $count } lances antes desta posição? A partida começará aqui.
}
prompt-strip-other = Remover { $count } itens desta partida?
prompt-confirm-remove = Remover
goto-move-title = Ir para o lance número
goto-move-prompt = Número do lance (acrescente ... ou b para as pretas)
goto-move-placeholder = ex.: 12
goto-move-invalid = Informe um número de lance maior que zero.
paste-pgn-title = Colar PGN
paste-pgn-import = Importar
paste-pgn-current = Inserir a partida atual
paste-pgn-placeholder = Cole aqui o texto PGN
paste-pgn-errors = { $count ->
    [one] 1 problema encontrado
   *[other] { $count } problemas encontrados
}
import-issue-nothing-to-import = Não há lance nem resultado no texto.
import-issue-text-outside-games = Texto que não pertence a nenhuma partida foi ignorado ({ $bytes } bytes).
import-issue-truncated = A leitura parou após { $games } partidas: um limite foi atingido.
import-position-line = Linha { $line }: { $message }
import-position-column = Linha { $line }, coluna { $column }: { $message }
pgn-diag-bad-tag = Não foi possível ler uma linha de cabeçalho
pgn-diag-tag-too-long = Um valor do cabeçalho é longo demais e foi cortado
pgn-diag-rating-too-large = Um rating é grande demais e foi ignorado
pgn-diag-invalid-fen = O FEN da partida não é válido
pgn-diag-illegal-castling = Os direitos de roque do FEN não são possíveis
pgn-diag-castling-reinterpreted = Os direitos de roque do FEN foram reinterpretados
pgn-diag-illegal-move = Lance ilegal
pgn-diag-ambiguous-move = Lance ambíguo
pgn-diag-invalid-move = Lance ilegível
pgn-diag-too-many-nodes = A partida tem lances demais
pgn-diag-too-many-tags = A partida tem tags de cabeçalho demais
pgn-diag-duplicate-variation = Uma variante repete um lance existente
pgn-diag-invalid-nag-symbol = Símbolo de anotação desconhecido
pgn-diag-nag-out-of-range = Número de anotação fora do intervalo
pgn-diag-nag-without-move = Uma anotação aparece antes de qualquer lance
pgn-diag-variation-before-first-move = Uma variante aparece antes do primeiro lance
pgn-diag-unbalanced-close-paren = Um parêntese fecha sem ter sido aberto
pgn-diag-unclosed-variation = Uma variante não foi fechada
pgn-diag-stray-close-brace = Uma chave fecha sem ter sido aberta
pgn-diag-unterminated-comment = Um comentário não foi fechado
pgn-diag-comment-truncated = Um comentário é longo demais e foi cortado
pgn-diag-missing-result = A partida não tem resultado
pgn-diag-unexpected-token = Texto inesperado nos lances
pgn-diag-result-mismatch = O resultado do cabeçalho e o dos lances são diferentes
pgn-diag-invalid-text-in-header = Texto inválido em um cabeçalho
pgn-diag-header-only-at-eof = O texto termina após um cabeçalho, sem lances
pgn-diag-junk-between-games = Texto entre partidas foi ignorado
pgn-diag-encoding-fallback = A codificação do texto foi adivinhada
pgn-diag-chess960-detected = A partida é de Chess960
pgn-diag-unsupported-variant = Esta variante de xadrez não é suportada
pgn-diag-game-too-large = A partida é grande demais
pgn-diag-too-many-diagnostics = Há observações demais; o restante não é mostrado
pgn-diag-unknown = Observação desconhecida sobre o texto
chooser-title = Escolher uma partida
chooser-filter-placeholder = Filtrar por jogador, evento ou resultado
chooser-count = { $shown } de { $total } partidas
chooser-open = Abrir
variations-title = Variantes
action-clear = Limpar
prompt-dont-ask-again = Não perguntar de novo

chess960-title = Nova partida Chess960
chess960-prompt = Número da posição inicial (0 a 959; 518 é a disposição clássica):
chess960-placeholder = 0 a 959
chess960-random = Aleatória
chess960-start-play = Iniciar e jogar contra a engine...
chess960-start = Iniciar partida
chess960-invalid = Informe um número de 0 a 959.
chess960-hint = Para jogar contra uma engine use o segundo botão, ou Jogar > Jogar contra a engine... depois (começando da posição atual).

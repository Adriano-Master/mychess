# Exportação de documentos e imagens do tabuleiro (spec M7-4, fatia b).
# A camada de aplicação nomeia estas chaves; os rótulos do diálogo são da
# fatia c.

export-options = Opções de exportação
export-format-label = Formato
export-format-pgn = PGN
export-format-html = HTML
export-format-viewer = HTML com visualizador
export-format-latex = LaTeX
export-format-epd = Posições (EPD)
export-format-list = Lista de partidas (texto)
export-format-csv = Lista de partidas (CSV)

export-html-comments = Comentários
export-html-variations = Variantes
export-html-symbols = Símbolos nas anotações (! ?) em vez de números (NAG)
export-html-diagram-size = Tamanho dos diagramas (pixels, 0 para nenhum)
export-viewer-per-file = Partidas por arquivo
export-viewer-include-pgn = Gravar também o arquivo PGN
export-viewer-open-after = Abrir no navegador ao terminar
export-latex-comments = Comentários
export-latex-variations = Variantes
export-latex-flip = Tabuleiro visto das Pretas
export-latex-null-moves = Escrever lances nulos como comentários
export-epd-dedupe = Gravar cada posição uma só vez
export-epd-final-position = Usar a posição final de cada partida
export-epd-counters = Incluir os contadores de lances (hmvc, fmvn)
export-epd-id = Incluir um id com os jogadores e o evento
export-epd-game-number = Incluir o número da partida (c0)
export-list-format = Formato
export-list-legend = w Brancas, b Pretas, W Elo das Brancas, B Elo das Pretas, m Lances, r Resultado, g Número, d Data, c País, e Evento, s Local, f Número no filtro, C Comentários, y Ano, n Rodada, F Material final, S Começa de uma posição, D Apagada, o ECO, O Abertura, V Variantes
export-list-default = Padrão
export-csv-header = Linha de cabeçalho
export-csv-separator = Separador
export-csv-separator-comma = Vírgula
export-csv-separator-semicolon = Ponto e vírgula
export-csv-bom = Marca de ordem de bytes (para planilhas)
export-template-title = Modelo do documento
export-template-start = Texto no início do arquivo
export-template-end = Texto no fim do arquivo
export-template-reset = Restaurar padrão

export-confirm-replace = { $count ->
    [one] O arquivo { $files } já existe. Substituí-lo?
   *[other] Estes arquivos já existem: { $files }. Substituí-los?
}
export-confirm-and-more = e mais { $count }
export-confirm-append = As partidas serão acrescentadas a { $files }. As linhas finais do arquivo serão trocadas pelas novas partidas e pelo texto de fim. Continuar?

export-image-title = Exportar posição como imagem
export-image-format = Formato
export-image-size = Tamanho (pixels)
export-image-coordinates = Coordenadas
export-image-flip = Virado
export-image-last-move = Realçar o último lance
export-image-marks = Setas e marcações
export-image-transparent = Fundo transparente
export-image-margin = Margem (pixels)
export-image-preview = Pré-visualização
export-image-save = Salvar...

export-notice-viewer = { $volumes ->
    [one] { $count } partidas exportadas para { $name }
   *[other] { $count } partidas exportadas para { $volumes } arquivos, a partir de { $name }
}
export-notice-epd = { $repeated ->
    [0] { $count } posições exportadas para { $name }
   *[other] { $count } posições exportadas para { $name } ({ $repeated } repetidas deixadas de fora)
}
export-notice-image-saved = Imagem salva em { $name }
export-notice-open-failed = O sistema não conseguiu abrir o arquivo exportado

export-err-no-marker = O arquivo não tem a linha final antes da qual acrescentar as partidas (</body> ou \end{"{"}document{"}"}): ele não foi alterado
export-err-not-utf8 = O arquivo não é texto UTF-8; não é possível acrescentar partidas a ele: ele não foi alterado
export-err-empty-format = O formato da lista de partidas não tem nenhum campo conhecido
export-err-options = Uma opção está fora do intervalo permitido
export-err-assets = Os desenhos das peças não estão disponíveis
export-err-image-size = O tamanho ou a margem da imagem está fora do intervalo permitido
export-err-image-piece = O desenho de uma peça está ausente ou não é um SVG utilizável
export-err-image-font = A fonte das coordenadas está ausente ou é inválida
export-err-image-render = Não foi possível desenhar a imagem
export-err-image-protected = Esse arquivo é um dos bancos abertos e não pode ser sobrescrito
export-err-internal = A exportação falhou de forma inesperada

export-csv-col-number = Número
export-csv-col-filter-number = Número no filtro
export-csv-col-white = Brancas
export-csv-col-black = Pretas
export-csv-col-white-elo = Elo das Brancas
export-csv-col-black-elo = Elo das Pretas
export-csv-col-result = Resultado
export-csv-col-date = Data
export-csv-col-year = Ano
export-csv-col-event = Evento
export-csv-col-site = Local
export-csv-col-round = Rodada
export-csv-col-eco = ECO
export-csv-col-moves = Lances
export-csv-col-flags = Marcadores

# Os diálogos (fatia c, M7-4c).
menu-file-export-image = Exportar posição como imagem...
file-dialog-export-doc = Exportar partidas
file-dialog-export-append = Acrescentar as partidas a um arquivo
file-dialog-export-image = Salvar a imagem do tabuleiro
export-image-format-png = Imagem PNG
export-image-format-svg = Imagem SVG
export-ui-format-title = Formato do arquivo
export-ui-diagram-none = nenhum
export-ui-per-file-hint = Partidas em cada arquivo do visualizador
export-ui-list-hint = Letras que representam os campos de cada linha
export-ui-confirm-title = O arquivo já existe
export-ui-replace = Substituir
export-ui-add = Acrescentar as partidas
export-ui-needs-assets = Os desenhos das peças ainda não estão prontos; os diagramas e o visualizador podem falhar
export-ui-sep-hint = Separador de campos
export-report-repeated = Posições repetidas deixadas de fora: { $count }
export-report-not-selected = Partidas sem posição para gravar: { $count }
export-report-volumes = Arquivos gravados: { $count }
export-image-close = Fechar
export-image-picking = Esperando o nome do arquivo...
export-image-saving = Gravando a imagem...
export-image-no-preview = Não foi possível fazer a pré-visualização
export-image-margin-none = nenhuma
export-image-confirm = Substituir o arquivo { $name }?

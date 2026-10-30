dialog-ok = OK
dialog-cancel = Cancelar
about-title = Sobre o MyChess
about-version = Versão { $version }
about-license = Licenciado sob a Licença Pública Geral GNU, versão 3 ou posterior.
about-credits = Peças de xadrez de Colin M.L. Burnett (GPL-2.0-or-later). Fonte da interface: Noto Sans (SIL Open Font License 1.1).
about-description = Um programa de banco de dados e análise de xadrez, inspirado no Scid vs PC.
dialog-discard-title = Alterações não salvas
dialog-discard-message = A partida foi modificada. Deseja salvar as alterações?
dialog-discard-save = Salvar
dialog-discard-dont-save = Não salvar
dialog-overwrite-title = Substituir arquivo
dialog-overwrite-message = { $name } contém { $games ->
    [one] uma partida
   *[other] { $games } partidas
}. Substituir todas por esta partida?
dialog-overwrite-yes = Substituir
file-dialog-open-pgn = Abrir arquivo PGN
file-dialog-open-base = Abrir banco ou arquivo PGN
file-dialog-save-pgn = Salvar partida como PGN
file-dialog-append-pgn = Anexar partida a um arquivo PGN
file-dialog-open-eco = Abrir arquivo ECO
file-filter-pgn = Arquivos PGN
file-filter-eco = Arquivos ECO
file-dialog-open-mask = Abrir arquivo de máscara
file-dialog-save-mask = Salvar arquivo de máscara
file-filter-mask = Arquivos de máscara
file-filter-all = Todos os arquivos
dialog-overwrite-partial = { $name } foi lido só em parte; substituí-lo apaga o restante. Substituir?
dialog-overwrite-existing = { $name } já existe. Substituir?
dialog-copy-title = Copiar partidas
dialog-copy-message = Copiar { $count ->
    [one] uma partida
   *[other] { $count } partidas
} de { $from } para { $to }?
dialog-copy-selected-message = Copiar { $count ->
    [one] a partida selecionada
   *[other] as { $count } partidas selecionadas
} de { $from } para { $to }?
dialog-copy-yes = Copiar
file-dialog-import-pgn = Importar arquivos PGN
file-dialog-new-base = Nome da nova base
file-filter-base = Bases do Scid

# Importação de arquivos PGN (spec M3-8). Scid: `importPgnFile`, "Importing PGN file".
file-import-title = Importando arquivos PGN
file-import-target-existing = Banco: { $name }
file-import-target-new = Banco novo: { $name }
file-import-files = Arquivos ({ $count })
file-import-files-more = ... e mais { $count }
file-import-progress = Arquivo { $file } de { $total } - { $games } partidas gravadas - { $percent }%
file-import-stop = Parar
file-import-close = Fechar
file-import-summary = Partidas encontradas: { $read }. Importadas: { $added }. Não importadas: { $skipped }. Avisos: { $warnings }.
file-import-interrupted = Interrompida: as partidas já lidas foram gravadas.
file-import-nothing = Nenhuma partida foi gravada.
file-import-removed = O banco novo não foi mantido porque nenhuma partida foi gravada.
file-import-error = A importação terminou antes do fim: { $reason }
file-import-file-error = Erro ao abrir { $file }: { $reason }
file-import-line = { $file }, partida { $game }, linha { $line }: { $text }
file-import-line-game = { $file }, partida { $game }: { $text }
file-import-line-file = { $file }: { $text }
file-import-more = ... e mais { $count } observações que não são mostradas.
file-import-opened = O banco novo foi aberto.
file-import-kept = O banco novo { $name } foi mantido, mas não foi aberto.
file-import-notice-imported = { $count ->
    [one] Uma partida importada
   *[other] { $count } partidas importadas
}
file-import-notice-interrupted = { $count ->
    [one] Importação interrompida: uma partida foi gravada
   *[other] Importação interrompida: { $count } partidas foram gravadas
}
file-import-notice-failed = { $count ->
    [one] A importação terminou com erro depois de uma partida: { $reason }
   *[other] A importação terminou com erro depois de { $count } partidas: { $reason }
}
file-import-notice-nothing = Nenhuma partida foi encontrada no arquivo
file-import-notice-tag-truncated = Um valor de tag com mais de 255 bytes foi encurtado
file-import-notice-extra-tags = Mais de 40 tags extras: as demais foram descartadas
file-import-notice-empty-tag = Uma tag com nome vazio foi descartada
file-import-notice-tag-name = Um nome de tag com mais de 240 bytes foi descartado
file-import-notice-name-truncated = Um nome com mais de 255 bytes foi encurtado
file-import-notice-flag = Uma letra de ScidFlags não é uma marca
file-import-notice-no-games = O arquivo não tem partidas
file-import-notice-tag-dropped = Uma tag foi descartada porque o registro não a comporta
file-import-notice-other = A partida foi corrigida
file-import-skip-too-large = A partida é grande demais para o banco e foi ignorada
file-import-skip-variant = A partida é de uma variante sem suporte e foi ignorada
file-import-skip-game-too-large = A partida passa do limite de tamanho e foi ignorada
file-import-skip-unencodable = A partida não pode ser gravada e foi ignorada
file-import-skip-other = A partida foi ignorada
file-import-stop-read = Não foi possível ler o arquivo até o fim

# Exportação de partidas (spec M3-9). Scid: `exportGames`, "Exportar partidas".
export-games-title = Exportar partidas
export-games-what = O que exportar
export-games-scope-all = Todas as partidas da base ({ $count })
export-games-scope-filter = As partidas do filtro ({ $count })
export-games-scope-selection = As partidas selecionadas ({ $count })
export-games-scope-current = A partida atual
export-games-comments = Incluir comentários
export-games-variations = Incluir variantes
export-games-marks = Incluir comandos como [%eval] e [%clk]
export-games-only-tactics = Somente partidas com a bandeira Tática (T)
export-games-append = Acrescentar ao fim do arquivo
export-games-choose = Escolher destino...
export-games-cancel = Cancelar
export-games-picking = Aguardando o nome do arquivo...
export-games-stop = Parar
export-games-close = Fechar
export-games-progress = { $done } de { $total } partidas - { $percent }%
export-games-destination = Arquivo: { $name }
export-games-summary = Partidas exportadas: { $written }. Fora por não terem a bandeira T: { $excluded }. Não exportadas: { $skipped }.
export-games-appended = As partidas foram acrescentadas ao fim do arquivo existente.
export-games-stopped = Interrompido.
export-games-error = A exportação falhou: { $reason }
export-games-line = Partida { $game }: { $text }
export-games-more = ... e mais { $count } partidas que não são listadas.
export-games-skip-unreadable = A partida não pôde ser lida do banco
export-games-skip-unwritable = A partida não pôde ser escrita como PGN
export-games-notice-exported = { $count ->
    [0] Nenhuma partida exportada
    [one] Uma partida exportada para { $name }
   *[other] { $count } partidas exportadas para { $name }
}
export-games-notice-skips = { $count ->
    [0] Nenhuma partida exportada para { $name }; { $skipped ->
        [one] uma não exportada
       *[other] { $skipped } não exportadas
    }
    [one] Uma partida exportada para { $name }; { $skipped ->
        [one] uma não exportada
       *[other] { $skipped } não exportadas
    }
   *[other] { $count } partidas exportadas para { $name }; { $skipped ->
        [one] uma não exportada
       *[other] { $skipped } não exportadas
    }
}
export-games-notice-all-skipped = { $skipped ->
    [one] A partida não pôde ser exportada: o arquivo não foi alterado
   *[other] Nenhuma das { $skipped } partidas pôde ser exportada: o arquivo não foi alterado
}
export-games-notice-stopped = Exportação interrompida: o arquivo não foi gravado
export-games-notice-stopped-appended = { $count ->
    [0] Exportação interrompida: nenhuma partida foi acrescentada ao arquivo
    [one] Exportação interrompida: uma partida já tinha sido acrescentada ao arquivo
   *[other] Exportação interrompida: { $count } partidas já tinham sido acrescentadas ao arquivo
}
export-games-notice-nothing = Nenhuma partida para exportar: nada foi gravado

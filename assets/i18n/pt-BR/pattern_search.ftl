# Busca de táticas e padrões (spec M6-2, fatia d). As frases dos cartões
# saem de uma chave mais argumentos (veja o rustdoc de
# `mychess_app::Sentence`): cada região traz a própria preposição e a
# interface tira "em qualquer casa" de dentro das frases.

menu-search-pattern = Táticas e padrões...

pattern-title = Táticas e padrões
pattern-tab-presets = Prontas
pattern-tab-advanced = Avançada
pattern-presets-hint = Escolha uma tática e aperte Procurar.
pattern-customize = Personalizar
pattern-customize-tip = Copiar este padrão para a aba Avançada para alterá-lo
pattern-run = Procurar
pattern-stop = Parar
pattern-stop-tip = Interromper a busca; o filtro fica como estava
pattern-close = Fechar
pattern-hide = Ver o tabuleiro
pattern-hide-tip = Esconder esta janela para ver as ocorrências no tabuleiro
pattern-show = Táticas e padrões
pattern-show-tip = Abrir a janela de busca de novo
pattern-test-game = Testar nesta partida
pattern-test-game-tip = Rodar o padrão só na partida do tabuleiro, sem mexer no filtro
pattern-test-result = { $count ->
    [one] Vale em uma posição desta partida
   *[other] Vale em { $count } posições desta partida
}
pattern-test-none = Não vale em nenhuma posição desta partida

# Escopo e execução.
pattern-scope-title = Onde procurar
pattern-op = Operação sobre o filtro
pattern-op-reset = Procurar em todas as partidas
pattern-op-and = Restringir o filtro
pattern-op-or = Somar ao filtro
pattern-reference = Procurar na base
pattern-reference-none = Escolha uma base
pattern-move-range = Número do lance
pattern-hold = Vale por pelo menos
pattern-hold-unit = { $count ->
    [one] meio-lance
   *[other] meios-lances
}
pattern-end-only = Só a posição final
pattern-ignore-colors = Ignorar as cores
pattern-progress = { $done } de { $total } partidas
pattern-summary = { $matched } / { $start } partidas ({ $secs } s)
pattern-interrupted = Busca interrompida
pattern-unknown = { $count ->
    [one] Uma posição complexa demais para verificar
   *[other] { $count } posições complexas demais para verificar
}
pattern-unreadable = { $count ->
    [one] Uma partida ilegível
   *[other] { $count } partidas ilegíveis
}
pattern-cost-title = Busca demorada
pattern-cost-warning = Esta busca pode demorar (cerca de { $positions } posições em { $games } partidas). Continuar?
pattern-cost-yes = Buscar mesmo assim
pattern-cost-no = Cancelar

# Ocorrências.
pattern-occurrence = Ocorrência { $n } de { $total }
pattern-occurrence-test = Teste: ocorrência { $n } de { $total }
pattern-hit-prev = Ocorrência anterior
pattern-hit-next = Próxima ocorrência
pattern-hit-clear = Esquecer as ocorrências
pattern-hit-none = Nenhuma ocorrência nesta partida

# Padrões prontos.
pattern-preset-mate-in = Mate em { $n }
pattern-preset-pin = Cravada
pattern-preset-fork = Garfo
pattern-preset-skewer = Espeto
pattern-preset-discovered = Ataque descoberto
pattern-preset-double-attack = Ataque duplo
pattern-preset-sacrifice = Sacrifício
pattern-preset-hanging = Peça pendurada
pattern-preset-passed-pawn = Peão passado
pattern-preset-exposed-king = Rei exposto
pattern-preset-back-rank = Mate do corredor
pattern-preset-promotion = Promoção
pattern-preset-iqp = Peão isolado da dama (Brancas)
pattern-preset-iqp-black = Peão isolado da dama (Pretas)
pattern-preset-iqp-both = Peões isolados da dama (os dois lados)
pattern-preset-hanging-pawns = Peões pendentes
pattern-preset-maroczy = Centro Maróczy
pattern-preset-fianchetto-light = Fianchetto (bispo de casas claras)
pattern-preset-fianchetto-dark = Fianchetto (bispo de casas escuras)
pattern-preset-fianchetto-four = Fianchetto duplo
pattern-preset-opposite-castling = Roques opostos
pattern-preset-mate-in-desc = Um dos lados tem mate no número de lances escolhido.
pattern-preset-pin-desc = Uma peça não pode se mover sem expor outra mais valiosa.
pattern-preset-fork-desc = Uma peça ataca, ao mesmo tempo, dois alvos que valem a pena.
pattern-preset-skewer-desc = Uma peça valiosa é atacada e, ao sair, expõe a que está atrás.
pattern-preset-discovered-desc = Mover uma peça abre a linha de outra.
pattern-preset-double-attack-desc = Um lance ataca dois alvos ao mesmo tempo.
pattern-preset-sacrifice-desc = Material é entregue e depois recuperado, ou termina em mate.
pattern-preset-hanging-desc = Uma peça é atacada e ninguém a defende.
pattern-preset-passed-pawn-desc = Um peão sem peão inimigo à frente nem nas colunas vizinhas.
pattern-preset-exposed-king-desc = O rei perdeu a proteção dos peões.
pattern-preset-back-rank-desc = Xeque-mate na primeira linha, com o rei preso pelos próprios peões.
pattern-preset-promotion-desc = Um peão chega à última linha e vira outra peça.
pattern-preset-iqp-desc = Peão branco em d4, sem peões brancos nas colunas c e e.
pattern-preset-iqp-black-desc = Peão preto em d5, sem peões pretos nas colunas c e e.
pattern-preset-iqp-both-desc = Peões isolados da dama dos dois lados.
pattern-preset-hanging-pawns-desc = Peões lado a lado em c e d, sem peões em b e e.
pattern-preset-maroczy-desc = Peões brancos em c4 e e4, sem peão branco na coluna d.
pattern-preset-fianchetto-light-desc = O bispo de casas claras na grande diagonal.
pattern-preset-fianchetto-dark-desc = O bispo de casas escuras na grande diagonal.
pattern-preset-fianchetto-four-desc = Os dois bispos nas grandes diagonais.
pattern-preset-opposite-castling-desc = Um rei rocou para o lado da dama e o outro para o lado do rei.

# Parâmetros dos padrões prontos.
pattern-param-by = Quem
pattern-param-n = Lances até o mate
pattern-param-mate-mode = Mate
pattern-param-max-nodes = Esforço da busca (posições)
pattern-param-piece = Peça
pattern-param-target-piece = Peça-alvo
pattern-param-against-king = Cravada contra o rei
pattern-param-discovery = Tipo
pattern-param-min-net = Material entregue (no mínimo)
pattern-param-outcome = Depois
pattern-param-within = Lances para recuperar
pattern-param-require-win = Só em partidas que o lado venceu
pattern-param-min-own-rank = Avançado até a linha
pattern-param-rank-any = Qualquer
pattern-param-other = Opção
pattern-mate-mode-played = Mate que foi jogado
pattern-mate-mode-available = Qualquer mate disponível
pattern-mate-forcing-only = Só mates que começam com xeque
pattern-discovery-attack = Ataque
pattern-discovery-check = Xeque
pattern-discovery-double-check = Xeque duplo
pattern-any-piece-hint = Sem nenhuma peça marcada vale qualquer peça.

# Valores.
pattern-who-white = Brancas
pattern-who-black = Pretas
pattern-who-any = Qualquer lado
pattern-who-mover = Quem joga
pattern-who-opponent = Quem não joga
pattern-role-king = rei
pattern-role-queen = dama
pattern-role-rook = torre
pattern-role-bishop = bispo
pattern-role-knight = cavalo
pattern-role-pawn = peão
pattern-role-any = qualquer peça
pattern-role-minor = peça menor
pattern-phase-opening = abertura
pattern-phase-middlegame = meio-jogo
pattern-phase-endgame = final
pattern-status-check = em xeque
pattern-status-checkmate = em xeque-mate
pattern-status-stalemate = afogado
pattern-result-white-wins = Brancas vencem
pattern-result-draw = empate
pattern-result-black-wins = Pretas vencem
pattern-result-unknown = sem resultado
pattern-flag-yes = sim
pattern-flag-no = não
pattern-flag-both = tanto faz
pattern-castle-none = nenhum
pattern-castle-short = curto
pattern-castle-long = longo
pattern-castle-any = qualquer
pattern-gain-any = qualquer ataque
pattern-gain-undefended = só se estiver sem defesa
pattern-gain-wins-material = só se ganhar material
pattern-when-any = a qualquer momento
pattern-when-actor-just-moved = logo depois de a peça se mover
pattern-when-actor-to-move = quando o lado dela joga
pattern-outcome-recovered = o material é recuperado
pattern-outcome-mate = termina em mate
pattern-outcome-either = o material é recuperado ou termina em mate
pattern-outcome-any = sem outra exigência
pattern-bishops-either = Bispos quaisquer
pattern-bishops-same = bispos de mesma cor
pattern-bishops-opposite = bispos de cores opostas
pattern-list-and = { $head } e { $last }
pattern-list-or = { $head } ou { $last }

# Regiões.
pattern-region-anywhere = em qualquer casa
pattern-region-kind-anywhere = Qualquer casa
pattern-region-kind-squares = Casas
pattern-region-kind-file = Coluna
pattern-region-kind-rank = Linha
pattern-region-kind-own-rank = Linha própria
pattern-region-kind-rect = Retângulo
pattern-region-kind-named = Área
pattern-region-pick = Clique nas casas para escolhê-las
pattern-region-use-marks = Usar as casas marcadas
pattern-region-clear = Limpar
pattern-region-from = De
pattern-region-to = Até

# As peças das frases dos cartões.
pattern-ref-actor = { $piece } ({ $side }){ $where }
pattern-ref-target = { $piece }{ $where }
pattern-material-count =
    { $piece } ({ $side }): { $same ->
        [1] { $min }
       *[other] { $min } a { $max }
    }
pattern-material-diff =
    diferença (Brancas - Pretas): { $same ->
        [1] { $min }
       *[other] { $min } a { $max }
    }

# As frases dos cartões.
pattern-sentence-piece = { $side }: { $piece }{ $where }
pattern-sentence-piece-none = { $side }: { $piece }{ $where } (quantidade: 0)
pattern-sentence-piece-exactly = { $side }: { $piece }{ $where } (quantidade: { $count })
pattern-sentence-piece-between = { $side }: { $piece }{ $where } (quantidade: de { $min } a { $max })
pattern-sentence-piece-at-least = { $side }: { $piece }{ $where } (quantidade: pelo menos { $min })
pattern-sentence-material = Material: { $parts }
pattern-sentence-material-any = Material: qualquer
pattern-sentence-side-to-move = Lado que joga: { $side }
pattern-sentence-phase = Fase: { $phase }
pattern-sentence-status = Quem joga está { $status }
pattern-sentence-move-previous = Lance anterior: { $by }, { $piece }{ $from }{ $to }; captura: { $capture }; xeque: { $check }; promoção: { $promotion }; roque: { $castling }
pattern-sentence-move-next = Próximo lance: { $by }, { $piece }{ $from }{ $to }; captura: { $capture }; xeque: { $check }; promoção: { $promotion }; roque: { $castling }
pattern-sentence-result = Resultado da partida: { $results }
pattern-sentence-passed-pawn = { $side }: peão passado{ $where } (pelo menos { $min })
pattern-sentence-isolated-pawn = { $side }: peão isolado{ $where } (pelo menos { $min })
pattern-sentence-doubled-pawns = { $side }: peões dobrados{ $where } (pelo menos { $min })
pattern-sentence-exposed-king = { $side }: rei exposto{ $where }
pattern-sentence-bishop-pair = { $side }: tem o par de bispos
pattern-sentence-attacks =
    { $actor } ataca { $target } ({ $gain }; { $when }{ $new ->
        [1] ; criado pelo último lance
       *[other] { "" }
    })
pattern-sentence-defends =
    { $actor } defende { $target } ({ $when }{ $new ->
        [1] ; criado pelo último lance
       *[other] { "" }
    })
pattern-sentence-pin =
    { $actor } crava { $target } contra { $behind } ({ $when }{ $new ->
        [1] ; criado pelo último lance
       *[other] { "" }
    })
pattern-sentence-skewer =
    { $actor } espeta { $target } diante de { $behind } ({ $when }{ $new ->
        [1] ; criado pelo último lance
       *[other] { "" }
    })
pattern-sentence-fork =
    { $actor } dá garfo em { $count } ou mais de: { $target } ({ $when }{ $new ->
        [1] ; criado pelo último lance
       *[other] { "" }
    })
pattern-sentence-discovered-attack = { $by }: ataque descoberto (peça que se moveu: { $piece })
pattern-sentence-discovered-check = { $by }: xeque descoberto (peça que se moveu: { $piece })
pattern-sentence-double-check = { $by }: xeque duplo (peça que se moveu: { $piece })
pattern-sentence-double-attack = { $by }: ataque duplo
pattern-sentence-sacrifice = { $by }: sacrifica { $piece } (perda líquida de pelo menos { $net }); { $outcome } em até { $within } { $within ->
    [one] lance
   *[other] lances
}
pattern-sentence-mate =
    { $by }: mate em { $same ->
        [1] { $min }
       *[other] { $min } a { $max }
    } { $max ->
        [one] lance
       *[other] lances
    } ({ $mode }{ $forcing ->
        [1] ; { pattern-mate-forcing-only }
       *[other] { "" }
    })

# O editor.
pattern-add-condition = Adicionar condição
pattern-cat-logic = Grupos
pattern-cat-pieces = Peças e casas
pattern-cat-relations = Relações
pattern-cat-tactics = Táticas
pattern-cat-structure = Estrutura e segurança do rei
pattern-cat-move = Lance e estado
pattern-group-all = Todas estas (E)
pattern-group-any = Qualquer uma destas (OU)
pattern-group-not = Não isto (NÃO)
pattern-group-short-all = E
pattern-group-short-any = OU
pattern-group-short-not = NÃO
pattern-cond-piece = Peça em casas
pattern-cond-material = Material
pattern-cond-side = Vez de jogar
pattern-cond-phase = Fase
pattern-cond-status = Estado
pattern-cond-previous-move = Lance anterior
pattern-cond-next-move = Próximo lance
pattern-cond-result = Resultado da partida
pattern-cond-passed-pawn = Peão passado
pattern-cond-isolated-pawn = Peão isolado
pattern-cond-doubled-pawns = Peões dobrados
pattern-cond-exposed-king = Rei exposto
pattern-cond-bishop-pair = Par de bispos
pattern-cond-attacks = Ataca
pattern-cond-defends = Defende
pattern-cond-pins = Crava
pattern-cond-skewers = Espeta
pattern-cond-forks = Dá garfo
pattern-cond-discovered = Ataque descoberto
pattern-cond-double-attack = Ataque duplo
pattern-cond-sacrifice = Sacrifício
pattern-cond-mate = Mate em N
pattern-empty-tree = Adicione uma condição para começar.
pattern-pick-card = Escolha um cartão para editá-lo.
pattern-remove = Remover
pattern-move-up = Subir
pattern-move-down = Descer
pattern-wrap = Envolver em
pattern-unwrap = Desembrulhar
pattern-change-group = Transformar em
pattern-new = Novo padrão
pattern-name-label = Nome
pattern-name-placeholder = Nome do padrão
pattern-description-placeholder = Descrição (opcional)
pattern-issue-here = Corrija este cartão primeiro.
pattern-field-side = Lado
pattern-field-piece = Peça
pattern-field-where = Casas
pattern-field-actor = Peça que age
pattern-field-target = Alvo
pattern-field-behind = Peça atrás
pattern-field-count = Quantidade
pattern-field-min = No mínimo
pattern-field-max = No máximo
pattern-field-limit = Limitar
pattern-field-diff = Diferença
pattern-field-bishops = Bispos
pattern-field-gain = Ganho
pattern-field-when = Quando
pattern-field-new = Só se o último lance criou isso
pattern-field-targets = Número de alvos
pattern-field-capture = Captura
pattern-field-check = Xeque
pattern-field-promotion = Promoção
pattern-field-castling = Roque
pattern-field-net = Perda líquida (no mínimo)
pattern-field-within = Em até (lances)
pattern-field-outcome = Depois
pattern-field-mate-min = Menos lances
pattern-field-mate-max = Mais lances
pattern-field-nodes = Esforço da busca
pattern-field-phase = Fase
pattern-field-status = Estado
pattern-field-results = Resultados
pattern-field-discovery = Tipo
pattern-field-moved = Peça que se moveu

# Meus padrões.
pattern-saved-title = Meus padrões
pattern-save = Salvar padrão...
pattern-save-tip = Salvar o padrão com este nome
pattern-import = Importar...
pattern-export = Exportar...
pattern-import-title = Importar um padrão
pattern-export-title = Exportar o padrão
pattern-file-filter = Padrões do MyChess
pattern-saved-empty = Ainda não há padrões salvos.
pattern-saved-load-tip = Abrir este padrão na aba Avançada
pattern-saved-delete-tip = Apagar este padrão
pattern-saved-damaged = (arquivo ilegível)
pattern-saved-no-folder = Não há pasta para os padrões salvos.

# Avisos.
pattern-notice-unknown-preset = Não existe um padrão pronto chamado { $name }.
pattern-notice-unknown-preset-blank = Não existe esse padrão pronto.
pattern-notice-no-database = Não há base para pesquisar.
pattern-notice-stale = A base mudou durante a busca; o resultado foi descartado.
pattern-notice-failed = Não foi possível concluir a busca.
pattern-notice-hits-failed = Não foi possível calcular as ocorrências nesta partida.
pattern-notice-no-saved-folder = Não há pasta para os padrões salvos.
pattern-notice-saved = Padrão "{ $name }" salvo.
pattern-notice-deleted = Padrão "{ $name }" apagado.
pattern-notice-saved-not-found = Não há padrão salvo chamado "{ $name }".
pattern-notice-invalid-saved-name = Este nome não pode ser usado para um padrão.
pattern-notice-saved-failed = Não foi possível ler ou gravar o padrão salvo.
pattern-notice-imported = Padrão "{ $name }" importado.
pattern-notice-import-failed = Não foi possível importar o padrão.
pattern-notice-exported = Padrão exportado.
pattern-notice-export-failed = Não foi possível exportar o padrão.
pattern-error-empty-group = Este grupo está vazio.
pattern-error-too-deep = O padrão tem grupos aninhados demais.
pattern-error-too-many-nodes = O padrão tem condições demais.
pattern-error-mover-not-allowed = "Quem joga" não vale aqui: depois do lance, use "Quem não joga".
pattern-error-invalid = Valor inválido em { $path }.
pattern-error-invalid-param = Valor inválido em "{ $field }".
pattern-error-file = Não foi possível ler este arquivo de padrão.
pattern-error-newer-version = Este padrão foi salvo por uma versão mais nova do MyChess.
pattern-edit-refused-bad-path = Esse cartão não existe mais.
pattern-edit-refused-not-a-group = Só um grupo E / OU aceita itens novos.
pattern-edit-refused-root = O primeiro grupo não pode ser removido nem movido.
pattern-edit-refused-too-deep = Os grupos ficariam aninhados demais.
pattern-edit-refused-too-many-nodes = O padrão ficaria com condições demais.
pattern-edit-refused-group-full = Um grupo aceita no máximo { $limit } itens.
pattern-edit-refused-move-into-itself = Um cartão não pode ser movido para dentro de si mesmo.
pattern-edit-refused-needs-one-item = O grupo precisa ter exatamente um item.
pattern-edit-refused-no-region = Este cartão não tem casas para definir.
pattern-edit-refused-no-marks = O comentário do tabuleiro não marca nenhuma casa.

pattern-region-squares =
    { $mode ->
        [from] de { $squares }
        [to] para { $squares }
       *[at] em { $squares }
    }

pattern-region-file =
    { $mode ->
        [from] da coluna { $file }
        [to] para a coluna { $file }
       *[at] na coluna { $file }
    }

pattern-region-rank =
    { $mode ->
        [from] da linha { $rank }
        [to] para a linha { $rank }
       *[at] na linha { $rank }
    }

pattern-region-own-rank =
    { $mode ->
        [from] da sua linha { $rank }
        [to] para a sua linha { $rank }
       *[at] na sua linha { $rank }
    }

pattern-region-rect =
    { $mode ->
        [from] do retângulo { $from }-{ $to }
        [to] para o retângulo { $from }-{ $to }
       *[at] no retângulo { $from }-{ $to }
    }

pattern-region-named-center =
    { $mode ->
        [from] das casas centrais
        [to] para as casas centrais
       *[at] nas casas centrais
    }

pattern-region-named-extended-center =
    { $mode ->
        [from] do centro ampliado
        [to] para o centro ampliado
       *[at] no centro ampliado
    }

pattern-region-named-queen-side =
    { $mode ->
        [from] da ala da dama
        [to] para a ala da dama
       *[at] na ala da dama
    }

pattern-region-named-king-side =
    { $mode ->
        [from] da ala do rei
        [to] para a ala do rei
       *[at] na ala do rei
    }

pattern-region-named-white-half =
    { $mode ->
        [from] da metade das Brancas
        [to] para a metade das Brancas
       *[at] na metade das Brancas
    }

pattern-region-named-black-half =
    { $mode ->
        [from] da metade das Pretas
        [to] para a metade das Pretas
       *[at] na metade das Pretas
    }

pattern-region-named-light-squares =
    { $mode ->
        [from] das casas claras
        [to] para as casas claras
       *[at] nas casas claras
    }

pattern-region-named-dark-squares =
    { $mode ->
        [from] das casas escuras
        [to] para as casas escuras
       *[at] nas casas escuras
    }

pattern-region-named-edge =
    { $mode ->
        [from] das bordas do tabuleiro
        [to] para as bordas do tabuleiro
       *[at] nas bordas do tabuleiro
    }

pattern-region-named-long-diagonals =
    { $mode ->
        [from] das grandes diagonais
        [to] para as grandes diagonais
       *[at] nas grandes diagonais
    }

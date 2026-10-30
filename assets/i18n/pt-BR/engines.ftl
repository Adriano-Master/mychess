# Gerência de engines (spec M4-3): diálogo Engines, editor, opções UCI,
# avisos da barra de status e menu Ferramentas.

menu-tools = Ferramentas
menu-tools-engines = Engines...
menu-tools-start-engine = Iniciar engine { $number }
menu-tools-toggle-analysis = Ligar ou desligar a análise

action-start-engine = Iniciar engine
action-manage-engines = Engines
action-toggle-engine-analysis = Ligar ou desligar a análise da engine

# ---- diálogo Engines ----
engines-title = Engines
engines-col-name = Nome
engines-col-key = Tecla
engines-col-type = Tipo
engines-col-elo = Elo
engines-col-last-used = Último uso
engines-empty = Ainda não há engines. Adicione uma ou procure as engines instaladas neste computador.
engines-btn-new = Novo
engines-btn-edit = Editar
engines-btn-copy = Copiar
engines-btn-delete = Excluir
engines-btn-up = Subir
engines-btn-down = Descer
engines-btn-options = Opções UCI...
engines-btn-start = Iniciar
engines-btn-stop = Parar
engines-btn-restart = Reiniciar
engines-btn-close = Fechar
engines-btn-find = Procurar engines instaladas
engines-btn-import = Importar engines.dat do Scid...
engines-btn-dismiss = Dispensar
engines-btn-add = Adicionar
engines-btn-analysis-options = Opções de análise
engines-status-starting = iniciando...
engines-status-running = em execução
engines-status-stopping = encerrando...
engines-status-failed = falhou
engines-finding = Procurando engines...
engines-found-title = Engines encontradas (clique para adicionar)
engines-found-note = Adicionar uma engine executa o programa uma vez para identificá-lo.
engines-found-none = Nenhuma engine conhecida foi encontrada.
engines-probing = Verificando { $count ->
    [one] uma engine
   *[other] { $count } engines
}...
engines-many-running = { $count } engines estão em execução; isso usa muita CPU.
engines-delete-title = Confirmar exclusão
engines-delete-message = Excluir a engine { $name } ({ $command })?
engines-issues-title = Observações sobre a lista de engines
engines-issue-field-reset = Entrada { $entry }: o campo { $field } foi restaurado.
engines-issue-entry-dropped = A entrada { $entry } foi descartada.
engines-issue-not-a-table = O item { $entry } do arquivo não é uma engine e foi ignorado.
engines-issue-duplicate-hotkey = A entrada { $entry } perdeu a tecla: outra engine já a tinha.
engines-issue-too-many = Só as primeiras { $limit } engines foram lidas.
engines-import-summary = Importadas: { $added }; ignoradas: { $duplicates } já na lista e { $invalid } inutilizáveis.
engines-import-incomplete = O arquivo foi cortado ou tem uma parte que não pôde ser lida.
engines-import-relative = { $engine }: o comando ou a pasta é relativo à pasta do Scid; confira.
engines-import-args = { $engine }: os parâmetros não puderam ser separados com exatidão; confira.

# ---- preferências de análise ----
analysis-pref-title = Opções de análise
analysis-pref-log-size = Tamanho do log (linhas, 0 = desligado)
analysis-pref-max-ply = Profundidade máxima (0 = ilimitada)
analysis-pref-add-name = Incluir o nome da engine nos comentários
analysis-pref-low-priority = Prioridade baixa de CPU
analysis-pref-wrap-pv = Quebrar linhas longas
analysis-pref-show-info = Mostrar informações extras

# ---- editor ----
engines-edit-title-new = Nova engine
engines-edit-title = Editar engine
engines-field-name = Nome
engines-field-command = Comando
engines-field-args = Parâmetros
engines-field-dir = Pasta
engines-field-url = Página
engines-field-elo = Elo
engines-field-protocol = Protocolo
engines-field-key = Tecla
engines-required = Os campos em negrito são obrigatórios; os outros são opcionais.
engines-browse = Procurar...
engines-protocol-uci = UCI
engines-protocol-xboard = XBoard
engines-key-none = Nenhuma
engines-btn-test = Testar engine
engines-btn-test-tip = Executa o programa para ler o nome e as opções.
engines-btn-detect-tip = Executa o programa para descobrir qual protocolo ele fala.
engines-btn-detect = Detectar protocolo
engines-btn-use-detected = Usar
engines-btn-save = Salvar
engines-test-running = Testando...
engines-test-ok = OK: { $name } ({ $protocol }, { $options } opções)
engines-test-failed = O teste falhou: { $reason }
engines-detect-running = Detectando...
engines-detect-found = A engine fala { $protocol }.
engines-detect-failed = A detecção falhou: { $reason }
engines-warning-command-not-found = O comando não foi encontrado neste computador.
engines-warning-not-executable = O arquivo não pode ser executado.
engines-warning-working-dir-missing = A pasta não existe.
engines-pick-command-title = Escolha o programa da engine
engines-pick-dat-title = Escolha o engines.dat do Scid
engines-filter-exe = Programas
engines-filter-dat = Listas de engines (engines.dat)

engines-error-empty-name = O nome não pode ficar vazio.
engines-error-empty-command = O comando não pode ficar vazio.
engines-error-elo-range = O Elo deve estar entre 0 e 4000.
engines-error-elo-number = O Elo deve ser um número.
engines-error-path-not-utf8 = O caminho tem caracteres que não podem ser gravados.
engines-error-too-long = Um campo está comprido demais.
engines-error-args-quote = Os parâmetros têm uma aspa que não foi fechada.

# ---- opções UCI ----
engines-options-title = Opções UCI: { $name }
engines-options-probing = Iniciando a engine para ler as opções...
engines-options-failed = Não foi possível iniciar a engine: { $reason }
engines-options-not-uci = Só engines UCI têm opções.
engines-options-none = A engine não tem opções.
engines-options-default = padrão: { $default }
engines-options-reset = Restaurar padrões
engines-options-save = Salvar
engines-options-button-hint = Os botões só agem com a engine em execução.
engines-option-multipv = Número de linhas (MultiPV)
engines-option-hash = Memória Hash (MB)
engines-option-ownbook = Usar o livro da própria engine
engines-option-bookfile = Arquivo de livro
engines-option-limitstrength = Limitar a força
engines-option-error-range = O valor deve estar entre { $min } e { $max }.
engines-option-error-not-in-list = O valor não é uma das opções.
engines-option-error-wrong-type = O valor tem o tipo errado.
engines-option-error-control = O texto não pode ter quebras de linha.
engines-option-error-other = O valor não é aceito.

# ---- falhas e avisos (as chaves vêm do aplicativo) ----
engines-failure-exit-code = código de saída { $code }
engines-failure-stderr = A engine disse: { $text }
engines-failure-cannot-start = Não foi possível iniciar o programa{ $detail }.
engines-failure-exited-early = A engine terminou antes de ficar pronta{ $detail }.
engines-failure-crashed = A engine terminou sozinha{ $detail }.
engines-failure-timeout = A engine não respondeu a tempo{ $detail }.
engines-failure-stop-timeout = A engine ignorou o pedido de parada e foi encerrada{ $detail }.
engines-failure-not-an-engine = O arquivo não é uma engine ou não fala UCI nem XBoard{ $detail }.
engines-failure-io = A comunicação com a engine falhou{ $detail }.
engines-failure-closed = A engine já estava fechada{ $detail }.
engines-failure-other = A engine falhou{ $detail }.
engines-notice-with-name = { $name }: { $text }
engines-notice-file-unreadable = A lista de engines não pôde ser lida; uma cópia do arquivo foi guardada.
engines-notice-newer-version = A lista de engines foi gravada por uma versão mais nova do MyChess.
engines-notice-file-fixed = A lista de engines tinha { $count ->
    [one] um problema, que foi corrigido
   *[other] { $count } problemas, que foram corrigidos
}.
engines-notice-save-failed = A lista de engines não pôde ser salva.
engines-notice-no-engine-on-key = Nenhuma engine tem a tecla { $key }.
engines-notice-no-engine-running = Nenhuma engine está em execução.
engines-notice-list-full = A lista de engines está cheia.
engines-notice-not-found = A engine não está na lista.
engines-notice-entry-invalid = A engine não pode ser guardada como está.
engines-notice-restart-to-apply = Reinicie a engine para aplicar a mudança.
engines-notice-many-running = { $count } engines estão em execução.
engines-notice-command-refused = A engine recusou o comando.
engines-notice-imported = { $added ->
    [one] Importada uma engine
   *[other] Importadas { $added } engines
}; { $duplicates ->
    [one] ignorada uma repetida
   *[other] ignoradas { $duplicates } repetidas
} e { $invalid ->
    [one] uma inutilizável
   *[other] { $invalid } inutilizáveis
}.
engines-notice-nothing-to-import = O arquivo não tem engine para importar.

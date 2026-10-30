# Jogo online no Lichess (spec M8-3, fase c). Sem equivalente no Scid.

menu-online = Online
menu-online-connect = Conectar ao Lichess...
menu-online-seek = Buscar partida...
menu-online-challenge = Desafiar um usuário...
menu-online-incoming = Desafios recebidos ({ $number })...
menu-online-disconnect = Sair da conta
menu-online-fics = FICS...
menu-online-remove-lock = Remover o bloqueio de assistência...

action-online-connect = Conectar ao Lichess
action-online-seek = Buscar partida no Lichess

# Janela de conexão
online-connect-title = Conectar ao Lichess
online-connect-steps = 1. Copie o link abaixo e abra no navegador para criar um token pessoal com os escopos "Jogar partidas com a API de tabuleiro", "Ler desafios recebidos" e "Criar, aceitar e recusar desafios".
    2. Cole o token aqui. Ele nunca é exibido, registrado em log nem gravado nas preferências.
online-connect-copy-link = Copiar o link para criar o token
online-connect-token-placeholder = Token de acesso pessoal
online-connect-store = Onde guardar o token:
online-store-keyring = No chaveiro do sistema (recomendado)
online-store-file = Em um arquivo que só você lê (concordo em guardá-lo no disco)
online-store-session = Não guardar (só nesta sessão)
online-connect-fairplay = Enquanto você joga, engines, árvore de aberturas, tablebases e buscas por posição ficam desativadas, como exigem as regras de jogo limpo do Lichess.
online-connect-connecting = Conectando...
online-connect-saved = Usar o token guardado ({ $login })
online-connect-button = Conectar

# Janelas
online-minutes = Minutos
online-increment = Incremento (s)
online-rated = Partida valendo rating
online-assist-warning = Quando a partida começar, engines e consultas ficam desativadas até ela terminar.
online-close = Fechar
online-no = Não
online-yes = Sim

online-seek-title = Buscar partida
online-seek-waiting = Procurando oponente...
online-seek-cancel = Parar de procurar
online-seek-invalid = O Lichess não aceita este tempo de jogo em buscas (rápida ou mais lenta).
online-seek-button = Buscar

online-challenge-title = Desafiar um usuário
online-challenge-user = Nome do usuário no Lichess
online-challenge-waiting = Aguardando a resposta de { $user }...
online-challenge-cancel = Retirar o desafio
online-challenge-invalid = Informe um nome de usuário válido e um tempo de blitz ou mais lento.
online-challenge-button = Desafiar
online-color-random = Sorteio
online-color-white = Brancas
online-color-black = Pretas
online-variant = Variante
online-variant-standard = Padrão
online-variant-chess960 = Chess960
online-challenge-start = Posição (0-959)
online-challenge-start-hint = sorteio
online-challenge-start-invalid = Digite uma posição de 0 a 959 (vazio: o Lichess sorteia). Posição fixa exige partida amistosa.

online-incoming-title = Desafios recebidos
online-incoming-empty = Nenhum desafio.
online-incoming-rated = { $who }: { $time }, valendo rating
online-incoming-casual = { $who }: { $time }, informal
online-incoming-unplayable = O MyChess não joga este desafio (variante ou tempo).
online-incoming-chess960 = Variante Chess960.
online-incoming-accept = Aceitar
online-incoming-decline = Recusar

online-confirm-resign-title = Desistir?
online-confirm-resign = Deseja desistir desta partida?
online-confirm-unlock-title = Remover o bloqueio de assistência?
online-confirm-unlock = Entendo que a partida pode continuar no servidor e que não devo usar assistência nela.
online-confirm-quit-title = Há uma partida em andamento
online-confirm-quit = Fechar o programa não encerra a partida: o relógio continua correndo no servidor. Fechar mesmo assim?

# Painel sob o tabuleiro
online-status-your-turn = Sua vez
online-status-opponent-turn = Vez do oponente
online-status-reconnecting = Reconectando...
online-status-over = Partida encerrada
online-versus = contra { $name }
online-info-rated = Valendo rating { $time }
online-info-casual = Informal { $time }
online-abort = Abortar
online-abort-tip = Aborta a partida antes que ela comece de fato
online-resign = Desistir
online-resign-tip = Abandona a partida
online-offer-draw = Oferecer empate
online-offer-draw-tip = Oferece empate ao oponente
online-draw-accept = Aceitar empate
online-draw-accept-tip = Aceita o empate oferecido pelo oponente
online-draw-decline = Recusar
online-draw-decline-tip = Recusa o empate oferecido pelo oponente
online-takeback-accept = Aceitar volta
online-takeback-accept-tip = Deixa o oponente voltar o último lance
online-takeback-decline = Recusar volta
online-takeback-decline-tip = Recusa o pedido de voltar o lance
online-claim = Reivindicar vitória
online-claim-tip = Vence a partida: o oponente saiu
online-gone-waiting = O oponente saiu. Você poderá reivindicar a vitória em { $secs } s.
online-gone-claimable = O oponente saiu. Você já pode reivindicar a vitória.
online-restore = Restaurar a partida anterior
online-restore-tip = Devolve ao editor a partida que estava nele antes
online-close-tip = Sair desta partida
online-premove-set = Pré-lance marcado (Esc cancela)
online-link-up = Conectado
online-link-reconnecting = Reconectando
online-link-down = Sem conexão
online-ping = { $ms } ms
online-assist-off = Partida online: assistência desativada (engines, árvore de aberturas, tablebases e buscas)
online-remove-lock = Remover bloqueio...

# Fim da partida
online-end-line = { $outcome } { $reason }
online-end-no-result = A partida terminou sem resultado.
online-end-mate = Por xeque-mate.
online-end-resign = Por desistência.
online-end-stalemate = Por afogamento.
online-end-timeout = Por tempo.
online-end-draw = Por acordo ou pelas regras.
online-end-insufficient = Por material insuficiente.
online-end-cheat = Infração das regras.
online-end-variant = Pelas regras da variante.
online-end-aborted = A partida foi abortada.
online-end-unknown = O servidor não disse o motivo.

# Motivos para recusar um desafio
online-decline-generic = O usuário não quer jogar agora.
online-decline-later = O usuário pede para jogar mais tarde.
online-decline-too-fast = O tempo de jogo é rápido demais para o usuário.
online-decline-too-slow = O tempo de jogo é lento demais para o usuário.
online-decline-time-control = O usuário não aceita este tempo de jogo.
online-decline-rated = O usuário só joga partidas valendo rating.
online-decline-casual = O usuário só joga partidas informais.
online-decline-standard = O usuário só joga xadrez padrão.
online-decline-variant = O usuário não joga esta variante.
online-decline-no-bot = O usuário não joga contra bots.
online-decline-only-bot = O usuário só joga contra bots.

# Avisos da barra de estado
online-notice-rejected-wrong-state = Isso não é possível no estado atual da conexão.
online-notice-rejected-not-connected = Conecte-se ao Lichess primeiro.
online-notice-rejected-busy = Termine antes a partida contra a engine, o torneio ou a anotação.
online-notice-rejected-game-in-progress = Há uma partida em andamento.
online-notice-rejected-events-not-ready = A conexão ainda não está pronta; tente de novo em instantes.
online-notice-rejected-time-control = O Lichess não aceita este tempo de jogo.
online-notice-rejected-not-your-turn = Não é a sua vez.
online-notice-rejected-no-game = Não há partida em andamento.
online-notice-rejected-illegal-move = Este lance não pode ser feito.
online-notice-rejected-challenge = Este desafio não pode ser jogado.
online-notice-rejected-not-available = Isso não está disponível agora.
online-notice-rejected-bad-user = Este não é um nome de usuário válido do Lichess.
online-notice-rejected-move-pending = Espere o servidor confirmar o seu último lance.
online-notice-assistance-disabled = Desativado durante a partida online.
online-notice-assistance-will-be-disabled = Quando a partida começar, engines e consultas serão desativadas.
online-notice-connected = Conectado ao Lichess.
online-notice-signed-out = Você saiu da conta. Para revogar o token, acesse lichess.org/account/oauth/token.
online-notice-disconnected = A conexão com o Lichess não voltou.
online-notice-another-game = Há outra partida em andamento em outro lugar; continue-a no navegador.
online-notice-resumed = Uma partida em andamento foi retomada.
online-notice-unsupported-variant = O MyChess joga online só xadrez padrão; continue esta partida no navegador.
online-notice-move-rejected = O servidor recusou o seu lance: { $reason }
online-notice-move-uncertain = O servidor pode não ter recebido o seu último lance; o tabuleiro segue o servidor.
online-notice-resynchronised = A partida foi sincronizada de novo com o servidor.
online-notice-desynced = A partida na tela não consegue mais acompanhar o servidor; continue no navegador.
online-notice-game-lost = Não foi possível acompanhar a partida e ela foi encerrada aqui; ela pode continuar no Lichess. Continue no navegador.
online-notice-search-timed-out = Nenhuma partida começou; a busca terminou.
online-notice-challenge-declined = Desafio recusado.
online-notice-challenge-declined-reason = Desafio recusado: { $reason }
online-notice-imported = A partida foi guardada na Clipbase.
online-notice-already-imported = Esta partida já está guardada.
online-notice-not-imported = A partida terminou sem lances e não foi guardada.
online-notice-import-failed = Não foi possível guardar a partida.
online-notice-lock-released = O bloqueio de assistência foi removido.
online-notice-marker-cleared = A partida que estava em andamento terminou; o bloqueio acabou.
online-notice-previous-restored = A partida anterior voltou ao editor.
online-notice-game-over = A partida terminou.

# Falhas
online-error-unauthorized = O Lichess recusou o token. Ele pode ter expirado ou sido revogado.
online-error-scope-missing = Falta uma permissão ao token. Crie-o de novo com os escopos do link.
online-error-rate-limited = Requisições demais. Aguarde { $secs } s.
online-error-network = Sem conexão com o Lichess.
online-error-timeout = O Lichess não respondeu a tempo.
online-error-not-found = Não encontrado no Lichess.
online-error-rejected = O Lichess recusou: { $reason }
online-error-protocol = O Lichess enviou algo que não se entende.
online-error-bot-account = Esta é uma conta Bot. O MyChess joga só com contas humanas.
online-error-insecure-url = O endereço do servidor não é seguro para um token. Só https é aceito.
online-error-no-token = Não há token guardado. Cole um.
online-error-bad-token = Isto não parece um token do Lichess.
online-error-token-store = Não foi possível guardar o token. Escolha outro lugar e conecte de novo.
online-error-time-control = O Lichess não aceita este tempo de jogo.
online-error-server = O Lichess está com problemas. Tente mais tarde.
online-error-other = Algo deu errado com o Lichess.

menu-online-watch = Observar uma partida...
menu-online-stop-watch = Parar de observar

online-oauth-button = Entrar pelo navegador
online-oauth-hint = Abre o lichess.org para você autorizar o MyChess lá; sem token para copiar.
online-oauth-waiting = Aguardando você autorizar o MyChess no navegador...
online-oauth-not-opened = O programa não conseguiu abrir o navegador. Copie o link e abra você mesmo.
online-oauth-copy = Copiar o link de autorização
online-oauth-cancel = Cancelar o login
online-error-login-failed = O login pelo navegador não foi concluído.

online-watch-title = Observar uma partida
online-watch-help = Cole o link de uma partida do Lichess ou o id de 8 caracteres.
online-watch-placeholder = https://lichess.org/codigo1234
online-watch-delay = Partidas em andamento chegam atrasadas 3 lances.
online-watch-button = Observar
online-watch-names = { $white } contra { $black }
online-watch-live = Observando. Partidas em andamento chegam atrasadas 3 lances.
online-watch-over = A partida terminou.
online-watch-incomplete = Observando. Os lances anteriores a esta posição não estão disponíveis.
online-watch-stop = Parar de observar
online-watch-stop-tip = Fecha a partida e traz de volta a que você tinha

online-promotion-title = Promover o peão para
online-promotion-queen = Dama
online-promotion-rook = Torre
online-promotion-bishop = Bispo
online-promotion-knight = Cavalo

online-takeback-propose = Pedir voltar o lance
online-takeback-propose-tip = Pede ao oponente para desfazer seu último lance
online-claim-draw = Reivindicar empate
online-claim-draw-tip = Pede o empate: o oponente saiu
online-rematch = Revanche
online-rematch-tip = Desafia o mesmo oponente de novo, com as cores trocadas

online-notice-rejected-bad-game-ref = Isso não é um link nem um id de partida do Lichess.
online-notice-rejected-own-game = Você não pode observar a partida em que está jogando.
online-notice-rejected-bad-variant = A posição Chess960 deve ser um número de 0 a 959, num desafio amistoso (a busca não permite fixá-la).
online-notice-watching = Abrindo a partida para observar...
online-notice-watch-ended = A partida observada terminou.
online-notice-watch-incomplete = Os lances anteriores a esta posição não estão disponíveis.
online-notice-watch-steps-dropped = Os lances chegaram mais depressa do que o histórico pôde ser carregado e alguns foram ignorados.
online-notice-browser-not-opened = Não foi possível abrir o navegador: copie o link da janela de login.
online-notice-rematch-sent = Revanche proposta. Aguardando a resposta.

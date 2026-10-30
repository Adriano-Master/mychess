# As ferramentas de ECO (spec M5-7b): o navegador ECO, a classificação de uma
# base, "Identificar abertura" e "Encontrar novidade". Os equivalentes dos
# textos do Scid estão em `english.tcl` (WindowsECO, ToolsMaintClass,
# GameDeepest, GameNovelty, Classify*, ECO*, Novelty*).

menu-game-identify-opening = Identificar abertura
menu-game-novelty = Encontrar novidade...
menu-tools-eco-browser = Navegador ECO...
menu-tools-eco-classify = Classificar partidas por ECO...

# A tradução dos nomes das aberturas: um `de => para` por linha. Como no
# Scid, os pares agem do último ao primeiro: as palavras vêm de baixo para
# cima, e as correções de ordem (`da Dama Gambito => Gambito da Dama`, que
# casam com o texto já traduzido) ficam no começo, para agir por último. Uma
# palavra que é parte de outra (`Open`, `Opening`) vem antes dela.
eco-glossary =
    do Rei Gambito => Gambito do Rei
    da Dama Gambito => Gambito da Dama
    do Rei Índia => Índia do Rei
    da Dama Índia => Índia da Dama
    Siciliana Defesa => Defesa Siciliana
    Francesa Defesa => Defesa Francesa
    Moderna Defesa => Defesa Moderna
    Holandesa Defesa => Defesa Holandesa
    Gambit => Gambito
    Defence => Defesa
    Defense => Defesa
    Attack => Ataque
    Variation => Variante
    Line => Linha
    Open => Aberta
    Opening => Abertura
    Closed => Fechada
    Declined => Recusado
    Accepted => Aceito
    Exchange => Troca
    Classical => Clássica
    Modern => Moderna
    Old => Antiga
    Main => Principal
    Symmetrical => Simétrica
    Orthodox => Ortodoxa
    Sicilian => Siciliana
    Spanish => Espanhola
    French => Francesa
    English => Inglesa
    Dutch => Holandesa
    Indian => Índia
    Knights => Cavalos
    Pawn => Peão
    King's => do Rei
    Queen's => da Dama
    Four => Quatro
    Two => Dois

eco-close = Fechar
eco-stop = Parar

eco-browser-title = Navegador ECO
eco-browser-all = Todas as seções do ECO
eco-browser-section = Seção do ECO { $code }
eco-browser-code = Código ECO { $code }
eco-browser-summary = { $games } partidas: +{ $white } ={ $draw } -{ $black } ({ $whole },{ $tenth }%)
eco-browser-counting = Contando as partidas...
eco-browser-base = Base { $name } ({ $classified } de { $total } partidas classificadas)
eco-browser-no-bars = Nada abaixo deste código: estão listadas as aberturas do arquivo ECO.
eco-browser-up = Subir
eco-browser-search = Procurar
eco-browser-search-placeholder = Procurar uma abertura pelo nome ou pelo código
eco-browser-scope-all = Todas as partidas
eco-browser-scope-filter = Só o filtro
eco-browser-no-book = Nenhum arquivo ECO está carregado.
eco-browser-load-file = Carregar arquivo ECO...
eco-browser-no-entries = O arquivo ECO não tem abertura aqui.
eco-browser-truncated = Só as primeiras { $count } aberturas são mostradas.
eco-browser-list-all = Aberturas do arquivo ECO (clique numa para importar a linha)
eco-browser-list-node = Aberturas deste código (clique numa para importar a linha)
eco-browser-found = Aberturas que contêm "{ $text }" (clique numa para importar a linha)

eco-classify-title = Classificar partidas por ECO
eco-classify-base = Base { $name } ({ $games } partidas)
eco-classify-which = Partidas a classificar
eco-classify-new = Só as partidas que ainda não têm código ECO
eco-classify-all = Todas as partidas (substitui os códigos antigos)
eco-classify-filter = Só as partidas do filtro ({ $count })
eco-classify-year = Todas as partidas jogadas no último ano
eco-classify-month = Todas as partidas jogadas no último mês
eco-classify-since = Partidas jogadas desde
eco-classify-since-placeholder = AAAA.MM.DD
eco-classify-invalid-date = Digite uma data que comece pelo ano, como 2020.01.31.
eco-classify-codes = Códigos ECO a usar
eco-classify-basic = Só códigos básicos ("B12")
eco-classify-extended = Extensões ("B12j")
eco-classify-preview = Prévia
eco-classify-run = Classificar
eco-classify-no-book = Nenhum arquivo ECO está carregado. Carregue um no navegador ECO antes.
eco-classify-read-only = Esta base não pode ser gravada: só a prévia está disponível.
eco-classify-progress = Classificando: { $percent }%
eco-classify-preview-done = { $classified } partidas examinadas: { $changed } mudariam.
eco-classify-done = { $classified } partidas classificadas: { $changed } mudaram, { $written } gravadas.
eco-classify-stopped = Interrompida: { $classified } partidas examinadas, { $written } gravadas.
eco-classify-unreadable = { $count } partidas não puderam ser lidas.
eco-classify-error = A classificação parou: { $reason }

eco-novelty-title = Encontrar novidade
eco-novelty-help = Encontra o primeiro lance desta partida que nenhuma outra partida da base escolhida jogou.
eco-novelty-database = Base
eco-novelty-older = Só partidas mais antigas (antes de { $date })
eco-novelty-find = Encontrar novidade
eco-novelty-searching = Procurando: { $percent }%
eco-novelty-found = Novidade: { $move }
eco-novelty-none = Nenhuma novidade foi encontrada nesta partida.
eco-novelty-stopped = Busca de novidade interrompida.
eco-novelty-non-standard = Esta partida começa de uma posição própria: não há busca de novidade.
eco-novelty-failed = A busca falhou: uma partida da base não pode ser lida.
eco-novelty-empty = A partida não tem lances.

eco-notice-no-book = Nenhum arquivo ECO está carregado.
eco-notice-opening = Abertura { $eco }: { $name } (depois do meio-lance { $ply })
eco-notice-no-opening = Nenhuma posição desta partida está no arquivo ECO.
eco-notice-preview = Prévia do ECO: { $classified } partidas examinadas, { $changed } mudariam.
eco-notice-classified = Classificação ECO: { $classified } partidas examinadas, { $changed } mudaram, { $written } gravadas.
eco-notice-classify-stopped = Classificação ECO interrompida: { $classified } partidas examinadas, { $written } gravadas.
eco-notice-read-only = Esta base não pode ser gravada: só a prévia é possível.
eco-notice-novelty = Novidade: { $move }
eco-notice-no-novelty = Nenhuma novidade foi encontrada nesta partida.
eco-notice-novelty-stopped = Busca de novidade interrompida.
eco-notice-novelty-non-standard = Esta partida começa de uma posição própria: não há busca de novidade.
eco-notice-novelty-failed = A busca de novidade falhou: uma partida da base não pode ser lida.
eco-notice-line-failed = Os lances dessa abertura não puderam ser jogados.


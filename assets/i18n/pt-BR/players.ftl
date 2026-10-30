# Faixas de jogador acima e abaixo do tabuleiro (spec M2-5, D230 a D239).

# O nome de um jogador que a partida não informa; a dica explica.
player-name-unknown = ?
player-unknown = Jogador desconhecido
# $title é o título (GM, IM...), $rating o rating com o tipo, ambos
# opcionais ($has_title e $has_rating valem "yes" ou "no").
player-tooltip = { $has_title ->
    [yes] { $title }{" "}
    *[no] {""}
}{ $name }{ $has_rating ->
    [yes] , { $rating }
    *[no] {""}
}
# Os pontos de vantagem ao lado das peças capturadas.
player-advantage-short = +{ $points }
player-advantage = { $points ->
    [one] À frente por 1 ponto
    *[other] À frente por { $points } pontos
}
player-clock = Relógio: { $time }
# Mostrado quando a partida tem relógios mas este lado ainda não tem.
player-clock-none = --:--

# Nomes curtos dos tipos de rating que não são o Elo simples.
player-rating-kind-rapid = Rápido
player-rating-kind-uscf = USCF
player-rating-kind-dwz = DWZ
player-rating-kind-ecf = ECF
player-rating-kind-iccf = ICCF

# Menu Visualizar.
menu-view-player-strips = Faixas de jogador
menu-view-player-strips-show = Mostrar as faixas
menu-view-player-strips-compact = Faixas compactas
menu-view-player-avatars = Avatares
menu-view-player-ratings = Ratings
menu-view-player-clocks = Relógios

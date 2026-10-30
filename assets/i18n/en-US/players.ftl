# Player strips above and below the board (spec M2-5, D230 to D239).

# The name of a player the game does not name; the tooltip explains it.
player-name-unknown = ?
player-unknown = Unknown player
# $title is the title (GM, IM...), $rating the rating with its kind, both
# optional ($has_title and $has_rating are "yes" or "no").
player-tooltip = { $has_title ->
    [yes] { $title }{" "}
    *[no] {""}
}{ $name }{ $has_rating ->
    [yes] , { $rating }
    *[no] {""}
}
# The points ahead next to the captured pieces.
player-advantage-short = +{ $points }
player-advantage = { $points ->
    [one] Ahead by 1 point
    *[other] Ahead by { $points } points
}
player-clock = Clock: { $time }
# Shown when the game has clocks but this side has none yet.
player-clock-none = --:--

# Short names of the rating kinds that are not the plain Elo.
player-rating-kind-rapid = Rapid
player-rating-kind-uscf = USCF
player-rating-kind-dwz = DWZ
player-rating-kind-ecf = ECF
player-rating-kind-iccf = ICCF

# View menu.
menu-view-player-strips = Player strips
menu-view-player-strips-show = Show strips
menu-view-player-strips-compact = Compact strips
menu-view-player-avatars = Avatars
menu-view-player-ratings = Ratings
menu-view-player-clocks = Clocks

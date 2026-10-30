# Players, tournaments and the spelling file (spec M6-3, slice c).
menu-group-lichess = Lichess
menu-group-fics = FICS
menu-group-players = Players
menu-stats-find-players = Player finder...
menu-stats-find-tournaments = Tournament finder...
menu-stats-player-info = Player information...
menu-stats-spell = Spell-check names
menu-stats-spell-player = Player names...
menu-stats-spell-event = Event names...
menu-stats-spell-site = Site names...
menu-stats-spell-round = Round names...
menu-stats-add-elo = Add Elo ratings...
menu-file-load-spelling = Load spelling file...
menu-file-reload-spelling = Reload spelling file
menu-file-unload-spelling = Unload spelling file
menu-file-cancel-spelling-load = Cancel loading the spelling file
action-player-finder = Show or hide the player finder
action-tournament-finder = Show or hide the tournament finder
action-player-info = Player information
file-dialog-open-spelling = Open spelling file
file-filter-spelling = Spelling files (.ssp, .ssp.gz)

# Common bits of the windows.
ptools-close = Close
ptools-stop = Stop
ptools-defaults = Defaults
ptools-update = Update
ptools-progress = { $done } of { $total }
ptools-from-to = to

# Player finder (Scid: "Player Finder").
pfinder-title = Player finder
pfinder-title-file = Player finder: { $file }
pfinder-title-count = { $count ->
    [one] { $file }, 1 player
   *[other] { $file }, { $count } players
}
pfinder-col-rank = #
pfinder-col-games = Games
pfinder-col-oldest = Oldest
pfinder-col-newest = Newest
pfinder-col-country = Country
pfinder-col-elo = Elo
pfinder-col-name = Name
pfinder-games = Games
pfinder-player = Player
pfinder-elo = Elo
pfinder-list-size = List size
pfinder-country = Country
pfinder-country-tip = no: no column. yes: the country of everybody. Or the start of a code, like BRA.
pfinder-elo-tip = The highest rating in the database. Some come from the spelling file (estimated).
pfinder-building = Reading the database
pfinder-no-base = Open a database to list its players.
pfinder-empty = No player matches.
pfinder-shown = { $count ->
    [one] 1 player listed
   *[other] { $count } players listed
}
finder-no-base = There is no database to list.
finder-stale = The database changed while the list was made; ask again.
finder-failed = The list could not be made.
tmt-filtered = { $games ->
    [one] The filter holds the 1 game of the tournament.
   *[other] The filter holds the { $games } games of the tournament.
}

# Tournament finder (Scid: "Tournaments").
tmt-title = Tournaments
tmt-title-file = Tournaments: { $file }
tmt-title-player = Tournaments: { $player } - { $file }
tmt-col-rank = #
tmt-col-date = Date
tmt-col-players = Players
tmt-col-games = Games
tmt-col-elo = Elo
tmt-col-event = Event
tmt-col-site = Site
tmt-col-winner = Winner
tmt-col-second = Runner-up
tmt-games = Games
tmt-players = Players
tmt-mean-elo = Mean Elo
tmt-date = Date
tmt-event = Event
tmt-site = Site
tmt-player = Player
tmt-country = Country
tmt-list-size = List size
tmt-none = No matching tournaments were found.
tmt-no-base = Open a database to search its tournaments.
tmt-running = Searching the tournaments
tmt-crosstable = Crosstable
tmt-show-in-filter = Show in filter
tmt-load-first = Load first game
tmt-select-tip = Click a tournament, then choose what to do with it. A double click opens its crosstable.
tmt-shown = { $count ->
    [one] 1 tournament
   *[other] { $count } tournaments
}

# Player information (Scid: "Player Information").
pinfo-title = Player information
pinfo-title-name = Player information: { $name }
pinfo-empty = Choose a player in the player finder or in the game information.
pinfo-no-base = Open a database to see the statistics of a player.
pinfo-working = Counting the games
pinfo-stale = Updating...
pinfo-no-games = 0 games
pinfo-not-in-base = This name is not in the database.
pinfo-no-spelling = No spelling file is loaded, so there is no title, country or biography.
pinfo-approximate = Approximate data: the name only starts like the player's.
pinfo-candidates = Possible matches: { $names }
pinfo-title-label = Title
pinfo-country-label = Country
pinfo-born = Born
pinfo-peak = Peak rating
pinfo-peak-estimate = { $peak } (estimated)
pinfo-bio = Biography
pinfo-fide = FIDE profile
pinfo-games = { $count ->
    [one] 1 game
   *[other] { $count } games
}
pinfo-dates = { $first }  to  { $last }
pinfo-filter-games = { $count ->
    [one] Filter: 1 game
   *[other] Filter: { $count } games
}
pinfo-legend = Won / Lost / Drawn
pinfo-won = Won
pinfo-lost = Lost
pinfo-drawn = Drawn
pinfo-all = All games
pinfo-filter = Filter games
pinfo-against = Against { $name }
pinfo-total = Total
pinfo-white = White
pinfo-black = Black
pinfo-openings-white = Most common openings as White
pinfo-openings-black = Most common openings as Black
pinfo-rating-history = Rating history
pinfo-lookup = Lookup name
pinfo-tournaments = Tournaments
pinfo-edit-ratings = Edit ratings
pinfo-edit-name = Edit name
pinfo-later-tip = Available with the name editor, in a later version.
pinfo-recent = Recent players
pinfo-no-player = There is no player to look up.
pinfo-bad-link = That link is not one of the window's.
pinfo-no-fide = The notes of this player have no FIDE profile.
pinfo-fide-failed = The web browser could not be started.
pinfo-filter-set = { $games ->
    [one] The filter holds 1 game.
   *[other] The filter holds { $games } games.
}

# Spelling file and the name checker (Scid: "Spellcheck").
spell-title = Spell-check
spell-title-kind = Spell-check: { $kind }
spell-kind-player = player names
spell-kind-event = event names
spell-kind-site = site names
spell-kind-round = round names
spell-col-apply = Apply
spell-col-old = Name in the database
spell-col-new = Correct name
spell-col-games = Games
spell-col-life = Life
spell-ambiguous = Ambiguous
spell-ambiguous-tip = Also show names that more than one correct name matches. They are never ticked.
spell-surnames = Surnames
spell-surnames-tip = Also look at names that are only a surname.
spell-select-all = Tick all
spell-select-none = Untick all
spell-find-placeholder = Search the list
spell-copy-text = Copy as text
spell-rescan = Scan again
spell-apply = Make corrections
spell-ambiguous-mark = ambiguous
spell-found = { $count ->
    [one] 1 correction found ({ $kind }).
   *[other] { $count } corrections found ({ $kind }).
}
spell-ticked = { $ticked } of { $total } ticked
spell-scanning = Looking for corrections
spell-writing = Writing the corrections
spell-empty = No corrections were found.
spell-partial = The scan was stopped: the list is partial.
spell-read-only = This database cannot be changed.
spell-confirm = Please confirm making the corrections. This cannot be undone.
spell-confirm-yes = Make corrections
spell-loaded = Spelling file loaded: { $players } players, { $events } events, { $sites } sites, { $rounds } rounds.
spell-read-error = The spelling file could not be read: { $reason }
spell-error-not-a-file = it is not a file
spell-error-too-large = it is too large
spell-error-gzip = the compressed data is damaged
spell-error-unclosed-quote = a section header on line { $line } has an unclosed quote
spell-error-cancelled = reading was cancelled
spell-not-loaded = No spelling file is loaded. You can load one from the File menu.
spell-no-elo = The spelling file has no Elo ratings (%Elo lines). Load a file that has them.
spell-no-base = There is no database to work on.
spell-none-valid = No valid corrections were found.
spell-interrupted = Stopped before the end.
spell-done = { $names ->
    [one] 1 name ({ $kind }) occurring { $times } times in total was corrected.
   *[other] { $names } names ({ $kind }) occurring { $times } times in total were corrected.
}
spell-done-skipped = { $count ->
    [one] 1 player name change was ignored because of the age of the player or the date of the game.
   *[other] { $count } player name changes were ignored because of the age of the player or the date of the game.
}
spell-not-found = { $count ->
    [one] 1 name was not in the database any more.
   *[other] { $count } names were not in the database any more.
}
spell-game-not-refreshed = The tags of the game on screen still show the old names; load the game again.

# Add Elo (Scid: "Add Elo ratings").
spell-add-elo = Add Elo ratings
spell-elo-description = Adds the Elo of the players to the games, using the information of the spelling file (ratings).
spell-elo-overwrite = Overwrite existing ratings
spell-elo-scope = Add ratings to
spell-elo-all = All games
spell-elo-filter = Games of the filter
spell-elo-plan = Check
spell-elo-planning = Working out what would change
spell-elo-planned = { $games ->
    [one] { $ratings } ratings would be added to 1 game.
   *[other] { $ratings } ratings would be added to { $games } games.
}
spell-elo-add = Add ratings
spell-elo-writing = Writing the ratings
spell-elo-done = { $games ->
    [one] { $ratings } Elo ratings were added to 1 game.
   *[other] { $ratings } Elo ratings were added to { $games } games.
}
spell-elo-nothing = There is nothing to add.
menu-group-statistics = Statistics and graphs

# Player report (spec M7-3): notices of the application, the dialog and the
# words of the exported documents. The labels `preport-label-*` are filled by
# `mychess_app::player_report::player_labels`; `{ $n }` receives the literal
# text `{n}` (the name or the moves), replaced when a document is written.
# The words shared with the opening report are the `oprep-label-*`.

preport-title = Player Report
menu-tools-player-report = Player Report...
preport-dialog-title = Generate Player Report
preport-player = Player:
preport-color = Color:
preport-color-white = with the White pieces
preport-color-black = with the Black pieces
preport-start = Start position:
preport-start-beginning = Game start
preport-start-position = Current position
preport-beginning = Beginning with
preport-after = after { $moves }
preport-openings = Openings
preport-clipbase = Empty the Clipbase and copy the games found to it
preport-clipbase-confirm = The Clipbase will be emptied. Continue?
preport-overwrite-confirm = Replace the file “{ $name }”?
preport-change = Change...
preport-matching = Matching:
preport-matching-exact = Exact
preport-matching-prefix = Starts with
preport-matching-contains = Contains
preport-none = No games of { $name }
preport-pov = from the point of view of { $name }
preport-no-games = No games in the current database

preport-err-unavailable = The database is being reloaded; try again in a moment
preport-err-name = Type the name of a player
preport-err-no-report = There is no player report to use
preport-stale = The report is out of date
preport-err-busy = Another operation of the report is in progress
preport-err-failed = The player report could not be made
preport-err-position = The position has too many pieces for the search
preport-err-line = The moves of the report are not legal
preport-partial = The report was interrupted
preport-filter-set = The filter now holds the chosen games
preport-saved = Report saved
preport-err-save = The report could not be saved
preport-err-save-base = That file is an open database; choose another name
preport-err-view = The browser could not be started
preport-err-clipbase = The Clipbase cannot be emptied to receive its own games
preport-err-no-name = The game on the board has no player name

preport-label-player = Player
preport-label-with-white = with the White pieces
preport-label-with-black = with the Black pieces
preport-label-after = after { $n }
preport-label-openings = Openings
preport-label-most-frequent-opponents = Most frequent opponents
preport-label-wins = Wins
preport-label-draws = Draws
preport-label-losses = Losses
preport-label-pov = from the point of view of { $n }; the "All games" lines count the whole database, not only the games of { $n }
preport-label-none = No games of { $n }
preport-label-player-rating = Player rating
preport-label-opponents-rating = Opponents rating
preport-label-player-perf = Player performance
preport-label-opponents-perf = Opponents performance

# The window and the dialog (slice c, M7-3c).
preport-ui-recent = Recent players:
preport-ui-white-of = White: { $name }
preport-ui-black-of = Black: { $name }
preport-ui-close = Close
preport-ui-update = Update
preport-ui-save = Save
preport-ui-save-title = Save the player report
preport-ui-save-choose = Save as...
preport-ui-save-dialog = Save the player report
preport-ui-view-html = View HTML
preport-ui-stop = Stop
preport-ui-progress = Making the player report
preport-ui-part = Part of the report
preport-ui-format = Format
preport-ui-part-full = Full report
preport-ui-part-compact = Compact report
preport-ui-part-table = Theory table only
preport-ui-fmt-text = Text
preport-ui-fmt-html = HTML
preport-ui-fmt-latex = LaTeX
preport-ui-score = { $name } scored +{ $wins } ={ $draws } -{ $losses } ({ $percent }%) in { $games } games
preport-ui-report-line = { $games } games in { $database }
preport-ui-opponents-hint = Click an opponent to see the player information
preport-ui-opponent-row = { $games } games, { $percent }%
preport-ui-opponent-row-year = { $games } games, { $percent }% ({ $first })
preport-ui-opponent-row-years = { $games } games, { $percent }% ({ $first }-{ $last })
preport-ui-rating-line = { $rating }: { $average } ({ $games } games); { $perf }: { $performance } ({ $percent }% vs { $against })
preport-ui-game-elo = ({ $elo })
preport-ui-game-ref = #{ $n }
preport-ui-eco-row = { $games } games, { $percent }%
preport-ui-games = Games of the report
preport-ui-games-shown = The first { $shown } of { $total } games are listed
preport-ui-theory = Theory table
preport-ui-theory-row = { $count } games, { $percent }%
preport-ui-themes = Positional themes
preport-ui-endgames = Endgames
preport-ui-all = All the games
preport-ui-info = Player report
preport-ui-info-tip = Make a report of the games of this player
preport-ui-white = White
preport-ui-black = Black

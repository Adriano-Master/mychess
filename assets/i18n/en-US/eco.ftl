# The ECO tools (spec M5-7b): the ECO browser, the classification of a
# database, "Identify opening" and "Find novelty". The equivalents of the
# texts of Scid are in `english.tcl` (WindowsECO, ToolsMaintClass, GameDeepest,
# GameNovelty, Classify*, ECO*, Novelty*).

menu-game-identify-opening = Identify opening
menu-game-novelty = Find novelty...
menu-tools-eco-browser = ECO browser...
menu-tools-eco-classify = ECO-classify games...

# The translation of the names of the openings: one `from => to` per line.
# Empty here: the names stay as the ECO file writes them.
eco-glossary = { "" }

eco-close = Close
eco-stop = Stop

eco-browser-title = ECO browser
eco-browser-all = All ECO sections
eco-browser-section = ECO section { $code }
eco-browser-code = ECO code { $code }
eco-browser-summary = { $games } games: +{ $white } ={ $draw } -{ $black } ({ $whole }.{ $tenth }%)
eco-browser-counting = Counting the games...
eco-browser-base = Database { $name } ({ $classified } of { $total } games classified)
eco-browser-no-bars = Nothing below this code: the openings of the ECO file are listed.
eco-browser-up = Up
eco-browser-search = Find
eco-browser-search-placeholder = Find an opening by name or code
eco-browser-scope-all = All games
eco-browser-scope-filter = Only the filter
eco-browser-no-book = No ECO file is loaded.
eco-browser-load-file = Load ECO file...
eco-browser-no-entries = The ECO file has no opening here.
eco-browser-truncated = Only the first { $count } openings are shown.
eco-browser-list-all = Openings of the ECO file (click one to import its line)
eco-browser-list-node = Openings of this code (click one to import its line)
eco-browser-found = Openings that contain "{ $text }" (click one to import its line)

eco-classify-title = ECO-classify games
eco-classify-base = Database { $name } ({ $games } games)
eco-classify-which = Games to classify
eco-classify-new = Only games with no ECO code yet
eco-classify-all = All games (overwrite old ECO codes)
eco-classify-filter = Only the games of the filter ({ $count })
eco-classify-year = All games played in the last year
eco-classify-month = All games played in the last month
eco-classify-since = Games played since
eco-classify-since-placeholder = YYYY.MM.DD
eco-classify-invalid-date = Type a date that starts with the year, as 2020.01.31.
eco-classify-codes = ECO codes to use
eco-classify-basic = Basic codes only ("B12")
eco-classify-extended = Extensions ("B12j")
eco-classify-preview = Preview
eco-classify-run = Classify
eco-classify-no-book = No ECO file is loaded. Load one in the ECO browser first.
eco-classify-read-only = This database cannot be written: only the preview is available.
eco-classify-progress = Classifying: { $percent }%
eco-classify-preview-done = { $classified } games examined: { $changed } would change.
eco-classify-done = { $classified } games classified: { $changed } changed, { $written } written.
eco-classify-stopped = Stopped: { $classified } games examined, { $written } written.
eco-classify-unreadable = { $count } games could not be read.
eco-classify-error = The classification stopped: { $reason }

eco-novelty-title = Find novelty
eco-novelty-help = Find the first move of this game that no other game of the selected database has played.
eco-novelty-database = Database
eco-novelty-older = Only older games (before { $date })
eco-novelty-find = Find novelty
eco-novelty-searching = Searching: { $percent }%
eco-novelty-found = Novelty: { $move }
eco-novelty-none = No novelty was found for this game.
eco-novelty-stopped = Novelty search interrupted.
eco-novelty-non-standard = This game starts from a position of its own: there is no novelty search.
eco-novelty-failed = The search failed: a game of the database cannot be read.
eco-novelty-empty = The game has no moves.

eco-notice-no-book = No ECO file is loaded.
eco-notice-opening = Opening { $eco }: { $name } (after half-move { $ply })
eco-notice-no-opening = No position of this game is in the ECO file.
eco-notice-preview = ECO preview: { $classified } games examined, { $changed } would change.
eco-notice-classified = ECO classification: { $classified } games examined, { $changed } changed, { $written } written.
eco-notice-classify-stopped = ECO classification stopped: { $classified } games examined, { $written } written.
eco-notice-read-only = This database cannot be written: only the preview is possible.
eco-notice-novelty = Novelty: { $move }
eco-notice-no-novelty = No novelty was found for this game.
eco-notice-novelty-stopped = Novelty search interrupted.
eco-notice-novelty-non-standard = This game starts from a position of its own: there is no novelty search.
eco-notice-novelty-failed = The novelty search failed: a game of the database cannot be read.
eco-notice-line-failed = The moves of that opening could not be played.


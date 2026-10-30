# The maintenance of the databases (spec M7-1): the window, the confirmations,
# the notices and the Cleaner. The equivalents of the texts of Scid are in
# `english.tcl` (ToolsMaint*, Twin*, Compact*, Cleaner*, ...).

menu-tools-maintenance = Maintenance...
menu-tools-name-editor = Name editor...
menu-tools-repair-base = Repair database...
menu-db-compact = Compact...
menu-db-sort = Sort...
menu-db-twins = Find duplicates...
menu-db-cleaner = Cleaner...
menu-db-check = Check games...
menu-db-type = Database type...
menu-db-description = Description...
menu-db-flags = Custom flags...
menu-db-tags = Extra tags...
menu-db-spelling-file = Spelling file
maint-title = Database maintenance
maint-database = Database:
maint-games = Games:
maint-deleted = Deleted:
maint-in-filter = In filter:
maint-marked = Marked:
maint-years = Years:
maint-ratings = Ratings:
maint-type = Database type
maint-description = Description
maint-custom-flags = Custom flags
maint-change = Change
maint-flags = Flags
maint-delete-current = Delete current game
maint-undelete-current = Undelete current game
maint-mark-current = Mark current game
maint-unmark-current = Unmark current game
maint-delete-filter = Delete filter games
maint-undelete-filter = Undelete filter games
maint-mark-filter = Mark filter games
maint-unmark-filter = Unmark filter games
maint-delete-all = Delete all games
maint-undelete-all = Undelete all games
maint-mark-all = Mark all games
maint-unmark-all = Unmark all games
maint-spell = Spell-checking
maint-operations = Database operations
maint-compact = Compact database
maint-sort = Sort database
maint-eco = ECO classify games
maint-add-elo = Add Elo ratings
maint-duplicates = Delete twin games
maint-autoload = Autoload game
maint-tags = Extra tags
maint-tag-strip = Strip tag
maint-check = Check games
maint-cleaner = Cleaner
maint-strip-comments = Remove comments
maint-strip-variations = Remove variations
maint-drop-deleted = Take deleted games out of the filter
maint-cleaner-help = The Cleaner runs all the chosen actions on the current database. The current ECO and duplicate settings apply.
maint-cleaner-confirm = Once started, the maintenance can be stopped between steps only. It can take a long time. Run the chosen functions?
maint-notice-read-only = This database is read-only
maint-notice-pgn = A PGN file cannot be edited: save it as a database first
maint-notice-clipbase = The Clipbase does not take this operation
maint-notice-no-current-game = No game of this database is on the board
maint-notice-nothing-to-do = There is nothing to do
maint-notice-invalid-value = The value is not valid
maint-notice-bad-sort = A sort needs between one and fifteen criteria
maint-notice-repair-open = The database is open: close it before repairing it
maint-notice-repair-not-si4 = Choose the .si4 file of the database to repair
maint-notice-no-pair = There is no pair of duplicate games to show
maint-notice-flags-done =
    { $changed ->
        [one] { $changed } game changed
       *[other] { $changed } games changed
    }
maint-notice-header-done = The header of the database was changed
maint-notice-twins-found =
    { $marked ->
        [one] { $marked } duplicate game found; { $protected } kept by the protection
       *[other] { $marked } duplicate games found; { $protected } kept by the protection
    }
maint-notice-twins-none = No duplicate game was found
maint-notice-twins-marked =
    { $marked ->
        [one] { $marked } duplicate game marked for deletion
       *[other] { $marked } duplicate games marked for deletion
    }
maint-notice-interrupted = The operation was stopped; what was done stays
maint-notice-check-clean =
    { $examined ->
        [one] No problem found in { $examined } game
       *[other] No problem found in { $examined } games
    }
maint-notice-check-problems =
    { $problems ->
        [one] { $problems } game has a problem
       *[other] { $problems } games have a problem
    }
maint-notice-compacted = Compacted: { $games_removed } games and { $names_removed } names removed
maint-notice-sorted =
    { $moved ->
        [one] The database was sorted; { $moved } game moved
       *[other] The database was sorted; { $moved } games moved
    }
maint-notice-filter-undeleted =
    { $removed ->
        [one] { $removed } deleted game taken out of the filter
       *[other] { $removed } deleted games taken out of the filter
    }
maint-notice-stripped = { $changed } games rewritten, { $skipped } left alone
maint-notice-tags-done = { $changed } games rewritten, { $skipped } left alone
maint-notice-tag-filtered =
    { $matched ->
        [one] The filter is the { $matched } game that has the tag
       *[other] The filter is the { $matched } games that have the tag
    }
maint-notice-names-done = Changed { $edits } names in { $games } games
maint-notice-names-refused = The name editor refused the request
maint-notice-repaired = The database was repaired ({ $games } games). The copies of the original files are kept.
maint-notice-repair-failed = The repair failed. The original files are intact; the copies made are kept.
maint-notice-cleaner-done =
    { $failed ->
        [one] The Cleaner is done; { $failed } step failed
       *[other] The Cleaner is done; { $failed } steps failed
    }
maint-notice-tags-shared =
    { $count ->
        [one] { $count } tag shared
       *[other] { $count } tags shared
    }
maint-notice-nothing-to-share = The two games have nothing to share
maint-notice-backups-kept = { $count } files were copied before the work and are kept
maint-notice-reports-stale = The reports that are open show old data
maint-notice-busy = Another operation is running on the database list. Try again when it ends
maint-confirm-mark-twins =
    { $count ->
        [one] { $count } duplicate game will be marked for deletion. Continue?
       *[other] { $count } duplicate games will be marked for deletion. Continue?
    }
maint-confirm-undelete-first =
    { $count ->
        [one] { $count } deleted game will lose its delete mark before the search. Continue?
       *[other] { $count } deleted games will lose their delete mark before the search. Continue?
    }
maint-confirm-undelete-all =
    { $count ->
        [one] Undelete all { $count } deleted game of the database?
       *[other] Undelete all { $count } deleted games of the database?
    }
maint-confirm-compact =
    { $count ->
        [one] { $count } game will be permanently deleted. Are you sure?
       *[other] { $count } games will be permanently deleted. Are you sure?
    }
maint-confirm-sort =
    { $count ->
        [one] Sorting rewrites the index of { $count } game for good. Continue?
       *[other] Sorting rewrites the index of { $count } games for good. Continue?
    }
maint-confirm-strip =
    { $count ->
        [one] { $count } game will be changed and this cannot be undone. Continue?
       *[other] { $count } games will be changed and this cannot be undone. Continue?
    }
maint-confirm-tag-add =
    { $count ->
        [one] The tag will be added to { $count } game. Continue?
       *[other] The tag will be added to { $count } games. Continue?
    }
maint-confirm-tag-strip =
    { $count ->
        [one] Remove all occurrences of the tag from { $count } game? This cannot be undone.
       *[other] Remove all occurrences of the tag from { $count } games? This cannot be undone.
    }
maint-confirm-names =
    { $count ->
        [one] The names of up to { $count } game will be changed for good. Continue?
       *[other] The names of up to { $count } games will be changed for good. Continue?
    }
maint-confirm-repair = The index and the name file will be rebuilt. Copies of the originals are made next to them and kept. Continue?
maint-confirm-cleaner = The Cleaner runs the chosen steps and some of them cannot be undone. Continue?
maint-confirm-share-tags =
    { $count ->
        [one] { $count } tag will be copied between the two games. Continue?
       *[other] { $count } tags will be copied between the two games. Continue?
    }
maint-confirm-weak-twins = The criteria are weak. Continue?
maint-confirm-backup = Copy the three files of the database first
maint-weak-no-moves = The moves are not compared and colours, event, site, year or month are not all required: many games that are not duplicates may be marked.
maint-weak-few-fields = Fewer than two of site, round and year are required: the criteria are weak.
maint-cleaner-step-players = Spelling of player names
maint-cleaner-step-events = Spelling of event names
maint-cleaner-step-sites = Spelling of site names
maint-cleaner-step-rounds = Spelling of round names
maint-cleaner-step-eco = ECO classification
maint-cleaner-step-elo = Add Elo ratings
maint-cleaner-step-twins = Mark duplicate games
maint-cleaner-step-compact-names = Compact the name file
maint-cleaner-step-compact-games = Compact the game file
maint-cleaner-step-tree = Fill the tree cache
maint-cleaner-status-done = Done
maint-cleaner-status-skipped = Skipped
maint-cleaner-status-interrupted = Stopped
maint-cleaner-status-failed = Failed
maint-cleaner-detail-no-spelling = No spelling file is loaded
maint-cleaner-detail-no-elo = The spelling file has no ratings
maint-cleaner-detail-no-eco-book = No ECO book is loaded
maint-cleaner-detail-stale = The files changed outside the program
maint-cleaner-detail-failed = The step failed
maint-cleaner-detail-stopped-before = Stopped before this step
maint-check-cause-index = The index entry cannot be read
maint-check-cause-record = The game record cannot be read
maint-check-cause-decode = The game cannot be decoded
maint-check-cause-names = A name of the game is missing
maint-check-cause-illegal-move = A move of the game is not legal
maint-repair-backup-kept = Copy of the original kept at { $path }

# ---- window M7-1c: labels of the forms and the window ----
maint-ui-refresh = Update
maint-ui-close = Close
maint-ui-stop = Stop
maint-ui-working = Working...
maint-ui-counting = Counting the database...
maint-ui-no-base = No database is open. The repair of a closed database is available below.
maint-ui-info = Database
maint-ui-edit = Edit
maint-ui-apply = Apply
maint-ui-confirm = Please confirm
maint-ui-state = State
maint-ui-state-rw = read and write
maint-ui-state-ro = read only (this session)
maint-clipbase = Clipbase
maint-count-percent = { $count } ({ $percent }%)
maint-range = { $low }-{ $high } ({ $mean })
maint-range-empty = -
maint-flag-entry = { $letter }: { $name }
maint-more = { $count } more
maint-scope-all = All games
maint-scope-filter = Games in the filter
maint-open-checker = Duplicate checker
maint-repair = Repair database
maint-names-editor = Name editor
maint-spell-players = Players
maint-spell-events = Events
maint-spell-sites = Sites
maint-spell-rounds = Rounds
maint-flag-delete = Deleted
maint-flag-white-opening = White opening
maint-flag-black-opening = Black opening
maint-flag-middlegame = Middlegame
maint-flag-endgame = Endgame
maint-flag-novelty = Novelty
maint-flag-pawn-structure = Pawn structure
maint-flag-tactics = Tactics
maint-flag-kingside = Kingside
maint-flag-queenside = Queenside
maint-flag-brilliancy = Brilliancy
maint-flag-blunder = Blunder
maint-flag-user = User
maint-flag-custom = Custom { $number }
maint-flag-custom-described = { $description } ({ $number })
maint-compact-name-file = Name file
maint-compact-game-file = Game file
maint-compact-players = Players
maint-compact-events = Events
maint-compact-sites = Sites
maint-compact-rounds = Rounds
maint-compact-names-col = Names
maint-compact-unused = Unused
maint-compact-games-col = Games
maint-compact-size = Size (kb)
maint-compact-current = Current state
maint-compact-after = After compaction
maint-compact-names = Compact the name file
maint-compact-games = Compact the game file
maint-compact-names-clean = There are no unused names: the name file is already fully compact.
maint-compact-games-clean = The game file is already fully compact.
maint-sort-criteria = Criteria
maint-sort-none = (none)
maint-sort-add = Add criterion
maint-sort-common = Common sorts
maint-sort-common-1 = Year, month, site, event, date, round
maint-sort-common-2 = Year, event, date, round
maint-sort-common-3 = ECO, rating
maint-sort-remove-last = Remove last
maint-sort-clear = Clear
maint-sort-reverse = Reverse the order
maint-sort-compact-after = Compact the games afterwards
maint-sort-note = Sorting rewrites the index for good. The game file stays out of order until it is compacted, which makes searches slower.
maint-sort-go = Sort
maint-sort-key-date = Date
maint-sort-key-year = Year
maint-sort-key-event = Event
maint-sort-key-site = Site
maint-sort-key-round = Round
maint-sort-key-white = White
maint-sort-key-black = Black
maint-sort-key-eco = ECO
maint-sort-key-result = Result
maint-sort-key-length = Length
maint-sort-key-rating = Rating
maint-sort-key-welo = White Elo
maint-sort-key-belo = Black Elo
maint-sort-key-country = Country
maint-sort-key-month = Month
maint-sort-key-deleted = Deleted
maint-sort-key-eventdate = Event date
maint-sort-key-variations = Variations
maint-sort-key-comments = Comments
maint-twins-note = To be duplicates, games must have the same players and the other criteria below. It is better to fix the spelling of the names first.
maint-twins-criteria = Criteria
maint-twins-names-exact = Exact player names
maint-twins-names-four = First four letters only
maint-twins-colors = Same colours
maint-twins-event = Same event
maint-twins-site = Same site
maint-twins-round = Same round
maint-twins-year = Same year
maint-twins-month = Same month
maint-twins-day = Same day
maint-twins-result = Same result
maint-twins-eco = Same ECO code
maint-twins-moves = Same moves
maint-twins-which-games = Which games
maint-twins-scope-all = All games
maint-twins-scope-filter = Games in the filter
maint-twins-when-deleting = When deleting
maint-twins-skip-short = Skip short games (fewer than 5 moves)
maint-twins-undelete-first = Undelete all games first (this erases your own delete marks)
maint-twins-set-filter = Set the filter to the duplicates to delete
maint-twins-keep-commented = Always keep games with comments
maint-twins-keep-variations = Always keep games with variations
maint-twins-which-delete = Which one to delete
maint-twins-delete-shorter = The shorter game
maint-twins-delete-lower = The lesser game number
maint-twins-delete-higher = The greater game number
maint-twins-defaults = Defaults
maint-twins-go = Find duplicates
maint-checker-title = Duplicate game checker
maint-checker-position = Pair { $position } of { $total }
maint-checker-game = Game { $game }
maint-checker-deleted = Deleted
maint-checker-comparing = Comparing the games...
maint-checker-too-large = The games are too large to compare.
maint-checker-share = Share tags...
maint-checker-undelete-all = Undelete all games
maint-checker-delete-twins = Delete duplicates...
maint-cleaner-go = Run
maint-cleaner-report = Report
maint-cleaner-step-unknown = Unknown step
maint-check-all = Check all games
maint-check-filter = Check the games in the filter
maint-check-summary = { $examined } games examined, { $problems } with a problem
maint-check-item = Game { $game }: { $cause }
maint-strip-title = Remove comments or variations
maint-strip-note = Removing comments or variations cannot be undone.
maint-strip-go = Remove
maint-tags-list = List the tags
maint-tags-filter = Filter by the tag
maint-tags-interrupted = The search was stopped: the list and the counts are not correct.
maint-tags-add = Add a tag
maint-tags-name = Tag name
maint-tags-value = Value
maint-tags-add-go = Add the tag
maint-tags-note = The name has letters, digits and underscores only. Adding tags makes the game file grow: the space comes back only when it is compacted.
maint-names-type = Kind of name to edit
maint-names-kind-player = Player
maint-names-kind-event = Event
maint-names-kind-site = Site
maint-names-kind-round = Round
maint-names-kind-rating = Rating
maint-names-kind-date = Date
maint-names-kind-event-date = Event date
maint-names-games = Games to edit
maint-names-scope-tournament = Only the games of the current tournament
maint-names-replace = Replace
maint-names-with = With
maint-names-player = Player
maint-names-elo = Rating
maint-names-old-date = Replace (YYYY.MM.DD, * for any)
maint-names-new-date = With (YYYY.MM.DD)
maint-names-pattern = Match by pattern (* and ?)
maint-names-go = Replace
maint-autoload-number = Game number (0 for none)
maint-autoload-none = None
maint-autoload-first = First
maint-autoload-current = Current
maint-autoload-last = Last
maint-repair-note = Choose the .si4 file of a database that is not open. The index and the name file are rebuilt; copies of the originals are made and kept.
maint-repair-none = No file chosen
maint-repair-choose = Choose file...
maint-repair-dialog-title = Choose the database to repair
maint-repair-file-filter = Scid databases (.si4)
maint-repair-drop = Also drop the entries whose game cannot be read
maint-repair-go = Repair
maint-type-0 = Unknown
maint-type-1 = Temporary database
maint-type-2 = Clipbase
maint-type-3 = PGN file
maint-type-4 = My games
maint-type-5 = Large database
maint-type-6 = Correspondence chess
maint-type-7 = Computer chess
maint-type-8 = Index of games
maint-type-9 = Player collection
maint-type-10 = Tournament
maint-type-11 = Internet
maint-type-12 = GM games
maint-type-13 = IM games
maint-type-14 = Blitz games
maint-type-15 = Tactics
maint-type-16 = Endgames
maint-type-17 = Openings for White
maint-type-18 = Openings for Black
maint-type-19 = Openings for either color
maint-type-20 = Theory: 1.c4
maint-type-21 = Theory: 1.d4
maint-type-22 = Theory: 1.d4 d5
maint-type-23 = Theory: QGD
maint-type-24 = Theory: Slav
maint-type-25 = Theory: 1.d4 Nf6
maint-type-26 = Theory: 1.e4
maint-type-27 = Theory: Sicilian
maint-type-28 = Theory: Sicilian 1.e4 c5 2.Nf3 d6 3.d4
maint-type-29 = Theory: Caro-Kann
maint-type-30 = Theory: French
maint-type-31 = Theory: Open Games
maint-custom-note = Each flag has a description of up to 8 characters; its first letter shows in the list of games.

# The opening tree panel (spec M5-2). The equivalents of the Scid's texts
# are in `english.tcl` 531-589.

pane-tree = Opening tree
action-toggle-tree-pane = Show or hide the opening tree

tree-col-move = Move
tree-col-games = Games
tree-col-score = Score
tree-col-draws = Draws
tree-col-elo = Elo
tree-col-perf = Perf
tree-col-year = Year
tree-col-eco = ECO
tree-total = TOTAL

# Percentages: whole part and tenths, with the decimal separator of the language.
tree-percent = { $whole }.{ $tenth }%
tree-percent-whole = { $value }%

tree-refresh = Refresh
tree-refresh-tip = Update the table when the position changes
tree-adjust = Adjust filter
tree-adjust-tip = Make the game list show only the games that reach this position
tree-sort = Sort
tree-sort-alphabetical = Alphabetical
tree-sort-eco = ECO code
tree-sort-frequency = Frequency
tree-sort-score = Score
tree-mode = Search
tree-mode-slow = Slow
tree-mode-fast = Fast
tree-mode-fast-then-slow = Fast, then slow
tree-short = Short display
tree-close = Close the tree

tree-searching = Searching...
tree-loading = Searching the games...
tree-unavailable = Base not available
tree-empty = No game reaches this position
tree-issue-too-many-moves = More than 60 moves have been played from this position
tree-issue-unsupported = This position is not supported by the database
tree-issue-filter-size = The database changed during the search
tree-issue-internal = The search failed
tree-issue-read = The database could not be read: { $reason }
tree-unreadable = { $count ->
        [one] { $count } game could not be read
       *[other] { $count } games could not be read
    }

# Spec M5-2b and M5-2c: best games, training, lock, open base as tree.
tree-training = Training
tree-training-tip = The tree answers your moves with a move drawn from the games
tree-lock = Lock
tree-lock-tip = Keep the tree on this database when you switch to another
tree-target = Database: { $name }
tree-target-clipbase = Database: Clipbase
tree-open-base = Open base as tree
tree-best = Best games
tree-best-result = Result
tree-best-sort = Sort
tree-best-max = Games
tree-best-results-all = All
tree-best-results-white = 1-0
tree-best-results-black = 0-1
tree-best-results-decisive = 1-0 and 0-1
tree-best-results-draw = Draws
tree-best-results-unfinished = No result
tree-best-order-rating = Best (rating)
tree-best-order-database = Database order
tree-best-columns = Columns
tree-best-load = Load
tree-best-browse = Browse
tree-best-copy = Copy
tree-best-empty = No games to show
tree-best-col-moves = Moves
file-dialog-open-tree-base = Open a database as a tree
file-filter-databases = Databases and PGN files

# Spec M5-3b: the cache of the trees (the `.stc` file). Scid: TreeFile* in `english.tcl`.
tree-cache = Cache
tree-cache-save = Save cache
tree-cache-fill-base = Fill with the database
tree-cache-fill-game = Fill with the current game
tree-cache-fill-lines = Fill with the standard lines
tree-cache-info = Cache information
tree-cache-size = Cache size
tree-cache-autosave = Save cache automatically
tree-cache-autosave-tip = Save the cache when the tree closes and when the program ends
tree-cache-busy-save = Saving the cache...
tree-cache-busy-info = Reading the cache...
tree-cache-busy-fill = Filling the cache: { $done } of { $total }
tree-cache-busy-fill-unknown = Filling the cache...
tree-cache-cancel = Cancel
tree-cache-dismiss = Dismiss
tree-cache-saved = Cache saved ({ $bytes } bytes)
tree-cache-saved-skipped = Cache saved ({ $bytes } bytes); { $skipped } trees were left out
tree-cache-filled = Cache filled: { $searches } searches, { $hits } already in the cache
tree-cache-info-text = Cache: { $used } of { $capacity } trees. Backup: { $backup } of { $backup_capacity }.
tree-cache-info-lowest = Smallest tree: { $games } games.
tree-cache-info-file = File: { $bytes } bytes.
tree-cache-info-no-file = No file.
tree-cache-failed-not-scid = Only Scid databases (.si4) have a cache file
tree-cache-failed-busy = Another cache operation is running
tree-cache-failed-discarding = The old cache file is still being removed; try again in a moment
tree-cache-failed-no-base = There is no database to work on
tree-cache-failed-stale = The database changed; the cache was discarded
tree-cache-failed-cancelled = Cancelled
tree-cache-failed-illegal-line = A standard line is not legal
tree-cache-failed-file = The cache file could not be used: { $reason }
tree-cache-failed-search = A search failed: { $reason }
tree-cache-failed-internal = The cache operation failed

# Tree masks (spec M5-4b)
tree-mask-open-failed = Could not open the mask { $path }: { $reason }
tree-mask-save-failed = Could not save the mask { $path }: { $reason }
tree-mask-saved = Mask saved
tree-mask-warnings = { $count ->
        [one] The mask was read with { $count } warning
       *[other] The mask was read with { $count } warnings
    }
tree-mask-no-mask = Open a mask file first
tree-mask-busy = Another job of the mask is running; stop it first
tree-mask-not-standard = Masks hold standard chess only
tree-mask-filled = Mask filled: { $moves ->
        [one] { $moves } move
       *[other] { $moves } moves
    } from { $games ->
        [one] { $games } game
       *[other] { $games } games
    } ({ $skipped } skipped)
tree-mask-fill-cancelled = Fill stopped: { $moves ->
        [one] { $moves } move
       *[other] { $moves } moves
    } from { $games ->
        [one] { $games } game
       *[other] { $games } games
    } were added ({ $skipped } skipped)
tree-mask-fill-unavailable = There are no games to fill the mask from
tree-mask-full = The mask is full: some data was left out
tree-mask-cannot-show = That position cannot be shown
tree-mask-issue-not-a-mask = the file is not a tree mask
tree-mask-issue-foreign = the file holds more than a mask; nothing was run
tree-mask-issue-syntax = the file is malformed
tree-mask-issue-too-large = the file is too large
tree-mask-issue-read-only = the file is read-only
tree-mask-issue-not-found = the file was not found
tree-mask-issue-denied = permission denied
tree-mask-issue-io = the file could not be used
tree-mask-issue-internal = something failed inside the program
tree-mask-unsaved-title = Save the mask?
tree-mask-unsaved-body = The mask { $name } was changed. Save it first?
tree-mask-unsaved-body-unnamed = The new mask was changed. Save it first?
tree-mask-game-too-large = The game is too big to fill the mask from it

# Tree masks, interface (spec M5-4c). The names of the markers follow the
# Scid (`english.tcl` 600-609).
tree-mask-menu = Mask
tree-mask-menu-tip = Open, edit and fill a tree mask
tree-mask-new = New
tree-mask-open = Open...
tree-mask-open-recent = Open recent
tree-mask-remove-recent = Remove from the list
tree-mask-save = Save
tree-mask-save-as = Save as...
tree-mask-close = Close
tree-mask-display = Display mask
tree-mask-search = Search the mask
tree-mask-fill-line = Fill with the line
tree-mask-fill-game = Fill with the game
tree-mask-fill-filter = Fill with the filter
tree-mask-show-markers = Show markers
tree-mask-show-tooltips = Show tooltips
tree-mask-auto-load = Load the last mask when the tree opens
tree-mask-title = Mask: { $name }
tree-mask-title-dirty = Mask: { $name } • modified
tree-mask-unnamed = (new mask)
tree-mask-col-comment = Comment
tree-mask-only-row-tip = This move is only in the mask
tree-mask-illegal-move = (illegal)
tree-mask-illegal-move-tip = This move is illegal in this position

tree-mask-marker-include = Include
tree-mask-marker-exclude = Exclude
tree-mask-marker-main-line = Main line
tree-mask-marker-bookmark = Bookmark
tree-mask-marker-white = White
tree-mask-marker-black = Black
tree-mask-marker-new-line = New line
tree-mask-marker-to-verify = To be verified
tree-mask-marker-to-train = To train
tree-mask-marker-dubious = Dubious
tree-mask-marker-to-remove = To remove
tree-mask-marker-other = Other marker
tree-mask-marker-none = No marker
tree-mask-color-white = White
tree-mask-color-green = Green
tree-mask-color-yellow = Yellow
tree-mask-color-blue = Blue
tree-mask-color-red = Red
tree-mask-color-other = Other color
tree-mask-nag-brilliant = !! Brilliant
tree-mask-nag-good = ! Good
tree-mask-nag-interesting = !? Interesting
tree-mask-nag-dubious = ?! Dubious
tree-mask-nag-poor = ? Poor
tree-mask-nag-blunder = ?? Blunder
tree-mask-nag-unclear = ~ Unclear
tree-mask-nag-none = No NAG

tree-mask-ctx-add = Add to the mask: { $move }
tree-mask-ctx-remove = Remove from the mask: { $move }
tree-mask-ctx-comment-move = Comment the move
tree-mask-ctx-comment-position = Comment the position
tree-mask-ctx-add-move = Add this move to the mask
tree-mask-ctx-no-moves = No legal moves
tree-mask-ctx-marker-1 = Marker 1
tree-mask-ctx-marker-2 = Marker 2
tree-mask-ctx-color = Color
tree-mask-ctx-nag = NAG
tree-mask-ctx-apply-line = Apply to the line

tree-mask-fill-line-title = Fill the mask with the line
tree-mask-fill-line-body = Add the moves of the line, from the start of the game to the cursor, to the mask. Which moves?
tree-mask-side-all = All moves
tree-mask-side-white = White's moves
tree-mask-side-black = Black's moves

tree-mask-progress-open = Opening the mask...
tree-mask-progress-save = Saving the mask...
tree-mask-progress-fill = Filling the mask: game { $done } of { $total }
tree-mask-progress-fill-unknown = Filling the mask: { $done } games
tree-mask-progress-search = Searching the mask...
tree-mask-progress-outline = Building the outline...
tree-mask-stop = Stop
tree-mask-stopping = Stopping...

tree-mask-comment-move-title = Comment on the move { $move }
tree-mask-comment-position-title = Comment on the position
tree-mask-comment-hint = { $shortcut } saves; Esc cancels.
tree-mask-comment-placeholder = Type a comment...
tree-mask-comment-ok = OK

tree-mask-search-title = Search the mask
tree-mask-search-nag = NAG
tree-mask-search-marker-1 = Marker 1
tree-mask-search-marker-2 = Marker 2
tree-mask-search-color = Color
tree-mask-search-move-text = Text in the comment of the move
tree-mask-search-position-text = Text in the comment of the position
tree-mask-search-run = Search
tree-mask-search-close = Close
tree-mask-search-go = Go to the position
tree-mask-search-col-move = Move
tree-mask-search-col-comment = Comment
tree-mask-search-col-fen = FEN
tree-mask-search-position-row = (position)
tree-mask-search-summary = Positions { $positions_found }/{ $positions_total }, moves { $moves_found }/{ $moves_total }
tree-mask-search-truncated = Only the first { $limit } rows are listed.
tree-mask-search-empty = Nothing found.
tree-mask-search-hint = Double click a row to open a new game at that position.

tree-mask-outline-title = Mask
tree-mask-outline-expand = Expand all
tree-mask-outline-collapse = Collapse all
tree-mask-outline-nags = Show NAGs and markers
tree-mask-outline-comments = Show comments
tree-mask-outline-refresh = Refresh
tree-mask-outline-truncated = The outline was cut: there are too many lines to show them all.
tree-mask-outline-empty = The mask has no moves from this position.
tree-mask-outline-building = Building the outline...
tree-mask-outline-hint = Double click a line to open a new game at that position.
tree-mask-outline-toggle = Expand or collapse

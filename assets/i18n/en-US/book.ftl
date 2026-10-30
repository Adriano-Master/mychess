# The book window: one Polyglot book, with a second one to compare (spec M5-6,
# M5-6c). The equivalents of the Scid's texts are in `english.tcl` 148, 168,
# 333, 746-749.

pane-book = Opening books
menu-tools-book = Opening books
pane-close-book = Close the opening books panel

book-title-first = Book 1
book-title-second = Book 2
book-compare = Compare
book-compare-tip = Compare with another book
book-compare-close-tip = Close the comparison
book-tools-title = Tools
book-opening-issue-badge = Opening book problem
book-tools-show-tip = Show the tools (edit weights, add lines, make and merge books)
book-tools-hide-tip = Hide the tools
book-tools-hint = The table can be edited now: type a weight, double, halve or remove a move.
book-group-edit = Edit this position
book-untitled = (no file)
book-changed = Changed
book-empty-slot = No book yet. Open a file or start a new one.
book-entries = { $count ->
    [one] 1 record
   *[other] { $count } records
}
book-read-only = Read-only
book-is-opening = Opening book

book-open = Open book...
book-new = New book
book-save = Save book
book-save-as = Save book as...
book-close-book = Close book (the file is kept)
book-use-as-opening = Use as the opening book
book-use-this = Use this book
book-use-first = Use book 1
book-use-second = Use book 2

book-col-move = Move
book-col-weight = Weight
book-col-percent = %
book-no-moves = The book has no move in this position.
book-not-standard = A Polyglot book only holds standard chess positions.
book-game-move = The game plays: { $san }
book-others = Other moves
book-others-tip = Click a move to add it to the book with weight 1
book-weight-tip = Type a weight from 1 to 65520 and press Enter
book-double = x2
book-half = /2
book-double-tip = Double the weight
book-half-tip = Halve the weight
book-remove-move-tip = Remove this move from the book

book-add-game-move = Add move
book-add-line = Add line
book-remove-line = Remove line
book-import-branch = Import branch
book-copy-from-other = Copy from other
book-add-game-move-tip = Add the move that the game plays next, with weight 1
book-add-line-tip = Add the main line of the game from this position, with weight 1
book-remove-line-tip = Remove the main line of the game from this position
book-import-branch-tip = Add every line of the game from this position, variations included
book-copy-tip = Replace the moves of this position with those of the other book

book-confirm-close = The book has changes that are not saved. Close it anyway?
book-confirm-yes = Close and discard
book-confirm-no = Keep

book-create-title = Make from the filter
book-create-games = { $count ->
    [one] 1 game in the filter
   *[other] { $count } games in the filter
}
book-create-max-ply = Plies per game
book-create-min-games = Minimum games
book-create-min-score = Minimum score (%)
book-create-only-white = Only White
book-create-only-black = Only Black
book-create-uniform = Same weight
book-create-make = Make book
book-create-into-first = Into book 1
book-create-into-second = Into book 2
book-create-stop = Stop
book-create-stop-tip = Stop making the book; nothing is kept

book-merge-title = Merge books
book-merge-keep-first = Keep book 1
book-merge-sum = Sum the weights
book-merge-button = Merge into book 1

book-opening-title = Opening book for annotation
book-opening-none = None chosen
book-opening-clear = Use none
book-opening-loaded = { $name } (loaded)

book-notice-saved = The book was saved.
book-notice-created = Book made: { $used } games used, { $skipped } left out, { $entries } records.
book-notice-merged = Books merged: { $written } records written, { $skipped } left out.
book-notice-added = { $count ->
    [one] 1 move added.
   *[other] { $count } moves added.
}
book-notice-removed = { $count ->
    [one] 1 move removed.
   *[other] { $count } moves removed.
}
book-notice-copied = The moves were copied.
book-notice-opening-loaded = The opening book was read.
book-notice-create-cancelled = Stopped. No book was made.
book-dismiss = Dismiss

book-issue-not-found = The file does not exist.
book-issue-unreadable = The file cannot be read as a Polyglot book.
book-issue-too-short = The file is too short to be a book.
book-issue-too-large = The file is too large for a book.
book-issue-read-only = The book is read-only.
book-issue-no-path = The book has no file yet. Use Save as.
book-issue-write-failed = The book could not be written.
book-issue-unsupported = Chess960 and set-up positions cannot be in a Polyglot book.
book-issue-illegal-move = The move is not legal here.
book-issue-zero-weight = The weight cannot be zero. Remove the move instead.
book-issue-weight-too-large = The weight cannot be above 65520.
book-issue-too-many-moves = A position can have at most 100 moves in a book.
book-issue-not-a-number = The weight must be a whole number.
book-issue-already-there = The move is already in the book.
book-issue-nothing-to-add = There is nothing after this position to use.
book-issue-branch-too-big = The branch has too many lines to import.
book-issue-unsaved-changes = The book has changes that are not saved.
book-issue-no-book = Open or start a book first.
book-issue-no-games = The current database has no game in its filter.
book-issue-busy = The book is busy with another operation.
book-issue-cancelled = The operation was stopped.
book-issue-internal = The operation failed.

book-dialog-open-title = Open an opening book
book-dialog-save-title = Save the opening book
book-dialog-filter = Opening books (*.bin)

action-toggle-book-pane = Show or hide the opening books panel

book-quit-title = Opening books with unsaved changes
book-quit-message = Some opening books have changes that are not saved. Save them before quitting?
book-quit-save = Save
book-quit-discard = Discard
book-quit-cancel = Cancel

book-export-title = Export branch to PGN
book-export-hint = Writes the lines of the book that start at the current position, one game per line, heaviest moves first.
book-export-max-plies = Plies per line
book-export-max-lines = Most lines
book-export-min-weight = Least weight (0: all)
book-export-make = Export branch to PGN…
book-export-first = Export book 1…
book-export-second = Export book 2…
book-export-tip = Export the lines of this book that start at the current position to a PGN file
book-export-stop = Stop
book-export-stop-tip = Stop the export; the file is not written
book-export-dialog-title = Export the branch to PGN
book-export-dialog-filter = PGN files (*.pgn)
book-notice-exported = { $count ->
    [0] No line was exported.
    [one] 1 line exported.
   *[other] { $count } lines exported.
}
book-notice-exported-truncated = { $count ->
    [0] No line was exported.
    [one] 1 line exported; the limit of lines cut the branch short.
   *[other] { $count } lines exported; the limit of lines cut the branch short.
}
book-notice-export-cancelled = Stopped. No file was written.
book-issue-empty-branch = The book has no move in this position.
book-issue-protected-file = That file belongs to an open database or book. Choose another name.
book-issue-file-exists = A file with that name already exists. Choose another name.
book-notice-export-stopping = Stopping the export…
book-notice-export-finished = { $count ->
    [0] The export had already finished; no line was written.
    [one] Too late to stop: the file was written with 1 line.
   *[other] Too late to stop: the file was written with { $count } lines.
}
book-issue-bad-name = The file name is not valid text. Choose another name.

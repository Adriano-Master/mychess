# The opening trainer (spec M8-5). The Scid's texts are in `english.tcl`
# (Openingtrainer, PlayerBestMove, OpponentBestMove, RemoveGames,
# OnlyFlaggedLines, resetStats, MaxPly, Movenotinrepertoire, EndOfVar, ...).

action-opening-trainer = Opening trainer
menu-play-opening-trainer = Opening trainer...

opening-title = Opening trainer
opening-title-white = Opening trainer - White
opening-title-black = Opening trainer - Black

opening-side = Side to train
opening-side-white = White
opening-side-black = Black
opening-side-both = Both
opening-source = Repertoire
opening-source-typed = By database type
opening-source-base = Current database
opening-source-file = File...
opening-choose-file = Choose...
opening-file-dialog-title = Choose the repertoire (book, mask or games)
opening-file-filter = Books, masks, Scid databases and PGN files
opening-no-file = No file chosen
opening-uses = Uses: { $names }
opening-uses-none = No database of the type "Openings for ..." is open
opening-depth = Maximum depth (plies)
opening-strict = Allow only the best moves
opening-opponent-best = The opponent plays the best moves
opening-no-repeat = Do not repeat a line
opening-only-flagged = Only flagged lines
opening-show-candidates = Show the candidate moves
opening-only-marked = From a mask: only the moves marked "to train" or green
opening-reset = Reset statistics
opening-reset-confirm = Click again to confirm
opening-reset-tip = Erases the score of this repertoire
opening-start = Start
opening-cancel = Cancel

opening-status-closed = No line on the board
opening-status-building = Reading the repertoire...
opening-status-user-turn = Your move
opening-status-line-complete = End of the line: the repertoire has no more moves here
opening-status-line-depth = End of the line: maximum depth reached
opening-status-exhausted = Every line that could be drawn was played in this session

opening-notice-busy = Finish the game, the tournament or the training in progress first
opening-notice-no-repertoire = No database of the type "Openings for ..." is open
opening-notice-no-source = Choose a repertoire file (or use the current database)
opening-notice-source-unreadable = The file could not be read
opening-notice-source-upgrade = The database is in the old SI3 format: convert it first
opening-notice-source-corrupt = The database files are damaged or from another version
opening-notice-source-unfinished = The database has an unfinished transaction: open it as a database and repair it first
opening-notice-book-unreadable = The book could not be read
opening-notice-mask-unreadable = The mask could not be read
opening-notice-empty-repertoire = The repertoire has no move for the side to train
opening-notice-one-side-empty = The repertoire has moves for only one side: training that one
opening-notice-truncated = The repertoire was cut by a limit (games or positions)
opening-notice-locked = The board is locked while it is your move
opening-notice-timed-out = The request took too long and was dropped
opening-notice-no-hint = There is no hint for this position
opening-notice-dubious = This move is in the repertoire but is marked as dubious
opening-notice-out-of-repertoire = Move not in the repertoire
opening-notice-store-not-saved = The score could not be saved
opening-notice-store-unreadable = The saved score was unreadable and was set aside
opening-notice-store-newer = The saved score is from a newer version of the program and will not be changed
opening-notice-source-changed = The database was changed or closed while the repertoire was being read

opening-session = Lines { $lines } (perfect { $perfect }), right { $good }, dubious { $dubious }, out { $out }
opening-candidates = Repertoire: { $moves }
opening-report = Positions { $positions }, never played { $never }, played { $played }, due { $due }, learned { $learned } - right { $good }, dubious { $dubious }, out { $out }
opening-hint-piece = Hint: move the piece on { $square }

opening-hint = Hint
opening-hint-tip = Shows which piece to move and then the move itself
opening-retry = Repeat line
opening-retry-tip = Starts the same line again
opening-next = Next line
opening-next-tip = Draws another line (Enter at the end of a line)
opening-report-button = Report
opening-report-tip = Shows how much of the repertoire you know
opening-close = Close
opening-close-tip = Ends the training and brings back your game

opening-notice-game-not-restored = Your previous game could not be put back: the editor holds another game with unsaved changes and no slot is free to keep it

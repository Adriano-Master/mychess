# The tactics training: "mate in N" and "find the best move" (spec M8-4).
# The Scid's texts are in `english.tcl` (MateFound, BestSolutionNotFound,
# MateNotFound, ShorterMateExists, WinWonGame, ShowSolution, ...).

action-tactics-trainer = Tactics training
menu-group-training = Training
menu-play-tactics = Tactics training...

tactics-title = Tactics training
tactics-title-mate = Mate in { $n }
tactics-title-best = Find the best move

tactics-status-closed = No problem on the board
tactics-status-opening = Reading the problems...
tactics-status-searching = Looking for the next problem... ({ $count } games examined)
tactics-status-judging = Checking your move...
tactics-status-solving-mate = Your move: find the mate in { $n }
tactics-status-solving-best = Your move: find the best move
tactics-status-exhausted = No more problems here: all of them were solved or skipped
tactics-feedback-correct = Correct! Keep going: mate in { $n }
tactics-feedback-no-mate = Not that one: the forced mate is gone
tactics-feedback-longer-mate = A shorter mate exists
tactics-feedback-not-best = Not the best move
tactics-result-perfect = Solved at the first try!
tactics-result-mistakes = Solved, after some mistakes
tactics-result-help = Solved, with a hint
tactics-result-gave-up = The solution is on the board
tactics-result-no-attempts = Out of attempts: the solution is on the board
tactics-result-time-up = Time is up: the solution is on the board

tactics-notice-busy = Finish the game, the tournament or the analysis mode that is on first
tactics-notice-unsaved-game = Save or discard the game of the editor first
tactics-notice-no-source = Choose a PGN file (or use the current database)
tactics-notice-source-unreadable = The file could not be read
tactics-notice-source-upgrade = The database is in the old SI3 format: upgrade it first
tactics-notice-source-corrupt = The database files are damaged or from another version
tactics-notice-source-unfinished = The database has an unfinished transaction: open it as a base and repair it first
tactics-notice-no-engine = No engine of the list can give a second opinion
tactics-notice-opinion-unavailable = The engine did not answer: the move was judged by the game alone
tactics-notice-no-engine-to-play = No UCI engine of the list can play the game
tactics-notice-parked-unsaved = Close the training first: the game set aside has unsaved changes
tactics-notice-no-games = The source has no games
tactics-notice-locked = The board is locked while a problem is open
tactics-notice-timed-out = The request took too long and was dropped
tactics-notice-inconclusive = This move could not be checked: try another or see the solution
tactics-notice-no-hint = There is no hint for this position
tactics-notice-store-not-saved = The score could not be saved
tactics-notice-store-unreadable = The saved score was unreadable and was set aside
tactics-notice-store-newer = The saved score is from a newer version of the program and will not be changed
tactics-notice-source-changed = The database changed or was closed, so the training ended

tactics-stats = Solved { $solved } of { $shown } ({ $percent }%), streak { $streak } (best { $best }), average { $time }
tactics-hint-piece = Hint: move the piece on { $square }
tactics-time-limit = limit { $count } s
tactics-attempts = attempts { $count }/{ $max }

tactics-hint = Hint
tactics-hint-tip = Show which piece to move, then the move itself
tactics-show-solution = Show solution
tactics-show-solution-tip = Give up and see the solution on the board
tactics-retry = Try again
tactics-retry-tip = Set the same problem up again
tactics-continue = Continue
tactics-continue-tip = Play on against the engine from this position
tactics-next = Next problem
tactics-next-tip = Go to the next problem (skips this one if it is not solved)
tactics-close = Close
tactics-close-tip = End the training and get your game back

tactics-mode = Training
tactics-mode-mate = Mate in N
tactics-mode-best = Best move
tactics-source = Problems from
tactics-source-file = File (SI4 or PGN)
tactics-source-base = Current database (filter)
tactics-choose-file = Choose...
tactics-file-dialog-title = Choose the file with the problems
tactics-file-filter = Scid databases and PGN files
tactics-second-opinion = Ask the engine when my move is not the one of the game
tactics-flagged-only = Only games with the Tactics flag (databases)
tactics-beyond-limit =
    { $count ->
        [0] No mate problem is beyond the limit
        [one] { $count } mate problem of more than { $limit ->
            [one] { $limit } move
           *[other] { $limit } moves
        } was skipped: raise "Longest mate"
       *[other] { $count } mate problems of more than { $limit ->
            [one] { $limit } move
           *[other] { $limit } moves
        } were skipped: raise "Longest mate"
    }
tactics-scan-mates = Look for mates in the games without markers
tactics-no-file = No file chosen
tactics-order = Order
tactics-order-sequential = In sequence
tactics-order-random = Random
tactics-mate-min = Shortest mate
tactics-mate-max = Longest mate
tactics-difficulty-min = Easiest difficulty
tactics-difficulty-max = Hardest difficulty
tactics-elo-min = Lowest rating
tactics-elo-max = Highest rating
tactics-attempts-label = Attempts per problem
tactics-time-limit-label = Time limit (seconds)
tactics-include-solved = Show the problems already solved
tactics-hints = Allow hints
tactics-accept-longer = Accept a mate that is up to two moves longer
tactics-reset = Reset score
tactics-reset-confirm = Click again to confirm
tactics-reset-tip = Clears the score of this mode and the solved problems of this source
tactics-off = off
tactics-cancel = Cancel
tactics-start = Start

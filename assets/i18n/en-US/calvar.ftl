# Calculation of variants, the Stoyko exercise (spec M8-6). The Scid's texts
# are in `english.tcl` (Calvar, ConfigureCalvar, ToolsTrainCalvar,
# DoneWithPosition).

action-calvar-trainer = Calculation of variants
menu-play-calvar = Calculation of variants...

calvar-title = Calculation of variants
calvar-title-white = Calculation of variants: White to move
calvar-title-black = Calculation of variants: Black to move

calvar-status-entering = Calculate in your head and type the lines you find. End each one with your evaluation.
calvar-status-ready = You may press "Done" now, or give more lines.
calvar-status-evaluating = The engine is evaluating your lines...
calvar-status-waiting-root = Waiting for the engine to finish its own analysis...
calvar-status-failed-engine = The engine could not evaluate the exercise.
calvar-status-failed-timed-out = The engine took too long and was dropped.
calvar-status-failed-time-up = Time is up and no line was given.

calvar-engine-thinking = Engine { $name }: thinking...
calvar-engine-ready = Engine { $name }: ready
calvar-lines-given = Lines: { $count } of { $required }
calvar-time-limit = limit { $count } s
calvar-line-in-progress = Line in progress: { $line }
calvar-no-line-in-progress = Type the moves of the next line
calvar-entry-placeholder = Moves, for example: Nf3 d5 d4
calvar-entry-tip = Type one or more moves in SAN or UCI and press Enter, with the piece letters N, B, R, Q, K. The board does not move.
calvar-rejected = Not a legal move here: { $text }
calvar-end-line = End the line with your evaluation:
calvar-eval-tip = End the line: { $symbol } ({ $meaning })
calvar-eval-black-winning = Black is winning
calvar-eval-black-better = Black is clearly better
calvar-eval-black-slightly = Black is slightly better
calvar-eval-equal = equal
calvar-eval-white-slightly = White is slightly better
calvar-eval-white-better = White is clearly better
calvar-eval-white-winning = White is winning

calvar-undo = Take back
calvar-undo-tip = Take back the last move of the line in progress
calvar-done = Done
calvar-done-tip = Ask the engine to evaluate your lines
calvar-close = Close
calvar-close-tip = End the exercise and get your game back
calvar-remove-line = Remove
calvar-remove-line-tip = Remove this line from the exercise
calvar-add-to-game = Add to the game
calvar-add-to-game-tip = Add your lines and the missed moves to the game as variants
calvar-new = New exercise
calvar-new-tip = Close this result and set up another exercise

calvar-result-score = Score: { $score } out of 100
calvar-result-found = Good moves found: { $found } of { $total }
calvar-result-all-found = You considered every good move.
calvar-result-crucial = Crucial move missed: { $move } ({ $eval })
calvar-result-missed = Other good moves you did not consider: { $moves }
calvar-result-parts = Candidates { $candidates }%, evaluations { $accuracy }%, lines { $quality }%{ $penalty ->
    [0] {""}
   *[other] , minus { $penalty } for the crucial move
}
calvar-result-line = { $line }: you said { $yours }, the engine says { $engine } ({ $eval }); { $plies } plies follow the engine; { $verdict }
calvar-result-line-unknown = { $line }: you said { $yours }; the engine could not evaluate this line
calvar-verdict-sound = the line holds
calvar-verdict-unsound = the line loses ground
calvar-verdict-weak = and its first move is not among the good moves

calvar-stats = Exercises: { $done }, mean score { $mean }, best { $best }; good moves found { $found } of { $good }
calvar-recent = Last scores: { $scores }
calvar-no-history = No exercise done yet

calvar-position = Position
calvar-position-current = Current position of the game
calvar-position-fen = FEN
calvar-fen-placeholder = Paste a FEN here
calvar-engine = Engine
calvar-engine-auto = Automatic
calvar-engine-none = No UCI engine in the list: add one in Tools > Engines
calvar-lines-label = Lines asked for
calvar-multipv-label = Engine lines at the start (MultiPV)
calvar-root-time-label = Engine thinking time at the start (seconds)
calvar-line-time-label = Time for each search of a line (seconds)
calvar-time-limit-label = Time limit
calvar-off = off
calvar-minutes = { $count } min
calvar-follow-board = Show the pieces of the line on the board
calvar-follow-board-tip = Off: the board stays still and you calculate in your head
calvar-reset = Clear history
calvar-reset-confirm = Click again to confirm
calvar-reset-tip = Clears the counters and the list of exercises
calvar-cancel = Cancel
calvar-start = Start

calvar-notice-busy = Finish the game, the tournament, the analysis mode or the training that is on first
calvar-notice-no-engine = No UCI engine of the list can be used
calvar-notice-bad-fen = The FEN is not valid
calvar-notice-no-moves = The position has no legal move
calvar-notice-illegal-move = A move was refused: it is not legal in the line you are calculating
calvar-notice-use-keyboard = That move is not legal in the line you are calculating: type it
calvar-notice-line-open = End the line in progress with your evaluation first
calvar-notice-need-more-lines = Give the lines that were asked for first
calvar-notice-empty-line = There is no move in the line in progress
calvar-notice-too-many-lines = The exercise has the most lines it can have
calvar-notice-line-too-long = The line has the most moves it can have
calvar-notice-locked = The board is locked while the exercise is on
calvar-notice-timed-out = The request took too long and was dropped
calvar-notice-engine-failed = The engine did not answer
calvar-notice-time-up = Time is up and there was no line to evaluate
calvar-notice-store-not-saved = The history could not be saved
calvar-notice-store-unreadable = The saved history was unreadable and was set aside
calvar-notice-store-newer = The saved history is from a newer version of the program and will not be changed
calvar-notice-cannot-add = Nothing can be added: the lines are already in the game, or the position is at its last move
calvar-notice-game-unreadable = The game could not be read
calvar-notice-no-problem = The tactics training has no problem on the board
calvar-notice-no-review = That exercise has no saved lines to review

calvar-origin-game = Position of game { $number }
calvar-origin-tactic = Position of the tactics problem
calvar-origin-review = Review of the exercise of { $date } (read only)
calvar-reading-game = Reading the game...
calvar-history-title = Past exercises
calvar-history-row = { $date }: { $score } points, { $found } of { $good } good moves, { $lines } lines
calvar-history-review = Review
calvar-history-review-tip = Show the lines, evaluations and analysis saved for this exercise, without starting an engine
calvar-history-old = (no saved lines)
calvar-from-tactics = Calculate variants
calvar-from-tactics-tip = End the training and calculate variants from the position of this problem

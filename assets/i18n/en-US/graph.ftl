# The score and time graph (spec M4-7). The equivalents of the Scid's texts
# are in the comments (`english.tcl`).

pane-graph = Game graph
menu-view-pane-graph = Game graph
menu-tools-graph = Score graph
action-toggle-graph-pane = Show or hide the score graph

# Auto, Score, Time, Score Combo, Time Combo
graph-kind-auto = Auto
graph-kind-score = Score
graph-kind-time = Time
graph-kind-score-combo = Score combo
graph-kind-time-combo = Time combo

# GraphOptionsBar "Highlight Current Move"
graph-highlight = Highlight current move
graph-highlight-tip = Marks the bar of the move on the board.
# GraphOptionsWhite / GraphOptionsBlack ("Invert White" / "Invert Black"):
# the flag names the side to move in the evaluated position.
graph-invert-white = Invert White
graph-invert-white-tip = Reverses the scores written after Black's moves (positions with White to move), for engines that report the side to move's point of view.
graph-invert-black = Invert Black
graph-invert-black-tip = Reverses the scores written after White's moves (positions with Black to move), for engines that report the side to move's point of view.

graph-empty = No scores or times in this game
graph-hover-score = { $move } { $value }
graph-hover-time = { $move } { $value } s
graph-style-line = Continuous line
graph-style-line-tip = Draws the scores as one curve with the advantage filled in; unchecked, one bar per move as in Scid.

# ---- classes of the game report (M4-10a) ----
report-class-brilliant = Brilliant
report-class-great = Great
report-class-best = Best
report-class-excellent = Excellent
report-class-good = Good
report-class-book = Book
report-class-inaccuracy = Inaccuracy
report-class-mistake = Mistake
report-class-miss = Miss
report-class-blunder = Blunder

# Game report panel (spec M4-10b)
menu-tools-report = Game report
pane-report = Report
report-side-white = White
report-side-black = Black
report-accuracy = Accuracy
report-preset-fast = Fast
report-preset-standard = Standard
report-preset-deep = Deep
report-generate = Generate report
report-regenerate = Analyze again
report-clear = Clear
report-empty = Analyze the whole game with the engine to see how each move compares with the best one. The game is not changed.
report-running = Analyzing the game with the engine... the game cannot be edited meanwhile.
report-partial = Partial report: { $done } of { $total } moves analyzed.
report-moments = Moves to look at again
report-moments-filtered = Moves of the chosen class
report-no-moments = No move to show.
report-moment-loss = -{ $loss } pts
report-estimate-note = Accuracy and classes are estimates from public win-probability models; they do not match any website.
report-progress = Analyzing... move { $done } of { $total }
report-progress-starting = Analyzing... starting the engine
annotate-notice-report-stopped = Report stopped after { $plies } of { $total } moves; the partial report was kept.
report-write = Write to game
report-write-tip = Writes the evaluations, the move marks and the best line of each fault into the game (one undo step).
report-from-game = Use the game's evaluations
report-from-game-tip = Builds the report from the [%eval] comments already in the game, without an engine. The best move is not known this way.
annotate-notice-report-written = Report written to the game: { $moves } moves judged, { $variations ->
    [one] { $variations } variation
   *[other] { $variations } variations
} added.
annotate-notice-report-no-evals = The game has no evaluations ([%eval]) to build a report from.
annotate-notice-report-continued-other-engine = The engine that began the report is gone: the analysis went on with another one, so the evaluations may differ.
annotate-notice-review-move-ignored = Look for the better move on the board: adding moves and variations is paused during the review.
annotate-notice-report-write-failed = The report could not be written into the game.
annotate-notice-report-kept = Run cancelled after { $plies } moves; the earlier report of { $kept } moves was kept.
report-review-start = Review the moves of:
report-review-both = Both
report-review-title = Moment { $index } of { $total }
report-review-looking-best = { $played } was played, { $class }: it loses about { $loss } points. The best move was { $best }.
report-review-looking = { $played } was played, { $class }: it loses about { $loss } points.
report-review-trying = Find a better move than { $played } on the board.
report-review-hint = Not that one. Hint: move the piece on { $square }.
report-review-solved = Correct! { $best } is the best move.
report-review-revealed = The best move was { $best }.
report-review-try = Try again
report-review-show-best = See the best move
report-review-previous = Previous
report-review-next = Next
report-review-exit = Exit
report-review-arrow = Show the arrow of the best move

# Report: settings, continue, user side, time left, board marks (M4-10 close)
report-configure = Configure...
report-continue = Continue analysis
report-continue-tip = Analyzes only the moves the stopped run did not reach; the ones already analyzed are kept.
report-progress-eta = Analyzing... move { $done } of { $total }, about { $time } left
report-user-side = I played:
report-user-none = Not set
report-user-ask = Choose your side to see your accuracy and your mistakes first.
report-side-you = { $side } (you)
report-review-keys = The Left and Right keys go to the previous and the next moment.
report-config-title = Configure report
report-config-limit-label = Limit of each search
report-config-limits = Classes of move: loss of win probability, in points
report-config-limit = { $class }: up to
report-config-last = Above the last limit a move is a { $class }.
report-config-marks = Show the class of the move on its square of the board
report-config-defaults = Restore defaults
report-config-note = Brilliant, Great, Book and Miss keep their own rules and are not changed here.

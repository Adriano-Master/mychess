# The game against an engine (spec M8-1b). The Scid's texts are in
# `english.tcl` (ComputerGame, UCIGame) and `sergame.tcl`.

action-play-engine = Play against the engine

play-title = Play against the engine
play-submit = Play
play-engine = Engine
play-no-engine = There is no UCI engine registered. Add one in the Engines dialog to play.
play-open-engines = Open Engines...
play-configure-engine = Configure engine...
play-configure-engine-tip = Open the Engines dialog to add or set up engines
play-color = You play
play-color-white = White
play-color-black = Black
play-color-random = Random
play-section-time = Time control
play-section-start = Start position
play-section-book = Opening book
play-section-strength = Engine strength
play-section-record = Record
play-store-clock = Store the clock in the move comments
play-mode = Mode
play-mode-timebonus = Clock with increment
play-mode-movetime = Time per move
play-mode-depth = Fixed depth
play-mode-nodes = Fixed nodes
play-white-minutes = White time
play-white-bonus = White increment
play-black-minutes = Black time
play-black-bonus = Black increment
play-depth = Depth
play-nodes = Nodes
play-movetime = Time per move
play-unit-minutes = minutes
play-unit-bonus = seconds per move
play-unit-plies = plies
play-unit-nodes = nodes
play-unit-seconds = seconds
play-range = ({ $min } to { $max })
play-start = Start from
play-start-new = New game
play-start-current = Current position
play-start-opening = Specific opening
play-openings-pick = Opening
play-openings-empty = No valid line yet. Write one per row below.
play-openings-help = One line per row, as "Name: moves", for example "Sicilian: 1.e4 c5 2.Nf3 d6". The engine plays the moves of the line.
play-book-use = Play from a book
play-book-file = Book file
play-book-choose = Choose file...
play-book-none = No file chosen
play-book-selection = Book move
play-book-weighted = Weighted random
play-book-best = Best move
play-book-dialog-title = Choose an opening book
play-book-filter = Polyglot books
play-strength-elo = Limit strength (UCI_Elo)
play-strength-skill = Skill Level
play-strength-none = This engine declares no strength options.
play-strength-unknown = The engine declares these options when it starts; they are used only if it supports them.

play-status-your-turn = Your turn
play-status-engine-thinking = The engine is thinking...
play-status-question = Waiting for your answer
play-status-engine-lost = The engine stopped answering
play-status-paused = Game paused
play-status-over = Game over
play-status-closed = Game closed
play-versus = Against { $name }
play-resign = Resign
play-resign-tip = Give up the game; it is saved as a loss
play-restart = Restart
play-restart-tip = Start again with the same settings
play-quit = Quit
play-quit-tip = Leave the game without a result and without saving
play-resume = Resume
play-resume-tip = Go on with the paused game
play-take-back = Take back
play-take-back-tip = Undo your last move and the engine's reply
play-offer-draw = Offer draw
play-offer-draw-tip = Propose a draw to the engine
play-yes = Yes
play-no = No

play-ask-book-title = Leave the book?
play-ask-book = Your move is not in the book, which still has moves for this position. Continue with it?
play-ask-line-title = Leave the line?
play-ask-line = Your move leaves the opening line. Continue with it?

play-end-title = Game over
play-end-won = You won.
play-end-lost = You lost.
play-end-draw = You drew.
play-end-no-result = The game ended without a result.
play-end-result = Result:
play-end-save = Save game
play-end-close = Close
play-end-read-only = The current database is read-only: the game is saved to a PGN file.

play-lost-title = Engine lost
play-lost-message = The engine is no longer answering. Go on with another engine or end the game.
play-lost-switch = Continue with { $name }
play-lost-end = End game

play-notice-already-running = A game against an engine is already on.
play-notice-no-engine = The engine is not available.
play-notice-not-uci = Only UCI engines can play.
play-notice-chess960-unsupported = The engine does not support Chess960 (no UCI_Chess960 option).
play-notice-busy = Finish the annotation, the training or the open dialogs first.
play-notice-locked = The game cannot be edited while the engine plays.
play-notice-unsaved-game = Save or discard the game in the editor first.
play-notice-book-unreadable = The book could not be read; playing without it.
play-notice-bad-opening = The specific opening has an illegal move.
play-notice-illegal-move = That move is illegal.
play-notice-not-your-turn = It is not your turn.
play-notice-move-taken-back = The move was taken back.
play-notice-nothing-to-take-back = There is no move to take back now.
play-notice-draw-not-allowed = A draw cannot be offered now.
play-notice-draw-declined = The engine declined the draw.
play-notice-engine-lost = The engine stopped answering; the game is paused.
play-notice-unsaved = The game is in the editor with its result; save it to keep it.
play-notice-end-checkmate = Checkmate.
play-notice-end-stalemate = Stalemate: draw.
play-notice-end-material = Draw: neither side can mate.
play-notice-end-fifty = Draw by the fifty-move rule.
play-notice-end-repetition = Draw by threefold repetition.
play-notice-end-timeout = Time forfeit.
play-notice-end-timeout-draw = Draw: time ran out but the opponent cannot mate.
play-notice-end-resignation = Resignation.
play-notice-end-draw-agreed = Draw by agreement.
play-notice-end-engine-failure = The engine played an illegal move or none; the game stopped without a result.

play-confirm-resign-title = Resign?
play-confirm-resign = You give up the game and it is saved as a loss.
play-confirm-restart-title = Restart?
play-confirm-restart = The game in progress is abandoned and a new one starts with the same settings.
play-confirm-quit-title = Quit the game?
play-confirm-quit = The game is left without a result and is not saved.
play-status-starting = Starting the engine...
play-start-dropped = The game was not started. Click Play again when you are ready.
play-restart-dropped = The game was not restarted. Click Restart again when you are ready.

# Ponder and coach (spec M8-1d).
play-section-help = Help and thinking
play-ponder = Let the engine think on my time (ponder)
play-ponder-unsupported = The engine must declare the Ponder option; otherwise this is ignored.
play-coach = Coach watches my moves (a second engine)
play-coach-engine = Coach engine
play-coach-no-engine = Add a second UCI engine in the Engines dialog (different from the opponent) to use a coach.
play-coach-depth = Coach depth
play-coach-seconds = Coach time per move
play-coach-hint = Show the better move when mine was weak
play-coach-numbers = Show the loss in pawns
play-pondering = Thinking on your time
play-coach-line = Coach: { $remark }
play-coach-busy = judging your move...
play-coach-wait = watching
play-coach-lost = unavailable (the engine stopped)
play-coach-best = best move
play-coach-good = good move
play-coach-inaccuracy = inaccuracy
play-coach-mistake = mistake
play-coach-blunder = blunder
play-coach-loss = { $word } (-{ $loss })
play-coach-better = { $text }, better was { $san }
play-notice-ponder-unsupported = The engine has no Ponder option; playing without pondering.
play-notice-coach-unavailable = The coach needs a UCI engine different from the opponent; playing without it.
play-notice-coach-lost = The coach engine stopped; the game goes on without it.
play-notice-coach-no-take-back = Taking back is off while the coach watches.

# The tactical game (spec M8-7).
menu-play-tactical = Tactical game...
action-tactical-game = Tactical game
play-tactical-title = Tactical game
play-tactical-section-strength = Strength (Elo)
play-tactical-section-start = Start position
play-tactical-elo = Elo of the engine
play-tactical-adjust = Change the Elo after each game
play-tactical-adjust-step = Step
play-tactical-plan-unknown = The strength is set when the game begins, from the options the engine declares.
play-tactical-plan-elo = The engine limits itself with UCI_Elo { $elo }.
play-tactical-plan-elo-clamped = The engine accepts UCI_Elo only from { $min } to { $max }: { $elo } will be used.
play-tactical-plan-skill = No UCI_Elo: Skill Level { $level } will be used (approximate).
play-tactical-plan-depth = No strength option: every search is cut at depth { $depth } (approximate).
play-tactical-stats-none = No games yet.
play-tactical-stats =
    { $count ->
        [one] { $count } game
       *[other] { $count } games
    }. Won: { $wins }, drawn: { $draws }, lost: { $losses } ({ $percent }%). Best streak: { $streak }. Highest Elo beaten: { $elo }.
play-tactical-reset = Reset the score
play-tactical-reset-confirm = Confirm?
play-tactical-start-new = New game
play-tactical-start-current = Current position
play-tactical-start-opening = Specific opening
play-tactical-start-problem = Problem of the tactics training
play-tactical-start-problem-none = Open a tactics problem first to use its position.
play-tactical-start-fen = FEN
play-tactical-start-fen-hint = Paste a FEN
play-tactical-fen-bad = This is not a valid FEN.
play-tactical-fen-ended = There is nothing to play in this position.
play-tactical-start-file = Random position from a file
play-tactical-start-base = Random position from the current database
play-tactical-file-choose = Choose...
play-tactical-file-none = No file chosen.
play-tactical-line =
    Tactical: { $plan } · score { $wins }-{ $draws }-{ $losses }
play-tactical-line-elo = engine at Elo { $elo }
play-tactical-line-skill = Skill Level { $level } (about Elo { $elo })
play-tactical-line-depth = depth up to { $depth } (about Elo { $elo })
play-tactical-next-elo = Elo of the next game: { $from } to { $to }.
play-notice-tactical-elo-clamped = The Elo is outside what the engine accepts; the closest value will be used.
play-notice-tactical-depth-limited = The engine has no strength option: its search is cut at a depth, so the strength is only approximate.
play-notice-tactical-pick-failed = Could not draw a position from that source.
play-notice-tactical-pick-empty = That source has no games.
play-notice-tactical-bad-position = There is nothing to play in that position.
play-status-picking = Drawing a position...

# Resign and Rematch for every mode of play (spec M8-12)
action-resign-game = Resign the game
action-rematch = Rematch
menu-game-resign = Resign
menu-game-rematch = Rematch
game-cmd-resign = Resign
game-cmd-resign-tip = Give up the game in progress
game-cmd-rematch = Rematch
game-cmd-rematch-tip = Play again with the colors swapped
play-rematch-dropped = The rematch did not start. Click Rematch again when you are ready.

# Premove against the engine (D4452)
play-premove = Premove
play-premove-tip = While the engine thinks, drag one of your pieces: the move is played when the engine answers, if it is still legal. Esc or right click cancels it.
play-premove-set = Premove set (Esc cancels it)

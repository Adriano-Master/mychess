# The tournament between engines (spec M8-2b). The Scid's texts are in
# `comp.tcl` (Computer Tournament) and `english.tcl`.

action-tournament-engines = Tournament between engines
menu-play-tournament = Tournament between engines...
menu-play-tournament-resume = Resume a tournament...
menu-play-tournament-panel = Tournament panel
pane-tournament = Tournament

tournament-title = Tournament between engines
tournament-yes = Yes
tournament-no = No
tournament-start = Start
tournament-resume = Resume a tournament...
tournament-new = New tournament...
tournament-none = There is no tournament. Start a new one or resume a saved one.

tournament-section-participants = Participants
tournament-section-tournament = Tournament
tournament-section-time = Time
tournament-section-opening = Openings
tournament-section-engines = Engines
tournament-section-adjudication = Adjudication
tournament-section-failures = Failures
tournament-section-recording = Recording

tournament-add = Add engine
tournament-duplicate = Duplicate
tournament-remove = Remove
tournament-configure-engines = Configure engines...
tournament-no-engine = There is no engine registered. Add engines in the Engines dialog.
tournament-engine-gone = (engine missing)
tournament-options-hint = Options: Name=Value; Name=Value

tournament-name = Name
tournament-site = Site
tournament-scheduling = Format
tournament-sched-round-robin = Round robin
tournament-sched-carousel = Carousel
tournament-sched-gauntlet = Gauntlet
tournament-tested = Tested engine
tournament-cycles = Games per pair

tournament-time-mode = Limit
tournament-time-clock = Clock with increment
tournament-time-movetime = Time per move
tournament-time-depth = Fixed depth
tournament-time-nodes = Fixed nodes
tournament-base = Base time
tournament-increment = Increment
tournament-movetime = Time per move
tournament-leeway = Tolerance
tournament-depth = Depth
tournament-nodes = Nodes
tournament-depth-limit = Ceiling per move
tournament-margin = Watchdog margin
tournament-unit-seconds = seconds
tournament-unit-percent = %
tournament-unit-plies = plies
tournament-unit-cp = centipawns
tournament-unit-mb = MB
tournament-estimate = { $games ->
    [one] { $games } game, at most { $time }
   *[other] { $games } games, at most { $time }
}

tournament-opening = Games start from
tournament-open-initial = Initial position
tournament-open-current = Current position
tournament-open-current-first = Current position, first game only
tournament-open-book = Book
tournament-open-suite = Suite of openings
tournament-book-file = Book file
tournament-book-pick = Book move
tournament-book-weighted = Weighted random
tournament-book-best = Best move
tournament-book-limit = Book plies (empty: no limit)
tournament-suite-file = Suite file
tournament-suite-order = Order
tournament-suite-sequential = In order
tournament-suite-random = Random
tournament-suite-plies = Plies taken from each game
tournament-pair-openings = Same opening for both games of a pair
tournament-seed = Seed (0: a new one)
tournament-choose-file = Choose file...
tournament-clear-file = Clear
tournament-no-file = No file chosen
tournament-suite-dialog-title = Choose a suite of openings
tournament-suite-filter = EPD and PGN files

tournament-ponder = Think on the opponent's time (ponder)
tournament-restart-each = Start the engines again before every game
tournament-hash = Hash (empty: as saved)
tournament-threads = Threads (empty: as saved)

tournament-adj-resign = Adjudicate a won game
tournament-adj-draw = Adjudicate a drawn game
tournament-adj-max-plies = Draw at a number of plies
tournament-adj-cp = Score
tournament-adj-moves = Moves in a row
tournament-adj-after = After move
tournament-adj-plies = Plies
tournament-adj-both = Both engines must agree
tournament-adj-tablebase = Adjudicate with the Syzygy tablebase
tournament-adj-tb-pieces = Up to this many pieces
tournament-unit-pieces = pieces

tournament-time-loss = A game lost on time
tournament-time-loss-loss = Counts as a loss
tournament-time-loss-replay = Is played again
tournament-retries = Times a broken game is played again
tournament-max-failures = Failures before the tournament waits for a decision

tournament-comment-score = Score comment
tournament-score-eval = [%eval]
tournament-score-pct = Bracketed
tournament-score-text = Plain text
tournament-score-none = None
tournament-comment-clock = Clock on each move
tournament-comment-time = Time of each move
tournament-claim-draws = Draw by repetition and 50 moves

tournament-resume-title = Resume a tournament
tournament-resume-none = There is no unfinished tournament.
tournament-resume-progress = { $played } of { $total } games

tournament-tab-live = Live
tournament-tab-pairings = Pairings
tournament-tab-standings = Standings
tournament-tab-cross = Crosstable
tournament-tab-log = Log

tournament-pause = Pause
tournament-continue = Continue
tournament-stop-after = Stop after this game
tournament-abort = End
tournament-leave = Leave
tournament-close = Close
tournament-restart = New tournament...
tournament-open-pgn = Open PGN as database
tournament-skip-current = Skip game
tournament-adj-white = White wins
tournament-adj-black = Black wins
tournament-adj-draw-result = Draw
tournament-progress = { $done } of { $total } games
tournament-summary-winners = Winners: { $winners }

tournament-phase-running = Playing
tournament-phase-pausing = Pausing after the move
tournament-phase-paused = Paused
tournament-phase-decision = Waiting for a decision
tournament-phase-stopped = Stopped after a game
tournament-phase-finished = Finished
tournament-phase-aborted = Ended

tournament-live-none = No game is being played.
tournament-live-preparing = Starting the engines...
tournament-live-game = Game { $number }, attempt { $attempt }

tournament-to-front = To the front
tournament-skip = Skip
tournament-redo = Play again
tournament-status-pending = Pending
tournament-status-playing = Playing
tournament-status-played = { $result } ({ $how })
tournament-status-replaced = Replaced: { $result }
tournament-status-skipped = Skipped
tournament-status-withdrawn = Withdrawn

tournament-term-normal = normal
tournament-term-time = time forfeit
tournament-term-rules = rules infraction
tournament-term-abandoned = abandoned
tournament-term-adjudication = adjudication
tournament-term-unterminated = unfinished

tournament-col-rank = #
tournament-col-name = Name
tournament-col-played = P
tournament-col-wins = W
tournament-col-draws = D
tournament-col-losses = L
tournament-col-points = Pts
tournament-col-percent = %
tournament-col-sb = SB
tournament-col-withdrawn = withdrawn

tournament-export = Export
tournament-export-csv = CSV
tournament-export-text = Text
tournament-export-markdown = Markdown
tournament-export-html = HTML
tournament-export-save = Save...
tournament-export-copy = Copy
tournament-file-standings = standings
tournament-file-cross = crosstable
tournament-save-dialog-title = Save the table

tournament-confirm-abort-title = End the tournament?
tournament-confirm-abort = The game in progress is dropped without a result. The games already played stay, and the tournament can be resumed later.
tournament-confirm-adjudicate-title = Decide the game?
tournament-confirm-adjudicate = The game in progress ends now: { $result }.
tournament-confirm-skip-title = Skip the game?
tournament-confirm-skip = The game in progress is dropped without a result and played again later.
tournament-confirm-close-title = Leave the tournament?
tournament-confirm-close = The engines end. The tournament is saved and can be resumed later.
tournament-confirm-quit-title = Quit with a tournament on?
tournament-confirm-quit = The engines end and the game in progress is lost. The tournament is saved and can be resumed later.

tournament-decision-title = An engine keeps failing
tournament-decision-body = { $name } failed too many times. What now?
tournament-decision-retry = Try again
tournament-decision-withdraw = Withdraw it
tournament-decision-replace = Replace it
tournament-decision-replace-with = Replace with
tournament-decision-later = Later
tournament-decision-reopen = Answer...

tournament-status-progress = Tournament { $name }: { $done } of { $total } games

tournament-fail-start = could not start
tournament-fail-died = stopped
tournament-fail-hung = did not answer
tournament-fail-time = lost on time

tournament-log-started = The tournament started.
tournament-log-game-started = Game { $number } started (attempt { $attempt }).
tournament-log-game-finished = Game { $number }: { $result } ({ $how }).
tournament-log-engine-failed = { $name } failed: { $reason }.
tournament-log-discarded = Game { $number } was thrown away.
tournament-log-withdrawn = { $name } left the tournament.
tournament-log-replaced = Another engine took the place of { $name }.
tournament-log-suite-wrapped = The suite of openings started again (game { $number }).
tournament-log-skipped = Game { $number } was skipped.
tournament-log-redo = Game { $number } will be played again.
tournament-log-paused = Paused.
tournament-log-resumed = Continued.
tournament-log-stopped = Stopped after a game.
tournament-log-aborted = The tournament was ended.
tournament-log-finished = The tournament is over.

tournament-notice-started = Tournament started.
tournament-notice-finished = Tournament over. Winners: { $winners }
tournament-notice-paused = Tournament paused.
tournament-notice-resumed = Tournament goes on.
tournament-notice-stopped = Tournament stopped after the game.
tournament-notice-aborted = Tournament ended.
tournament-notice-engine-failed = { $name } failed: { $reason }.
tournament-notice-too-many-failures = { $name } failed too many times: the tournament waits for a decision.
tournament-notice-not-applicable = That does not apply now.
tournament-notice-game-not-saved = A game could not be written to the PGN of the tournament.
tournament-notice-state-not-saved = The state of the tournament could not be saved.
tournament-notice-destination-read-only = The destination database is read only: the games go to the PGN only.
tournament-notice-play-busy = Finish the game against the engine first.
tournament-notice-already-running = A tournament is already on.
tournament-notice-busy = An annotation or a training mode is on.
tournament-notice-missing-engines = Engines not found: { $names }
tournament-notice-book-unreadable = The book could not be read: the games go on without it.
tournament-notice-suite-unreadable = The suite of openings could not be read.
tournament-notice-resume-failed = The saved tournament could not be read.
tournament-notice-tablebase-unavailable = The tablebase rule is on, but no Syzygy table could be opened: the games go on without it.
tournament-notice-exported = Table exported.
tournament-notice-export-failed = The table could not be written.

tournament-error-too-few = At least two participants are needed.
tournament-error-too-many = Too many participants (at most 32).
tournament-error-empty-name = Participant { $number } has no name.
tournament-error-duplicate-name = The name { $name } is used twice.
tournament-error-cycles = The number of games per pair is not valid.
tournament-error-tested = Choose the tested engine of the gauntlet.
tournament-error-time = The time control is not valid ({ $what }).
tournament-error-watchdog = The watchdog margin is not valid ({ $what }).
tournament-error-opening-data = The openings need more data ({ $what }).
tournament-error-start-fen = The position on the board cannot start a game.
tournament-error-book-limit = The number of book plies is not valid.
tournament-error-seed = The seed is not valid.
tournament-error-title-long = The name or the site is too long.
tournament-error-adjudication = An adjudication rule is not valid ({ $what }).
tournament-error-failure-limit = The limit of failures is not valid.
tournament-error-too-many-games = The schedule has too many games.
tournament-error-book-missing = The book was not found: { $path }
tournament-error-suite = The suite of openings cannot be read: { $path }

# M8-2c: games at once, Elo and LOS, time report, extra destination, XBoard.
tournament-concurrency = Games at once
tournament-concurrency-hint = advised { $recommended }, at most { $max } on this machine
tournament-estimate-parallel = { $games ->
    [one] { $games } game, { $lanes } at once, at most { $time }
   *[other] { $games } games, { $lanes } at once, at most { $time }
}
tournament-destination = Copy the games to
tournament-dest-none = Only the PGN
tournament-dest-slot = Current database
tournament-dest-clipbase = Clipbase

tournament-tab-ratings = Elo
tournament-tab-time = Time

tournament-col-elo = Elo
tournament-col-margin = Margin
tournament-col-los = LOS %
tournament-col-performance = Performance
tournament-col-sample = Sample
tournament-col-moves = Moves
tournament-col-mean = Mean
tournament-col-slowest = Slowest
tournament-col-overhead = Overhead
tournament-col-overruns = Over
tournament-col-forfeits = Flags
tournament-col-clock-used = Clock %
tournament-col-verdict = Verdict

tournament-sample-empty = no games
tournament-sample-very-uncertain = very uncertain
tournament-sample-low = low
tournament-sample-fair = fair

tournament-time-verdict-none = no data
tournament-time-verdict-ok = respects the clock
tournament-time-verdict-overruns = exceeds the time
tournament-time-verdict-forfeits = lost on time

tournament-ratings-note = Elo is relative to the opponents met (0 is a score of 50%), with a 95% margin. LOS is the likelihood of being the stronger one. With few games the numbers say little.
tournament-time-note = Response time is measured from the start of the search to the move. Overhead is the mean excess over a fixed time per move. An XBoard engine has no milliseconds: with less than one second per move it is given a whole second, so it may show as exceeding the time because of that. A game that was played again does not take back the numbers of the first one.

tournament-file-ratings = elo
tournament-file-time = time

tournament-notice-unsupported-time = { $name } speaks XBoard, which has no limit of nodes. Choose another limit.
tournament-notice-concurrency-reduced = Games at once brought from { $from } down to { $to }, what this machine takes.
tournament-notice-destination-failed = A game could not be copied to the chosen database. The PGN of the tournament has it.

tournament-error-concurrency = Games at once must be between 1 and 16.
tournament-notice-destination-not-reopened = The game was copied to the file of the chosen database, but the database could not be opened again. Close it and open it to see the game.
tournament-notice-destination-unavailable = The chosen database is not available (it was closed or changed, or the tournament was resumed): the games go to the PGN only.

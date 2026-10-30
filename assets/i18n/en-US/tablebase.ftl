# Tablebases (spec M8-8). The Scid has them in `tcl/tools/tablebase.tcl`
# (Nalimov and online); MyChess uses Syzygy files and the Lichess tablebase.
action-toggle-tablebase-pane = Show or hide the tablebase panel
menu-tools-tablebase = Tablebases
pane-tablebase = Tablebases
pane-close-tablebase = Close the tablebase panel
file-dialog-tablebase-dir = Choose a folder with Syzygy tablebase files

tablebase-source-local = Local (Syzygy)
tablebase-source-online = Online (Lichess)
tablebase-position-result = Position: { $result }
tablebase-counts = Wins { $wins }, draws { $draws }, losses { $losses }, unknown { $unknown }
tablebase-no-moves = The game is over in this position.

tablebase-col-move = Move
tablebase-col-result = Result
tablebase-col-dtz = DTZ
tablebase-col-dtm = DTM
tablebase-dtz-tip = Distance to zeroing: plies until the next capture or pawn move, with the result kept. It is not the number of moves to mate.
tablebase-dtm-tip = Distance to mate. Syzygy has no such value; only the online source gives it for some positions.
tablebase-mate-in = #{ $moves }
tablebase-best-tip = Best move

tablebase-result-win = Win
tablebase-result-maybe-win = Win?
tablebase-result-cursed-win = Cursed win
tablebase-result-draw = Draw
tablebase-result-blessed-loss = Blessed loss
tablebase-result-maybe-loss = Loss?
tablebase-result-loss = Loss
tablebase-result-unknown = Unknown
tablebase-result-cursed-win-tip = A win, but the 50-move rule makes it a draw.
tablebase-result-blessed-loss-tip = A loss, but the 50-move rule makes it a draw.
tablebase-result-maybe-tip = The distance is rounded: the 50-move clock decides.

tablebase-status-not-configured = No tablebase is turned on. Turn on the local Syzygy files or the online tablebase below.
tablebase-status-locked = Tablebases are not available during an online game.
tablebase-status-loading = Reading the tablebase folders...
tablebase-status-waiting = Looking up the position...
tablebase-status-stale = Showing the previous position.

tablebase-issue-no-tables = No Syzygy file was found in the folders.
tablebase-issue-too-many-pieces = { $pieces } pieces: the tablebase covers up to { $max }.
tablebase-issue-too-few-pieces = Two kings only: nothing to look up.
tablebase-issue-castling = A castling right is left: tablebases do not cover it.
tablebase-issue-missing-table = The table { $material } is not available.
tablebase-issue-corrupt = The table { $material } is corrupted.
tablebase-issue-network = No connection to the tablebase server.
tablebase-issue-timed-out = The tablebase took too long to answer.
tablebase-issue-rate-limited = The server asks for a pause: try again in { $seconds } s.
tablebase-issue-server = The tablebase server failed.
tablebase-issue-invalid = The server's answer could not be understood.
tablebase-issue-internal = The tablebase failed unexpectedly.

tablebase-config = Settings
tablebase-config-tip = Show or hide the settings of the tablebases
tablebase-config-local = Local tablebases (Syzygy files)
tablebase-config-online = Online tablebase (Lichess)
tablebase-config-online-privacy = Turning this on sends the position (FEN) to tablebase.lichess.ovh. Nothing is sent while it is off.
tablebase-config-folders = Folders with .rtbw and .rtbz files
tablebase-config-folder-placeholder = Type or paste a folder
tablebase-config-add = Add
tablebase-config-browse = Browse...
tablebase-config-remove = Remove
tablebase-config-reload = Read again
tablebase-config-dir-counts = { $wdl } WDL, { $dtz } DTZ
tablebase-config-dir-unreadable = not found
tablebase-config-dir-uppercase =
    { $count ->
        [one] { $count } file has an upper-case extension (.RTBW or .RTBZ) and is ignored: rename it to lower case
       *[other] { $count } files have an upper-case extension (.RTBW or .RTBZ) and are ignored: rename them to lower case
    }
tablebase-config-dir-bad =
    { $count ->
        [one] { $count } unusable file
       *[other] { $count } unusable files
    }
tablebase-config-summary = Files: WDL { $wdl }, DTZ { $dtz }; largest table: { $pieces } pieces
tablebase-config-help = The tables are not included. Download them from tablebase.lichess.ovh/tables (3-4-5 pieces: about 1 GB; 6 pieces: about 150 GB) and choose their folder here.

tablebase-train = Train
tablebase-train-tip = Play this position against the tablebase; the results are hidden
tablebase-train-stop = Stop training
tablebase-train-new = New position
tablebase-train-new-tip = Replace the game by a random position of this material and train
tablebase-train-material-placeholder = Material, for example KRvKP
tablebase-train-go = Go
tablebase-train-status =
    Training with { $side }: { $moves ->
        [one] { $moves } move
       *[other] { $moves } moves
    }, { $slips ->
        [one] { $slips } slip
       *[other] { $slips } slips
    }. Starting position: { $grade }.
tablebase-train-thinking = The tablebase is answering...
tablebase-train-your-move = Your move.
tablebase-side-white = White
tablebase-side-black = Black
tablebase-grade-win = a win
tablebase-grade-draw = a draw
tablebase-grade-loss = a loss
tablebase-verdict-converted = You converted the win.
tablebase-verdict-held = You held the draw.
tablebase-verdict-escaped = You got more than the position was worth.
tablebase-verdict-lost-as-expected = The position was lost and the tablebase won.
tablebase-verdict-failed = You let the result slip.
tablebase-verdict-inconclusive = The training ended without a result.
tablebase-end-checkmate = Checkmate.
tablebase-end-stalemate = Stalemate.
tablebase-end-insufficient-material = Draw: no material to mate with.
tablebase-end-fifty-moves = Draw by the 50-move rule.
tablebase-end-repetition = Draw by repetition.
tablebase-end-out-of-range = The position left the tablebase.
tablebase-outcome-summary =
    { $moves ->
        [one] { $moves } move
       *[other] { $moves } moves
    }, { $slips ->
        [one] { $slips } slip
       *[other] { $slips } slips
    }.
tablebase-dismiss = Close

tablebase-notice-busy = Finish the game, the tournament, the trainer or the dialog that is open first
tablebase-notice-not-at-end = Go to the end of the line to train
tablebase-notice-nothing-to-train = Turn on a tablebase and use a position that is not over
tablebase-notice-bad-material = That is not a material such as KRvKP
tablebase-notice-folder-not-saved = That folder name cannot be kept in the settings
tablebase-notice-too-many-folders = The list of folders is full

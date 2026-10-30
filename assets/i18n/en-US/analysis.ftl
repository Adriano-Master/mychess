# The analysis panel (spec M4-4): one section per running engine. The
# equivalents of the Scid's texts are in the comments (`english.tcl`).

pane-analysis = Analysis
menu-view-pane-analysis = Analysis
action-toggle-analysis-pane = Show or hide the analysis window

analysis-empty = No engine is running. Start one with Tools > Engines...

# ---- state of a section ----
analysis-state-starting = starting...
analysis-state-restarting = restarting...
analysis-state-stopping = stopping...
analysis-state-stopped = stopped
analysis-state-checkmate = (checkmate)
analysis-state-stalemate = (stalemate)
analysis-state-failed = failed: { $reason }

# ---- buttons (label, then tooltip; the tooltips are the Scid's) ----
analysis-tip-start = Start engine
analysis-tip-stop = Stop engine
analysis-btn-add-move = + Move
analysis-tip-add-move = Add move (right click: add score)
analysis-btn-add-line = + Line
analysis-tip-add-line = Add variation (right click: second line)
analysis-btn-add-all = + All
analysis-tip-add-all = Add all variations (right click: second line only; Ctrl-click: first moves only)
analysis-tip-add-this-line = Add this line
analysis-lines = Lines
analysis-tip-lines = Number of lines
analysis-btn-lock = Lock
analysis-btn-locked = Locked
analysis-tip-lock = Lock engine to current position
analysis-tip-unlock = Locked on { $description }. Click to unlock.
analysis-btn-exclude = Exclude
analysis-tip-exclude = Exclude move
analysis-tip-excluded = Excluded moves: { $moves }
analysis-btn-priority = Low CPU
analysis-tip-priority = Low CPU priority (restarts the engine)
analysis-btn-info = Info
analysis-tip-info = Show engine info
analysis-btn-display-hidden = Hidden
analysis-btn-display-nowrap = No wrap
analysis-btn-display-wrap = Wrap
analysis-tip-display = How the lines are shown: hidden, no wrap, wrap (middle click)
analysis-btn-stats = Update
analysis-tip-stats = Ask the engine for its statistics
analysis-btn-log = Log
analysis-tip-log = Engine log
analysis-tip-close = Close this engine
analysis-unsupported = Not supported by this engine.
analysis-unavailable-locked = Not available while the position is locked.
analysis-unavailable-no-lines = There are no lines yet.

# ---- lines ----
analysis-click-to-see = Click here to see the moves
analysis-history-line = { $depth } [{ $score }]  { $moves } ({ $time } s)
analysis-locked-hint = Locked on { $description }
analysis-lock-start = start position
analysis-lock-file-game = { $file }, game { $game }, { $move }
analysis-lock-game = game { $game }, { $move }
analysis-lock-vardepth =  (vardepth { $depth })

# ---- information lines ----
analysis-info-main = { $score }  Depth: { $depth }  Nodes: { $nodes }  Time: { $time } s
analysis-info-extra = NPS: { $nps }  Hash: { $hash }  Load: { $load }  TB: { $tb }  Current: { $current }

# ---- exclusion box ----
analysis-exclude-title = Exclude moves
analysis-exclude-help = Moves in SAN, separated by spaces or commas. Leave empty to clear.
analysis-exclude-placeholder = e.g. Nf3, d4

# ---- engine log ----
analysis-log-auto = Auto
analysis-log-update = Update
analysis-log-find = Find
analysis-log-save = Save as...
analysis-log-close = Close
analysis-log-empty = The log is empty.
analysis-log-save-title = Save the engine log
analysis-log-filter = Log files

# ---- remarks of the status bar ----
analysis-notice-at-game-end = This position is the end of the game: use Add move or Add score.
analysis-notice-line-truncated = The line has a move that cannot be played; { $count ->
    [one] { $count } move was added.
   *[other] { $count } moves were added.
}
analysis-notice-exclude-invalid = "{ $token }" is not a legal move here.
analysis-notice-exclude-all = At least one move must be left to search.
analysis-notice-exclude-unsupported = This engine cannot exclude moves.
analysis-notice-multipv-unsupported = This engine has no MultiPV option.
analysis-notice-locked = Not available while the analysis is locked.
analysis-notice-log-saved = The engine log was saved.

# ---- M4-5: arrows, evaluation bar and traffic lights ----
analysis-btn-arrows = Arrows
analysis-tip-arrows = Show this engine's arrows on the board (click again to hide)
analysis-arrow-mode-arrows = Arrows
analysis-arrow-mode-sequence = Sequence
analysis-arrow-seq-both = Sequence: both sides
analysis-arrow-seq-white = Sequence: White's moves
analysis-arrow-seq-black = Sequence: Black's moves
analysis-arrow-seq-len = Moves in the sequence
action-toggle-eval-bar = Show or hide the evaluation bar
action-toggle-traffic-lights = Show or hide the MultiPV traffic lights
action-toggle-arrow-numbers = Number or not the engine arrows

# ---- M4-6 and M4-8: annotation and engine modes ----
analysis-btn-annotate = Annotate
analysis-tip-annotate = Annotate the game with this engine
analysis-tip-annotate-stop = Stop the annotation (the edits are kept)
# FinishGame, Training
analysis-btn-threat = Threat
analysis-tip-threat = Threat analysis: passes the move (null move) and analyses the position (click again to leave)
analysis-tip-threat-check = There is no threat in check
analysis-btn-finish-game = Finish game
analysis-tip-finish-game = The engine plays both sides until the game ends
analysis-btn-training = Training
analysis-tip-training = The engine answers each of your moves
analysis-threat-label = Opponent's threat:
analysis-training-thinking = Thinking.....
analysis-training-your-move = Your move.....
# AnnotateTime ("Seconds per Move"), "Engine thinking time"
analysis-training-title = Engine thinking time
analysis-training-help = The engine answers each of your moves after this time (0.1 to 300 s; other values are adjusted).
analysis-training-seconds = Seconds per move
analysis-training-start = Start
analysis-tip-commandeered = Not available while the engine plays
analysis-mode-conflict = Another mode, the autoplay or an annotation was in the way: only one runs at a time.
analysis-mode-trial = Leave trial mode first.
analysis-mode-shootout-busy = The engine is playing the game: stop it before editing.
analysis-mode-ended-checkmate = Stopped: checkmate.
analysis-mode-ended-stalemate = Stopped: stalemate.
analysis-mode-ended-insufficient-material = Stopped: insufficient material.
analysis-mode-ended-fifty-moves = Stopped: fifty-move rule.
analysis-mode-ended-repetition = Stopped: threefold repetition.
analysis-mode-ended-no-move = Stopped: the engine gave no move.
analysis-mode-ended-engine-error = Stopped: the engine failed or gave moves that cannot be played.
analysis-mode-ended-user-stopped = Stopped.
analysis-mode-ended-game-changed = Stopped: the game was changed.

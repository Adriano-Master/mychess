# Automatic annotation (spec M4-6): the "Configure annotation" dialog, the
# progress and the remarks. The Scid's texts (`english.tcl`) are in the
# comments.

# AnnotateTitle
annotate-title = Configure annotation
annotate-start = Start
annotate-stop = Stop
annotate-engine = Engine
annotate-engine-default = Automatic
# MoveControl, Depth, Time
annotate-control = Move control
annotate-control-depth = Depth
annotate-control-time = Time
annotate-xboard-note = This engine can only be limited by time.
# DepthPerMove, SecondsPerMove
annotate-depth = Depth per move
annotate-seconds = Seconds per move
# Blunder ("Threshold"), CutOff
annotate-threshold = Threshold
annotate-cutoff = Cut off
# AddScores, AnnotateAllMoves, BlundersNotBest, Variations, No
annotate-add-score = Add score
annotate-score-all = All moves
annotate-score-blunders = Blunders/Not best
annotate-score-variations = Variations
annotate-score-no = No
# AddVars, AnnotateBlundersOnly, AnnotateNotBest, AnnotateAllMoves
annotate-add-vars = Add variations
annotate-vars-blunders = When move is a blunder
annotate-vars-notbest = When move is not the best
annotate-vars-all = All moves
annotate-vars-no = No
# AnnotateMissedMates, AnnotateEnd
annotate-missed-mates = Missed/shorter mates
annotate-end = At end of game
# AnnotateWhich, AnnotateAll, AnnotateWhite, AnnotateBlack
annotate-side = Which side
annotate-side-all = Both
annotate-side-white = White
annotate-side-black = Black
# ScoreFormat
annotate-score-format = Score format
annotate-format-plain = +1.5
annotate-format-bracket = [% +1.5]
annotate-format-eval = [%eval +1.5]
# UseBook (disabled until the books of M5)
annotate-use-book = Use book
annotate-book-needs-book = Choose an opening book in Tools > Opening books
# FindOpeningErrors, addAnnotatorTag, AnnotateVariations, MarkTacticalExercises
annotate-opening-only = Opening errors only
annotate-opening-moves = Moves of the opening
annotate-batch = Annotate every game of the filter (saves in the database)
annotate-add-tag = Add annotator tag
annotate-variations = Process variations
annotate-exercises = Mark tactical exercises
# ConfigureInformant and Informant!?, ?, ??, ?!, +=, +/-, +-, ++-
annotate-informant-show = Configure informant values...
annotate-informant-hide = Hide informant values
annotate-informant-defaults = Defaults
annotate-informant-interesting = Interesting move (!?)
annotate-informant-poor = Poor move (?)
annotate-informant-blunder = Blunder (??)
annotate-informant-dubious = Dubious move (?!)
annotate-informant-slight = Slight advantage (+=)
annotate-informant-moderate = Moderate advantage (+/-)
annotate-informant-decisive = Decisive advantage (+-)
annotate-informant-won = Won game (++-)
# The run
annotate-estimate-moves =
    { $moves ->
        [one] { $moves } move to annotate
       *[other] { $moves } moves to annotate
    }
annotate-estimate-time = Estimated time: { $minutes } min { $seconds } s
annotate-progress = Annotating... move { $done } of { $total }
annotate-progress-starting = Annotating... starting the engine
annotate-progress-batch = Annotating game { $game } of { $games }... move { $done } of { $total }
annotate-progress-batch-starting = Annotating game { $game } of { $games }... starting

# ---- remarks of the status bar ----
annotate-notice-already-running = An annotation is already running.
annotate-notice-no-engine = No engine is running and none is set for F2.
annotate-notice-trial-mode = Leave trial mode before annotating.
annotate-notice-nothing-to-annotate = There is no move to annotate from here.
annotate-notice-busy = The game cannot be edited while it is being annotated. Press Esc to stop.
annotate-notice-finished =
    Annotation finished: { $plies ->
        [one] { $plies } move
       *[other] { $plies } moves
    }, { $variations ->
        [one] { $variations } variation
       *[other] { $variations } variations
    }.
annotate-notice-cancelled = Annotation stopped after { $plies ->
    [one] { $plies } move
   *[other] { $plies } moves
}; the edits were kept.
annotate-notice-game-changed = The game changed during the annotation; it stopped after { $plies ->
    [one] { $plies } move
   *[other] { $plies } moves
}.
annotate-notice-engine-failed = The engine failed; the annotation stopped after { $plies ->
    [one] { $plies } move
   *[other] { $plies } moves
}.
annotate-notice-no-score = The engine gave no score; the annotation stopped after { $plies ->
    [one] { $plies } move
   *[other] { $plies } moves
}.
annotate-notice-batch-unavailable = Batch annotation needs an editable Scid database (.si4) in the current slot.
annotate-notice-batch-dirty = Save or discard the current game before annotating a batch.
annotate-notice-batch-finished = Batch finished: { $games ->
    [one] { $games } game annotated and saved
   *[other] { $games } games annotated and saved
}, { $skipped } without moves.
annotate-notice-batch-stopped = Batch stopped: { $games } of { $total } games annotated and saved.

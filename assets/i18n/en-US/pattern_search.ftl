# Search for tactics and patterns (spec M6-2, slice d). The sentences of the
# cards are built from a key plus arguments (see the rustdoc of
# `mychess_app::Sentence`): a region is written with its own preposition and
# the front-end drops "anywhere" from inside a sentence.

menu-search-pattern = Tactics and patterns...

pattern-title = Tactics and patterns
pattern-tab-presets = Ready-made
pattern-tab-advanced = Custom
pattern-presets-hint = Pick a tactic and press Search.
pattern-customize = Customize
pattern-customize-tip = Copy this pattern to the Custom tab to change it
pattern-run = Search
pattern-stop = Stop
pattern-stop-tip = Stop the search; the filter stays as it was
pattern-close = Close
pattern-hide = Show the board
pattern-hide-tip = Hide this window to see the occurrences on the board
pattern-show = Tactics and patterns
pattern-show-tip = Open the search window again
pattern-test-game = Test on this game
pattern-test-game-tip = Run the pattern on the game on the board only, without touching the filter
pattern-test-result = { $count ->
    [one] Holds at one position of this game
   *[other] Holds at { $count } positions of this game
}
pattern-test-none = Does not hold anywhere in this game

# Scope and execution.
pattern-scope-title = Where to look
pattern-op = Filter setting
pattern-op-reset = Search all games
pattern-op-and = Restrict the filter
pattern-op-or = Add to the filter
pattern-reference = Search in database
pattern-reference-none = Choose a database
pattern-move-range = Move number
pattern-hold = Must hold for at least
pattern-hold-unit = { $count ->
    [one] half-move
   *[other] half-moves
}
pattern-end-only = Final position only
pattern-ignore-colors = Ignore colors
pattern-progress = { $done } of { $total } games
pattern-summary = { $matched } / { $start } games ({ $secs } s)
pattern-interrupted = Search interrupted
pattern-unknown = { $count ->
    [one] One position too complex to check
   *[other] { $count } positions too complex to check
}
pattern-unreadable = { $count ->
    [one] One unreadable game
   *[other] { $count } unreadable games
}
pattern-cost-title = Long search
pattern-cost-warning = This search may take a long time (about { $positions } positions in { $games } games). Continue?
pattern-cost-yes = Search anyway
pattern-cost-no = Cancel

# Occurrences.
pattern-occurrence = Occurrence { $n } of { $total }
pattern-occurrence-test = Test: occurrence { $n } of { $total }
pattern-hit-prev = Previous occurrence
pattern-hit-next = Next occurrence
pattern-hit-clear = Forget the occurrences
pattern-hit-none = No occurrence in this game

# Presets.
pattern-preset-mate-in = Mate in { $n }
pattern-preset-pin = Pin
pattern-preset-fork = Fork
pattern-preset-skewer = Skewer
pattern-preset-discovered = Discovered attack
pattern-preset-double-attack = Double attack
pattern-preset-sacrifice = Sacrifice
pattern-preset-hanging = Hanging piece
pattern-preset-passed-pawn = Passed pawn
pattern-preset-exposed-king = Exposed king
pattern-preset-back-rank = Back-rank mate
pattern-preset-promotion = Promotion
pattern-preset-iqp = Isolated queen's pawn (White)
pattern-preset-iqp-black = Isolated queen's pawn (Black)
pattern-preset-iqp-both = Isolated queen's pawns (both)
pattern-preset-hanging-pawns = Hanging pawns
pattern-preset-maroczy = Maróczy bind
pattern-preset-fianchetto-light = Fianchetto (light-squared bishop)
pattern-preset-fianchetto-dark = Fianchetto (dark-squared bishop)
pattern-preset-fianchetto-four = Double fianchetto
pattern-preset-opposite-castling = Opposite-side castling
pattern-preset-mate-in-desc = A side has a mate in the number of moves chosen.
pattern-preset-pin-desc = A piece cannot move without exposing a more valuable one.
pattern-preset-fork-desc = A piece attacks two targets worth taking at once.
pattern-preset-skewer-desc = A valuable piece is attacked and must move, exposing the one behind it.
pattern-preset-discovered-desc = Moving a piece opens the line of another one.
pattern-preset-double-attack-desc = A move attacks two targets at the same time.
pattern-preset-sacrifice-desc = Material is given up and then recovered, or ends in mate.
pattern-preset-hanging-desc = A piece is attacked and nobody defends it.
pattern-preset-passed-pawn-desc = A pawn with no enemy pawn in front of it or beside its path.
pattern-preset-exposed-king-desc = The king has lost the cover of its pawns.
pattern-preset-back-rank-desc = Checkmate on the first rank, with the king boxed in by its own pawns.
pattern-preset-promotion-desc = A pawn reaches the last rank and becomes another piece.
pattern-preset-iqp-desc = White pawn on d4 with no White pawn on the c and e files.
pattern-preset-iqp-black-desc = Black pawn on d5 with no Black pawn on the c and e files.
pattern-preset-iqp-both-desc = Isolated queen's pawns on both sides.
pattern-preset-hanging-pawns-desc = Pawns side by side on c and d, with no pawns on b and e.
pattern-preset-maroczy-desc = White pawns on c4 and e4, with no White pawn on the d file.
pattern-preset-fianchetto-light-desc = The light-squared bishop on the long diagonal.
pattern-preset-fianchetto-dark-desc = The dark-squared bishop on the long diagonal.
pattern-preset-fianchetto-four-desc = Both bishops on the long diagonals.
pattern-preset-opposite-castling-desc = One king castled long, the other short.

# Parameters of the presets.
pattern-param-by = Who
pattern-param-n = Moves to mate
pattern-param-mate-mode = Mate
pattern-param-max-nodes = Search effort (positions)
pattern-param-piece = Piece
pattern-param-target-piece = Target piece
pattern-param-against-king = Pinned against the king
pattern-param-discovery = Kind
pattern-param-min-net = Material given up (at least)
pattern-param-outcome = Then
pattern-param-within = Moves to recover
pattern-param-require-win = Only in games the side won
pattern-param-min-own-rank = Advanced to rank
pattern-param-rank-any = Any
pattern-param-other = Option
pattern-mate-mode-played = Mate that was played
pattern-mate-mode-available = Any mate that was available
pattern-mate-forcing-only = Only mates that start with a check
pattern-discovery-attack = Attack
pattern-discovery-check = Check
pattern-discovery-double-check = Double check
pattern-any-piece-hint = No piece chosen means any piece.

# Values.
pattern-who-white = White
pattern-who-black = Black
pattern-who-any = Either side
pattern-who-mover = Side to move
pattern-who-opponent = Other side
pattern-role-king = king
pattern-role-queen = queen
pattern-role-rook = rook
pattern-role-bishop = bishop
pattern-role-knight = knight
pattern-role-pawn = pawn
pattern-role-any = any piece
pattern-role-minor = minor piece
pattern-phase-opening = opening
pattern-phase-middlegame = middlegame
pattern-phase-endgame = endgame
pattern-status-check = in check
pattern-status-checkmate = checkmated
pattern-status-stalemate = stalemated
pattern-result-white-wins = White wins
pattern-result-draw = draw
pattern-result-black-wins = Black wins
pattern-result-unknown = unknown result
pattern-flag-yes = yes
pattern-flag-no = no
pattern-flag-both = either
pattern-castle-none = none
pattern-castle-short = short
pattern-castle-long = long
pattern-castle-any = any
pattern-gain-any = any attack
pattern-gain-undefended = only if undefended
pattern-gain-wins-material = only if it wins material
pattern-when-any = at any time
pattern-when-actor-just-moved = right after the piece moved
pattern-when-actor-to-move = when its side is to move
pattern-outcome-recovered = the material is recovered
pattern-outcome-mate = it ends in mate
pattern-outcome-either = the material is recovered or it ends in mate
pattern-outcome-any = nothing else required
pattern-bishops-either = Any bishops
pattern-bishops-same = bishops on the same color
pattern-bishops-opposite = bishops on opposite colors
pattern-list-and = { $head } and { $last }
pattern-list-or = { $head } or { $last }

# Regions.
pattern-region-anywhere = anywhere
pattern-region-kind-anywhere = Anywhere
pattern-region-kind-squares = Squares
pattern-region-kind-file = File
pattern-region-kind-rank = Rank
pattern-region-kind-own-rank = Own rank
pattern-region-kind-rect = Rectangle
pattern-region-kind-named = Area
pattern-region-pick = Click squares to choose them
pattern-region-use-marks = Use marked squares
pattern-region-clear = Clear
pattern-region-from = From
pattern-region-to = To

# The pieces of the sentences of the cards.
pattern-ref-actor = { $piece } ({ $side }){ $where }
pattern-ref-target = { $piece }{ $where }
pattern-material-count =
    { $piece } ({ $side }): { $same ->
        [1] { $min }
       *[other] { $min } to { $max }
    }
pattern-material-diff =
    difference (White - Black): { $same ->
        [1] { $min }
       *[other] { $min } to { $max }
    }

# The sentences of the cards.
pattern-sentence-piece = { $side }: { $piece }{ $where }
pattern-sentence-piece-none = { $side }: { $piece }{ $where } (count: 0)
pattern-sentence-piece-exactly = { $side }: { $piece }{ $where } (count: { $count })
pattern-sentence-piece-between = { $side }: { $piece }{ $where } (count: { $min } to { $max })
pattern-sentence-piece-at-least = { $side }: { $piece }{ $where } (count: at least { $min })
pattern-sentence-material = Material: { $parts }
pattern-sentence-material-any = Material: any
pattern-sentence-side-to-move = Side to move: { $side }
pattern-sentence-phase = Phase: { $phase }
pattern-sentence-status = Side to move is { $status }
pattern-sentence-move-previous = Previous move: { $by }, { $piece }{ $from }{ $to }; capture: { $capture }; check: { $check }; promotion: { $promotion }; castling: { $castling }
pattern-sentence-move-next = Next move: { $by }, { $piece }{ $from }{ $to }; capture: { $capture }; check: { $check }; promotion: { $promotion }; castling: { $castling }
pattern-sentence-result = Game result: { $results }
pattern-sentence-passed-pawn = { $side }: passed pawn{ $where } (at least { $min })
pattern-sentence-isolated-pawn = { $side }: isolated pawn{ $where } (at least { $min })
pattern-sentence-doubled-pawns = { $side }: doubled pawns{ $where } (at least { $min })
pattern-sentence-exposed-king = { $side }: exposed king{ $where }
pattern-sentence-bishop-pair = { $side }: has the bishop pair
pattern-sentence-attacks =
    { $actor } attacks { $target } ({ $gain }; { $when }{ $new ->
        [1] ; created by the last move
       *[other] { "" }
    })
pattern-sentence-defends =
    { $actor } defends { $target } ({ $when }{ $new ->
        [1] ; created by the last move
       *[other] { "" }
    })
pattern-sentence-pin =
    { $actor } pins { $target } against { $behind } ({ $when }{ $new ->
        [1] ; created by the last move
       *[other] { "" }
    })
pattern-sentence-skewer =
    { $actor } skewers { $target } in front of { $behind } ({ $when }{ $new ->
        [1] ; created by the last move
       *[other] { "" }
    })
pattern-sentence-fork =
    { $actor } forks { $count } or more of: { $target } ({ $when }{ $new ->
        [1] ; created by the last move
       *[other] { "" }
    })
pattern-sentence-discovered-attack = { $by }: discovered attack (piece moved: { $piece })
pattern-sentence-discovered-check = { $by }: discovered check (piece moved: { $piece })
pattern-sentence-double-check = { $by }: double check (piece moved: { $piece })
pattern-sentence-double-attack = { $by }: double attack
pattern-sentence-sacrifice = { $by }: sacrifices { $piece } (net loss of at least { $net }); { $outcome } within { $within } { $within ->
    [one] move
   *[other] moves
}
pattern-sentence-mate =
    { $by }: mate in { $same ->
        [1] { $min }
       *[other] { $min } to { $max }
    } { $max ->
        [one] move
       *[other] moves
    } ({ $mode }{ $forcing ->
        [1] ; { pattern-mate-forcing-only }
       *[other] { "" }
    })

# The editor.
pattern-add-condition = Add condition
pattern-cat-logic = Groups
pattern-cat-pieces = Pieces and squares
pattern-cat-relations = Relations
pattern-cat-tactics = Tactics
pattern-cat-structure = Structure and king safety
pattern-cat-move = Move and state
pattern-group-all = All of these (AND)
pattern-group-any = Any of these (OR)
pattern-group-not = Not this (NOT)
pattern-group-short-all = AND
pattern-group-short-any = OR
pattern-group-short-not = NOT
pattern-cond-piece = Piece on squares
pattern-cond-material = Material
pattern-cond-side = Side to move
pattern-cond-phase = Phase
pattern-cond-status = State
pattern-cond-previous-move = Previous move
pattern-cond-next-move = Next move
pattern-cond-result = Game result
pattern-cond-passed-pawn = Passed pawn
pattern-cond-isolated-pawn = Isolated pawn
pattern-cond-doubled-pawns = Doubled pawns
pattern-cond-exposed-king = Exposed king
pattern-cond-bishop-pair = Bishop pair
pattern-cond-attacks = Attacks
pattern-cond-defends = Defends
pattern-cond-pins = Pins
pattern-cond-skewers = Skewers
pattern-cond-forks = Forks
pattern-cond-discovered = Discovered attack
pattern-cond-double-attack = Double attack
pattern-cond-sacrifice = Sacrifice
pattern-cond-mate = Mate in N
pattern-empty-tree = Add a condition to start.
pattern-pick-card = Pick a card to edit it.
pattern-remove = Remove
pattern-move-up = Move up
pattern-move-down = Move down
pattern-wrap = Wrap in
pattern-unwrap = Unwrap
pattern-change-group = Turn into
pattern-new = New pattern
pattern-name-label = Name
pattern-name-placeholder = Name of the pattern
pattern-description-placeholder = Description (optional)
pattern-issue-here = Fix this card first.
pattern-field-side = Side
pattern-field-piece = Piece
pattern-field-where = Squares
pattern-field-actor = Acting piece
pattern-field-target = Target
pattern-field-behind = Piece behind
pattern-field-count = Count
pattern-field-min = At least
pattern-field-max = At most
pattern-field-limit = Limit
pattern-field-diff = Difference
pattern-field-bishops = Bishops
pattern-field-gain = Gain
pattern-field-when = When
pattern-field-new = Only if the last move created it
pattern-field-targets = Number of targets
pattern-field-capture = Capture
pattern-field-check = Check
pattern-field-promotion = Promotion
pattern-field-castling = Castling
pattern-field-net = Net loss (at least)
pattern-field-within = Within (moves)
pattern-field-outcome = Then
pattern-field-mate-min = Fewest moves
pattern-field-mate-max = Most moves
pattern-field-nodes = Search effort
pattern-field-phase = Phase
pattern-field-status = State
pattern-field-results = Results
pattern-field-discovery = Kind
pattern-field-moved = Piece that moved

# My patterns.
pattern-saved-title = My patterns
pattern-save = Save pattern...
pattern-save-tip = Save the pattern with this name
pattern-import = Import...
pattern-export = Export...
pattern-import-title = Import a pattern
pattern-export-title = Export the pattern
pattern-file-filter = MyChess patterns
pattern-saved-empty = No saved patterns yet.
pattern-saved-load-tip = Open this pattern in the Custom tab
pattern-saved-delete-tip = Delete this pattern
pattern-saved-damaged = (unreadable file)
pattern-saved-no-folder = There is no folder for saved patterns.

# Remarks.
pattern-notice-unknown-preset = There is no ready-made pattern called { $name }.
pattern-notice-unknown-preset-blank = There is no ready-made pattern.
pattern-notice-no-database = There is no database to search.
pattern-notice-stale = The database changed during the search; the result was dropped.
pattern-notice-failed = The search could not be completed.
pattern-notice-hits-failed = The occurrences in this game could not be computed.
pattern-notice-no-saved-folder = There is no folder for saved patterns.
pattern-notice-saved = Pattern "{ $name }" saved.
pattern-notice-deleted = Pattern "{ $name }" deleted.
pattern-notice-saved-not-found = There is no saved pattern called "{ $name }".
pattern-notice-invalid-saved-name = This name cannot be used for a pattern.
pattern-notice-saved-failed = The saved pattern could not be read or written.
pattern-notice-imported = Pattern "{ $name }" imported.
pattern-notice-import-failed = The pattern could not be imported.
pattern-notice-exported = Pattern exported.
pattern-notice-export-failed = The pattern could not be exported.
pattern-error-empty-group = This group is empty.
pattern-error-too-deep = The pattern is too deeply nested.
pattern-error-too-many-nodes = The pattern has too many conditions.
pattern-error-mover-not-allowed = "Side to move" cannot be used here: after the move, use "Other side".
pattern-error-invalid = Invalid value in { $path }.
pattern-error-invalid-param = Invalid value for "{ $field }".
pattern-error-file = Cannot read this pattern file.
pattern-error-newer-version = This pattern was saved by a newer version of MyChess.
pattern-edit-refused-bad-path = That card no longer exists.
pattern-edit-refused-not-a-group = Only an AND / OR group takes new items.
pattern-edit-refused-root = The first group cannot be removed or moved.
pattern-edit-refused-too-deep = The groups would be nested too deeply.
pattern-edit-refused-too-many-nodes = The pattern would have too many conditions.
pattern-edit-refused-group-full = A group takes at most { $limit } items.
pattern-edit-refused-move-into-itself = A card cannot be moved into itself.
pattern-edit-refused-needs-one-item = The group must have exactly one item.
pattern-edit-refused-no-region = This card has no squares to set.
pattern-edit-refused-no-marks = The comment on the board marks no squares.

pattern-region-squares =
    { $mode ->
        [from] from { $squares }
        [to] to { $squares }
       *[at] on { $squares }
    }

pattern-region-file =
    { $mode ->
        [from] from file { $file }
        [to] to file { $file }
       *[at] on file { $file }
    }

pattern-region-rank =
    { $mode ->
        [from] from rank { $rank }
        [to] to rank { $rank }
       *[at] on rank { $rank }
    }

pattern-region-own-rank =
    { $mode ->
        [from] from its own rank { $rank }
        [to] to its own rank { $rank }
       *[at] on its own rank { $rank }
    }

pattern-region-rect =
    { $mode ->
        [from] from inside the rectangle { $from }-{ $to }
        [to] to inside the rectangle { $from }-{ $to }
       *[at] inside the rectangle { $from }-{ $to }
    }

pattern-region-named-center =
    { $mode ->
        [from] from the central squares
        [to] to the central squares
       *[at] on the central squares
    }

pattern-region-named-extended-center =
    { $mode ->
        [from] from the extended center
        [to] to the extended center
       *[at] on the extended center
    }

pattern-region-named-queen-side =
    { $mode ->
        [from] from the queenside
        [to] to the queenside
       *[at] on the queenside
    }

pattern-region-named-king-side =
    { $mode ->
        [from] from the kingside
        [to] to the kingside
       *[at] on the kingside
    }

pattern-region-named-white-half =
    { $mode ->
        [from] from White's half of the board
        [to] to White's half of the board
       *[at] on White's half of the board
    }

pattern-region-named-black-half =
    { $mode ->
        [from] from Black's half of the board
        [to] to Black's half of the board
       *[at] on Black's half of the board
    }

pattern-region-named-light-squares =
    { $mode ->
        [from] from the light squares
        [to] to the light squares
       *[at] on the light squares
    }

pattern-region-named-dark-squares =
    { $mode ->
        [from] from the dark squares
        [to] to the dark squares
       *[at] on the dark squares
    }

pattern-region-named-edge =
    { $mode ->
        [from] from the edge of the board
        [to] to the edge of the board
       *[at] on the edge of the board
    }

pattern-region-named-long-diagonals =
    { $mode ->
        [from] from the long diagonals
        [to] to the long diagonals
       *[at] on the long diagonals
    }

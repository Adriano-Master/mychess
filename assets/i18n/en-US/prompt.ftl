prompt-replace-title = Replace move?
prompt-replace-body = A move already exists here. Choose what to do with yours. “Replace move” discards all the following moves.
prompt-replace-replace = Replace move
prompt-replace-new-main = New main line
prompt-replace-add-variation = Add variation
prompt-replace-try = Try variation
prompt-promotion-title = Promote to
prompt-strip-title = Confirm removal
prompt-strip-comments = { $count ->
    [one] Remove 1 comment from this game?
   *[other] Remove { $count } comments from this game?
}
prompt-strip-variations = { $count ->
    [one] Remove 1 variation from this game?
   *[other] Remove { $count } variations from this game?
}
prompt-truncate-after = { $count ->
    [one] Remove 1 move after this position?
   *[other] Remove { $count } moves after this position?
}
prompt-truncate-from = { $count ->
    [one] Remove this move?
   *[other] Remove this move and the ones after it ({ $count } moves in all)?
}
prompt-truncate-before = { $count ->
    [one] Remove 1 move before this position? The game will start here.
   *[other] Remove { $count } moves before this position? The game will start here.
}
prompt-strip-other = Remove { $count } items from this game?
prompt-confirm-remove = Remove
goto-move-title = Go to move number
goto-move-prompt = Move number (add ... or b for Black)
goto-move-placeholder = e.g. 12
goto-move-invalid = Enter a move number greater than zero.
paste-pgn-title = Paste PGN
paste-pgn-import = Import
paste-pgn-current = Insert current game
paste-pgn-placeholder = Paste the PGN text here
paste-pgn-errors = { $count ->
    [one] 1 problem found
   *[other] { $count } problems found
}
import-issue-nothing-to-import = There is no move or result in the text.
import-issue-text-outside-games = Text that belongs to no game was skipped ({ $bytes } bytes).
import-issue-truncated = Reading stopped after { $games } games: a limit was reached.
import-position-line = Line { $line }: { $message }
import-position-column = Line { $line }, column { $column }: { $message }
pgn-diag-bad-tag = A header line could not be read
pgn-diag-tag-too-long = A header value is too long and was cut
pgn-diag-rating-too-large = A rating is too large and was ignored
pgn-diag-invalid-fen = The FEN of the game is not valid
pgn-diag-illegal-castling = The castling rights of the FEN are not possible
pgn-diag-castling-reinterpreted = The castling rights of the FEN were reinterpreted
pgn-diag-illegal-move = Illegal move
pgn-diag-ambiguous-move = Ambiguous move
pgn-diag-invalid-move = Unreadable move
pgn-diag-too-many-nodes = The game has too many moves
pgn-diag-too-many-tags = The game has too many header tags
pgn-diag-duplicate-variation = A variation repeats an existing move
pgn-diag-invalid-nag-symbol = Unknown annotation symbol
pgn-diag-nag-out-of-range = Annotation number out of range
pgn-diag-nag-without-move = An annotation comes before any move
pgn-diag-variation-before-first-move = A variation comes before the first move
pgn-diag-unbalanced-close-paren = A closing parenthesis has no opening one
pgn-diag-unclosed-variation = A variation was never closed
pgn-diag-stray-close-brace = A closing brace has no opening one
pgn-diag-unterminated-comment = A comment was never closed
pgn-diag-comment-truncated = A comment is too long and was cut
pgn-diag-missing-result = The game has no result
pgn-diag-unexpected-token = Unexpected text in the moves
pgn-diag-result-mismatch = The result of the header and of the moves differ
pgn-diag-invalid-text-in-header = Invalid text in a header
pgn-diag-header-only-at-eof = The text ends after a header, without moves
pgn-diag-junk-between-games = Text between games was skipped
pgn-diag-encoding-fallback = The text encoding was guessed
pgn-diag-chess960-detected = The game is Chess960
pgn-diag-unsupported-variant = This chess variant is not supported
pgn-diag-game-too-large = The game is too large
pgn-diag-too-many-diagnostics = There are too many remarks; the rest is not shown
pgn-diag-unknown = Unknown remark about the text
chooser-title = Choose a game
chooser-filter-placeholder = Filter by player, event or result
chooser-count = { $shown } of { $total } games
chooser-open = Open
variations-title = Variations
action-clear = Clear
prompt-dont-ask-again = Don't ask again

chess960-title = New Chess960 game
chess960-prompt = Starting position number (0 to 959; 518 is the standard arrangement):
chess960-placeholder = 0 to 959
chess960-random = Random
chess960-start-play = Start and play the engine...
chess960-start = Start game
chess960-invalid = Enter a number from 0 to 959.
chess960-hint = To play against an engine use the second button, or Play > Play against the engine... later (starting from the current position).

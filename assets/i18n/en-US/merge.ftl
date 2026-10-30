# Merge a game into the current one (spec M7-6, slice c).

menu-edit-merge-game = Merge into current game...
action-merge-game = Merge the selected game into the current game
merge-button = Merge
merge-action = Merge into current game...
merge-games = Merge games

merge-title = Merge preview
merge-title-batch = Merge games
merge-source-label = Game to merge
merge-up-to = Up to move (of the merged game)
merge-all = All
merge-note-extends = The position matches the end of the game: the moves will be added to the main line.
merge-legend = The moves that the merge adds are highlighted.
merge-preview-label = Game after the merge
merge-ok = Merge
merge-cancel = Cancel

# The description put in a comment before the first merged move.
merge-comment-line = { $white } -- { $black } { $result }
merge-comment-moves = { $count ->
    [one] ({ $count } move)
   *[other] ({ $count } moves)
}
merge-comment-event = { $event }, { $year }

merge-batch-confirm = Merge { $count ->
    [one] one game
   *[other] { $count } games
} into the current game?
merge-batch-summary = { $merged } to merge, { $skipped } skipped
merge-batch-skip-no-match = { $count } without a matching position
merge-batch-skip-nothing = { $count } ending at the matching position
merge-batch-skip-same = { $count } is the current game
merge-batch-skip-unreadable = { $count } that could not be read
merge-batch-done = Merge complete: { $merged } merged, { $skipped } skipped

merge-err-unable = Unable to merge the selected game: { $reason }
merge-err-no-game = No game is selected
merge-err-closed = The selected database is not open.
merge-err-number = Invalid game number.
merge-err-self = Game cannot be merged into itself.
merge-err-empty = The current game has no moves.
merge-err-decode = Error decoding game.
merge-err-nomatch = No matching position found.
merge-err-nothing = The game to merge has no moves after the matching position.
merge-err-failed = A move of the game could not be added.
merge-err-trial = Not available in trial mode.
merge-err-batch-limit = Too many games to merge at once.

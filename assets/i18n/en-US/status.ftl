status-ready = Ready
status-turn-white = White to move ({ $move })
status-turn-black = Black to move ({ $move })
status-check = Check
status-checkmate = Checkmate
status-stalemate = Stalemate
status-modified = Modified
status-notice-illegal-move = Illegal move
status-notice-null-move-in-check = A null move is not allowed while in check
status-notice-prompt-pending = Answer the open dialog first
status-notice-stale-node = That position no longer exists
status-notice-no-move-here = There is no move here
status-notice-not-a-variation = That is not a variation
status-notice-not-in-variation = Not inside a variation
status-notice-undo-disabled = Undo is disabled in trial mode
status-notice-unreadable-move = Unreadable move: { $text }
status-notice-game-too-large = The game is too large
status-notice-other = Operation not performed
status-file-error = { $reason } ({ $path })
file-error-not-found = File not found
file-error-permission-denied = Permission denied
file-error-already-exists = The file already exists
file-error-too-large = The file is larger than the limit of { $limit } MiB
file-error-unsupported-encoding = Unsupported text encoding: { $encoding }
file-error-corrupt-gzip = The compressed file is corrupt
file-error-no-games-found = No games were found in the file
file-error-encoding-lossy = The file's encoding cannot hold all the characters of the game
file-error-io = The file could not be read or written
clipboard-error-empty = The clipboard is empty
clipboard-error-too-large = The clipboard text is larger than the limit of { $limit } MiB
clipboard-error-unavailable = The clipboard could not be read
clipboard-error-write-failed = The clipboard could not be written
paste-fen-error-empty = There is no position to paste
paste-fen-error-invalid = The clipboard does not hold a valid FEN: { $excerpt }
app-notice-host-effect-not-handled = The program could not carry out that request
setup-chess960-unsupported = Chess960 positions cannot be set up by hand yet
prefs-issue-invalid-value = Invalid value for { $key } in the preferences; the default was used
prefs-issue-clamped = The value of { $key } in the preferences was out of range and was adjusted
prefs-issue-newer-version = The preferences come from a newer version ({ $found })
prefs-issue-unreadable = The preferences file could not be read; the defaults are used
prefs-issue-save-failed = The preferences could not be saved
prefs-issue-no-config-dir = There is no configuration folder; preferences are not saved
shortcut-issue-unknown-action = Unknown action in the shortcuts: { $name }
shortcut-issue-invalid-binding = Invalid key for { $action }: { $text }
shortcut-issue-conflict = The key for { $action } is already used by { $other }
status-move-entry = Enter move: [{ $text }] { $candidates }
status-altered = altered
status-read-only = read-only
app-notice-save-in-progress = A save is already in progress
file-error-needs-confirmation = The file needs confirmation before it is replaced
status-move-entry-no-match = No legal move starts with "{ $text }"
status-comment-too-long = Comment is too long.
status-nothing-to-strip = Nothing to remove.
status-nag-draft-discarded = The annotations typed were not saved: one of them is not a known annotation.
base-notice-no-free-slot = Too many databases are open; close one first
base-notice-open-in-progress = A database is already being opened
base-notice-already-in-clipbase = You are already in the clipbase
base-notice-clipbase-empty = The clipbase has no games to paste
base-error-si3-needs-upgrade = This database is in the old SI3 format; upgrade it to SI4 first
base-error-corrupt = The database files are damaged or have an unsupported version
base-error-pending-journal = The database has an unfinished operation and must be repaired first
base-error-unsupported-format = The file is not a PGN text
base-error-too-many-games = The file has more games than can be opened
base-error-too-large = The games take too much memory
base-error-out-of-memory = Not enough memory to open the database
base-error-no-games = The file has no games
base-error-cancelled = Opening cancelled
base-error-game-unreadable = The game cannot be read
base-error-clipbase-limit = The clipbase has reached its limit of games
base-error-unencodable = The game cannot be stored in a database
base-error-unsupported-start = The game cannot be stored in a database because it starts from a Chess960 position (or one SI4 cannot hold). Save it as PGN with File > Save as...
base-error-internal = Internal error
base-notice-not-activated = A database was opened but not selected; answer the question about the game first
base-notice-game-not-loaded = The game was not loaded because a question about the current game is open; try again
base-notice-list-busy = A search, sort or copy is still running; wait for it or cancel it
base-notice-not-found = No game found; the list went back to the start
base-notice-filtered = { $removed } games removed from the filter
base-notice-copied = { $count } games copied to the Clipbase
base-notice-read-only = This database cannot be changed (a PGN file, the Clipbase or a read-only database)
base-notice-nothing-selected = Select one or more games first
base-notice-unreadable-entries = { $count } games could not be read and were removed from the filter
base-error-already-open = The database is open for writing elsewhere
base-error-read-only = The database is read-only
base-error-stale = The database changed on disk; it was reloaded and nothing was written
base-notice-stale = The database changed while the operation ran; the result was discarded
base-notice-list-reset = The database was reloaded with a different number of games; the filter, selection and sort were reset
base-error-name-limit = The database cannot take more names (players, events, sites or rounds)
base-notice-copy-same = Cannot copy: the source and the target are the same database ({ $from })
base-notice-copy-source-closed = Cannot copy from { $from } to { $to }: the source database is not open
base-notice-copy-target-closed = Cannot copy from { $from } to { $to }: the target database is not open
base-notice-copy-target-read-only = Cannot copy from { $from } to { $to }: the target is read-only
base-notice-copy-empty-filter = Cannot copy from { $from } to { $to }: the filter of the source has no games
base-notice-copy-nothing-selected = Cannot copy from { $from } to { $to }: no game is selected
base-notice-filter-copied = { $count ->
    [one] One game copied
   *[other] { $count } games copied
} from { $from } to { $to }
base-notice-copy-cancelled = Copy from { $from } to { $to } stopped: { $count ->
    [one] one game was copied
   *[other] { $count } games were copied
}
base-notice-copy-failed = Copy from { $from } to { $to } failed after { $count ->
    [one] one game
   *[other] { $count } games
}: { $reason }
base-notice-filter-copied-notes = { $count ->
    [one] One game copied
   *[other] { $count } games copied
} from { $from } to { $to }{ $truncated ->
    [0] {""}
    [one] ; one name was shortened to fit the format
   *[other] ; { $truncated } names were shortened to fit the format
}{ $reset ->
    [0] {""}
   *[other] ; the list of the target was reset because the file had a different number of games
}
base-notice-browser-limit = The browser shows only the first { $shown } of { $asked } games

bookmark-notice-added = Bookmark added
bookmark-notice-cannot-add = Only a game of an open database can be bookmarked
bookmark-notice-path-not-utf8 = This database path cannot be stored in a bookmark
bookmark-notice-tree-full = The bookmarks are full or the folders are nested too deeply
bookmark-notice-base-unavailable = Cannot open the database of bookmark "{ $title }"
bookmark-notice-game-not-found = No game to go to for bookmark "{ $title }"
bookmark-notice-dropped = { $count ->
    [one] One invalid bookmark was left out
   *[other] { $count } invalid bookmarks were left out
}
bookmark-notice-file-unreadable = The bookmarks file could not be read and was set aside
bookmark-notice-newer-version = The bookmarks file comes from a newer version; a backup copy was kept
bookmark-notice-save-failed = The bookmarks could not be saved
base-error-exists = A database with that name exists already; nothing was created
base-error-import-self = A source file is one of the files of the database and was skipped
base-error-export-self = The destination is the file of an open database: choose another name
status-chess960 = Chess960
status-chess960-number = Chess960 no. { $n }

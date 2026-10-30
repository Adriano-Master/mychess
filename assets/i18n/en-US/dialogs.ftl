dialog-ok = OK
dialog-cancel = Cancel
about-title = About MyChess
about-version = Version { $version }
about-license = Licensed under the GNU General Public License, version 3 or later.
about-credits = Chess pieces by Colin M.L. Burnett (GPL-2.0-or-later). Interface font: Noto Sans (SIL Open Font License 1.1).
about-description = A chess database and analysis program, inspired by Scid vs PC.
dialog-discard-title = Unsaved changes
dialog-discard-message = The game has been modified. Do you want to save the changes?
dialog-discard-save = Save
dialog-discard-dont-save = Don't save
dialog-overwrite-title = Replace file
dialog-overwrite-message = { $name } holds { $games ->
    [one] one game
   *[other] { $games } games
}. Replace all of them with this game?
dialog-overwrite-yes = Replace
file-dialog-open-pgn = Open PGN file
file-dialog-open-base = Open database or PGN file
file-dialog-save-pgn = Save game as PGN
file-dialog-append-pgn = Append game to PGN file
file-dialog-open-eco = Open ECO file
file-filter-pgn = PGN files
file-filter-eco = ECO files
file-dialog-open-mask = Open mask file
file-dialog-save-mask = Save mask file
file-filter-mask = Mask files
file-filter-all = All files
dialog-overwrite-partial = { $name } was only partly read; replacing it erases the rest. Replace it?
dialog-overwrite-existing = { $name } already exists. Replace it?
dialog-copy-title = Copy games
dialog-copy-message = Copy { $count ->
    [one] one game
   *[other] { $count } games
} from { $from } to { $to }?
dialog-copy-selected-message = Copy { $count ->
    [one] the selected game
   *[other] the { $count } selected games
} from { $from } to { $to }?
dialog-copy-yes = Copy
file-dialog-import-pgn = Import PGN files
file-dialog-new-base = Name of the new database
file-filter-base = Scid databases

# Import of PGN files (spec M3-8). Scid: `importPgnFile`, "Importing PGN file".
file-import-title = Importing PGN files
file-import-target-existing = Database: { $name }
file-import-target-new = New database: { $name }
file-import-files = Files ({ $count })
file-import-files-more = ... and { $count } more
file-import-progress = File { $file } of { $total } - { $games } games stored - { $percent }%
file-import-stop = Stop
file-import-close = Close
file-import-summary = Games found: { $read }. Imported: { $added }. Not imported: { $skipped }. Warnings: { $warnings }.
file-import-interrupted = Stopped: the games already read were stored.
file-import-nothing = No game was stored.
file-import-removed = The new database was not kept because no game was stored.
file-import-error = The import ended early: { $reason }
file-import-file-error = Error opening { $file }: { $reason }
file-import-line = { $file }, game { $game }, line { $line }: { $text }
file-import-line-game = { $file }, game { $game }: { $text }
file-import-line-file = { $file }: { $text }
file-import-more = ... and { $count } more remarks that are not shown.
file-import-opened = The new database was opened.
file-import-kept = The new database { $name } was kept but not opened.
file-import-notice-imported = { $count ->
    [one] One game imported
   *[other] { $count } games imported
}
file-import-notice-interrupted = { $count ->
    [one] Import stopped: one game was stored
   *[other] Import stopped: { $count } games were stored
}
file-import-notice-failed = { $count ->
    [one] Import ended with an error after one game: { $reason }
   *[other] Import ended with an error after { $count } games: { $reason }
}
file-import-notice-nothing = No game was found in the file
file-import-notice-tag-truncated = A tag value longer than 255 bytes was shortened
file-import-notice-extra-tags = More than 40 extra tags: the others were dropped
file-import-notice-empty-tag = A tag with an empty name was dropped
file-import-notice-tag-name = A tag name longer than 240 bytes was dropped
file-import-notice-name-truncated = A name longer than 255 bytes was shortened
file-import-notice-flag = A letter of ScidFlags is not a flag
file-import-notice-no-games = The file has no games
file-import-notice-tag-dropped = A tag was dropped because the record cannot hold it
file-import-notice-other = The game was repaired
file-import-skip-too-large = The game is too large for the database and was skipped
file-import-skip-variant = The game is of an unsupported variant and was skipped
file-import-skip-game-too-large = The game is larger than the limit and was skipped
file-import-skip-unencodable = The game cannot be stored and was skipped
file-import-skip-other = The game was skipped
file-import-stop-read = The file could not be read any further

# Export of games (spec M3-9). Scid: `exportGames`, "Export games".
export-games-title = Export games
export-games-what = What to export
export-games-scope-all = All the games of the database ({ $count })
export-games-scope-filter = The games of the filter ({ $count })
export-games-scope-selection = The selected games ({ $count })
export-games-scope-current = The current game
export-games-comments = Include comments
export-games-variations = Include variations
export-games-marks = Include commands such as [%eval] and [%clk]
export-games-only-tactics = Only games with the Tactics (T) flag
export-games-append = Append to the end of the file
export-games-choose = Choose destination...
export-games-cancel = Cancel
export-games-picking = Waiting for the file name...
export-games-stop = Stop
export-games-close = Close
export-games-progress = { $done } of { $total } games - { $percent }%
export-games-destination = File: { $name }
export-games-summary = Games exported: { $written }. Left out for lacking the T flag: { $excluded }. Not exported: { $skipped }.
export-games-appended = The games were added to the end of the existing file.
export-games-stopped = Stopped.
export-games-error = The export failed: { $reason }
export-games-line = Game { $game }: { $text }
export-games-more = ... and { $count } more games that are not listed.
export-games-skip-unreadable = The game could not be read from the database
export-games-skip-unwritable = The game could not be written as PGN
export-games-notice-exported = { $count ->
    [one] One game exported to { $name }
   *[other] { $count } games exported to { $name }
}
export-games-notice-skips = { $count ->
    [one] One game exported to { $name }; { $skipped ->
        [one] one not exported
       *[other] { $skipped } not exported
    }
   *[other] { $count } games exported to { $name }; { $skipped ->
        [one] one not exported
       *[other] { $skipped } not exported
    }
}
export-games-notice-all-skipped = { $skipped ->
    [one] The game could not be exported: the file was not changed
   *[other] None of the { $skipped } games could be exported: the file was not changed
}
export-games-notice-stopped = Export stopped: the file was not written
export-games-notice-stopped-appended = { $count ->
    [one] Export stopped: one game was already added to the file
   *[other] Export stopped: { $count } games were already added to the file
}
export-games-notice-nothing = No game to export: nothing was written

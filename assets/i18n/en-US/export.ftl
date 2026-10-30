# Export of documents and board images (spec M7-4, slice b).
# The application layer names these keys; the labels of the dialog belong to
# slice c.

export-options = Export options
export-format-label = Format
export-format-pgn = PGN
export-format-html = HTML
export-format-viewer = HTML with viewer
export-format-latex = LaTeX
export-format-epd = Positions (EPD)
export-format-list = Game list (text)
export-format-csv = Game list (CSV)

export-html-comments = Comments
export-html-variations = Variations
export-html-symbols = Symbols for annotations (! ?) instead of numbers (NAG)
export-html-diagram-size = Diagram size (pixels, 0 for none)
export-viewer-per-file = Games per file
export-viewer-include-pgn = Also write the PGN file
export-viewer-open-after = Open in the browser at the end
export-latex-comments = Comments
export-latex-variations = Variations
export-latex-flip = Board seen from Black
export-latex-null-moves = Write null moves as comments
export-epd-dedupe = Write each position once
export-epd-final-position = Use the final position of each game
export-epd-counters = Include the move counters (hmvc, fmvn)
export-epd-id = Include an id with the players and the event
export-epd-game-number = Include the game number (c0)
export-list-format = Format
export-list-legend = w White, b Black, W White Elo, B Black Elo, m Moves, r Result, g Number, d Date, c Country, e Event, s Site, f Number in the filter, C Comments, y Year, n Round, F Final material, S Starts from a position, D Deleted, o ECO, O Opening, V Variations
export-list-default = Default
export-csv-header = Header row
export-csv-separator = Separator
export-csv-separator-comma = Comma
export-csv-separator-semicolon = Semicolon
export-csv-bom = Byte order mark (for spreadsheets)
export-template-title = Document template
export-template-start = Text at the start of the file
export-template-end = Text at the end of the file
export-template-reset = Restore default

export-confirm-replace = { $count ->
    [one] The file { $files } already exists. Replace it?
   *[other] These files already exist: { $files }. Replace them?
}
export-confirm-and-more = and { $count } more
export-confirm-append = The games will be added to { $files }. The closing lines of the file are replaced by the new games and the end text. Continue?

export-image-title = Export position as image
export-image-format = Format
export-image-size = Size (pixels)
export-image-coordinates = Coordinates
export-image-flip = Flipped
export-image-last-move = Highlight the last move
export-image-marks = Arrows and marks
export-image-transparent = Transparent background
export-image-margin = Margin (pixels)
export-image-preview = Preview
export-image-save = Save...

export-notice-viewer = { $volumes ->
    [one] { $count } games exported to { $name }
   *[other] { $count } games exported to { $volumes } files, starting with { $name }
}
export-notice-epd = { $repeated ->
    [0] { $count } positions exported to { $name }
   *[other] { $count } positions exported to { $name } ({ $repeated } repeated left out)
}
export-notice-image-saved = Image saved to { $name }
export-notice-open-failed = The system could not open the exported file

export-err-no-marker = The file has no closing line to add the games before (</body> or \end{"{"}document{"}"}): it was not changed
export-err-not-utf8 = The file is not UTF-8 text, so games cannot be added to it: it was not changed
export-err-empty-format = The format of the game list has no known field
export-err-options = An option is out of its range
export-err-assets = The piece drawings are not available
export-err-image-size = The size or the margin of the image is outside the allowed range
export-err-image-piece = A piece drawing is missing or is not a usable SVG
export-err-image-font = The font for the coordinates is missing or invalid
export-err-image-render = The image could not be drawn
export-err-image-protected = That file is one of the open databases and cannot be overwritten
export-err-internal = The export failed unexpectedly

export-csv-col-number = Number
export-csv-col-filter-number = Filter number
export-csv-col-white = White
export-csv-col-black = Black
export-csv-col-white-elo = White Elo
export-csv-col-black-elo = Black Elo
export-csv-col-result = Result
export-csv-col-date = Date
export-csv-col-year = Year
export-csv-col-event = Event
export-csv-col-site = Site
export-csv-col-round = Round
export-csv-col-eco = ECO
export-csv-col-moves = Moves
export-csv-col-flags = Flags

# The dialogs (slice c, M7-4c).
menu-file-export-image = Export position as image...
file-dialog-export-doc = Export games
file-dialog-export-append = Add the games to a file
file-dialog-export-image = Save the image of the board
export-image-format-png = PNG image
export-image-format-svg = SVG image
export-ui-format-title = Format of the file
export-ui-diagram-none = none
export-ui-per-file-hint = Games in each file of the viewer
export-ui-list-hint = Letters that stand for the fields of each line
export-ui-confirm-title = The file already exists
export-ui-replace = Replace
export-ui-add = Add the games
export-ui-needs-assets = The piece drawings are not ready yet; the diagrams and the viewer may fail
export-ui-sep-hint = Field separator
export-report-repeated = Repeated positions left out: { $count }
export-report-not-selected = Games with no position to give: { $count }
export-report-volumes = Files written: { $count }
export-image-close = Close
export-image-picking = Waiting for the file name...
export-image-saving = Writing the image...
export-image-no-preview = The preview could not be made
export-image-margin-none = none
export-image-confirm = Replace the file { $name }?

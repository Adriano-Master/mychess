# Engine management (spec M4-3): the Engines dialog, the editor, the UCI
# options, the remarks of the status bar and the Tools menu.

menu-tools = Tools
menu-tools-engines = Engines...
menu-tools-start-engine = Start engine { $number }
menu-tools-toggle-analysis = Turn analysis on or off

action-start-engine = Start engine
action-manage-engines = Engines
action-toggle-engine-analysis = Turn engine analysis on or off

# ---- the Engines dialog ----
engines-title = Engines
engines-col-name = Name
engines-col-key = Key
engines-col-type = Type
engines-col-elo = Elo
engines-col-last-used = Last used
engines-empty = No engines yet. Add one, or look for the engines installed on this computer.
engines-btn-new = New
engines-btn-edit = Edit
engines-btn-copy = Copy
engines-btn-delete = Delete
engines-btn-up = Up
engines-btn-down = Down
engines-btn-options = UCI options...
engines-btn-start = Start
engines-btn-stop = Stop
engines-btn-restart = Restart
engines-btn-close = Close
engines-btn-find = Find installed engines
engines-btn-import = Import engines.dat from Scid...
engines-btn-dismiss = Dismiss
engines-btn-add = Add
engines-btn-analysis-options = Analysis options
engines-status-starting = starting...
engines-status-running = running
engines-status-stopping = stopping...
engines-status-failed = failed
engines-finding = Looking for engines...
engines-found-title = Engines found (click to add)
engines-found-note = Adding an engine runs its program once to identify it.
engines-found-none = No known engine was found.
engines-probing = Checking { $count ->
    [one] one engine
   *[other] { $count } engines
}...
engines-many-running = { $count } engines are running; this uses a lot of CPU.
engines-delete-title = Confirm delete
engines-delete-message = Delete the engine { $name } ({ $command })?
engines-issues-title = Remarks about the engine list
engines-issue-field-reset = Entry { $entry }: the field { $field } was reset.
engines-issue-entry-dropped = Entry { $entry } was dropped.
engines-issue-not-a-table = Item { $entry } of the file is not an engine and was skipped.
engines-issue-duplicate-hotkey = Entry { $entry } lost its key: another engine had it.
engines-issue-too-many = Only the first { $limit } engines were read.
engines-import-summary = Imported { $added }; skipped { $duplicates } already in the list and { $invalid } unusable.
engines-import-incomplete = The file was cut short or has a part that could not be read.
engines-import-relative = { $engine }: the command or folder is relative to the Scid folder; check it.
engines-import-args = { $engine }: the arguments could not be split exactly; check them.

# ---- analysis preferences ----
analysis-pref-title = Analysis options
analysis-pref-log-size = Log size (lines, 0 = off)
analysis-pref-max-ply = Max ply (0 = unlimited)
analysis-pref-add-name = Add the engine name to comments
analysis-pref-low-priority = Low CPU priority
analysis-pref-wrap-pv = Wrap long lines
analysis-pref-show-info = Show extra information

# ---- the editor ----
engines-edit-title-new = New engine
engines-edit-title = Edit engine
engines-field-name = Name
engines-field-command = Command
engines-field-args = Parameters
engines-field-dir = Folder
engines-field-url = Web page
engines-field-elo = Elo
engines-field-protocol = Protocol
engines-field-key = Key
engines-required = Fields in bold are required; the others are optional.
engines-browse = Browse...
engines-protocol-uci = UCI
engines-protocol-xboard = XBoard
engines-key-none = None
engines-btn-test = Test engine
engines-btn-test-tip = Runs the program to read its name and options.
engines-btn-detect-tip = Runs the program to find out which protocol it speaks.
engines-btn-detect = Detect protocol
engines-btn-use-detected = Use it
engines-btn-save = Save
engines-test-running = Testing...
engines-test-ok = OK: { $name } ({ $protocol }, { $options } options)
engines-test-failed = Test failed: { $reason }
engines-detect-running = Detecting...
engines-detect-found = The engine speaks { $protocol }.
engines-detect-failed = Detection failed: { $reason }
engines-warning-command-not-found = The command was not found on this computer.
engines-warning-not-executable = The file cannot be run.
engines-warning-working-dir-missing = The folder does not exist.
engines-pick-command-title = Choose the engine program
engines-pick-dat-title = Choose the engines.dat of the Scid
engines-filter-exe = Programs
engines-filter-dat = Engine lists (engines.dat)

engines-error-empty-name = The name must not be empty.
engines-error-empty-command = The command must not be empty.
engines-error-elo-range = The Elo must be between 0 and 4000.
engines-error-elo-number = The Elo must be a number.
engines-error-path-not-utf8 = The path has characters that cannot be stored.
engines-error-too-long = A field is too long.
engines-error-args-quote = The parameters have a quote that is not closed.

# ---- the UCI options ----
engines-options-title = UCI options: { $name }
engines-options-probing = Starting the engine to read its options...
engines-options-failed = The engine could not be started: { $reason }
engines-options-not-uci = Only UCI engines have options.
engines-options-none = The engine has no options.
engines-options-default = default: { $default }
engines-options-reset = Restore defaults
engines-options-save = Save
engines-options-button-hint = Buttons act only while the engine is running.
engines-option-multipv = Number of lines (MultiPV)
engines-option-hash = Hash memory (MB)
engines-option-ownbook = Use the engine's own book
engines-option-bookfile = Book file
engines-option-limitstrength = Limit strength
engines-option-error-range = The value must be between { $min } and { $max }.
engines-option-error-not-in-list = The value is not one of the choices.
engines-option-error-wrong-type = The value has the wrong type.
engines-option-error-control = The text must not have line breaks.
engines-option-error-other = The value is not accepted.

# ---- failures and remarks (the keys come from the application) ----
engines-failure-exit-code = exit code { $code }
engines-failure-stderr = The engine said: { $text }
engines-failure-cannot-start = The program could not be started{ $detail }.
engines-failure-exited-early = The engine ended before it was ready{ $detail }.
engines-failure-crashed = The engine ended by itself{ $detail }.
engines-failure-timeout = The engine did not answer in time{ $detail }.
engines-failure-stop-timeout = The engine ignored the stop request and was ended{ $detail }.
engines-failure-not-an-engine = The file is not an engine, or it speaks neither UCI nor XBoard{ $detail }.
engines-failure-io = Talking to the engine failed{ $detail }.
engines-failure-closed = The engine was already closed{ $detail }.
engines-failure-other = The engine failed{ $detail }.
engines-notice-with-name = { $name }: { $text }
engines-notice-file-unreadable = The engine list could not be read; a copy of the file was kept.
engines-notice-newer-version = The engine list was written by a newer version of MyChess.
engines-notice-file-fixed = The engine list had { $count ->
    [one] one problem
   *[other] { $count } problems
} that { $count ->
    [one] was
   *[other] were
} fixed.
engines-notice-save-failed = The engine list could not be saved.
engines-notice-no-engine-on-key = No engine has the key { $key }.
engines-notice-no-engine-running = No engine is running.
engines-notice-list-full = The engine list is full.
engines-notice-not-found = The engine is not in the list.
engines-notice-entry-invalid = The engine cannot be stored as it is.
engines-notice-restart-to-apply = Restart the engine to apply the change.
engines-notice-many-running = { $count } engines are running.
engines-notice-command-refused = The engine refused the command.
engines-notice-imported = Imported { $added ->
    [one] one engine
   *[other] { $added } engines
}; { $duplicates ->
    [one] one duplicate
   *[other] { $duplicates } duplicates
} and { $invalid ->
    [one] one unusable
   *[other] { $invalid } unusable
} skipped.
engines-notice-nothing-to-import = The file has no engine to import.

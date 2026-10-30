# Sounds and move announcement (spec M8-11)
menu-view-sounds = Sounds...
sound-title = Sounds
sound-enabled = Play sounds
sound-volume = Volume
sound-on-new = Sound for each new move
sound-on-forward = Sound when moving forward
sound-on-back = Sound when moving back
sound-tick-only = Same tick for every move
sound-announce = Say the move (needs word files in the folder)
sound-folder = Sounds folder
sound-folder-builtin = Built-in sounds only
sound-folder-choose = Choose...
sound-folder-remove = Remove
sound-folder-none = No folder: the built-in sounds are used.
sound-folder-loading = Reading the folder...
sound-folder-found = { $found ->
    [one] 1 sound file found
   *[other] { $found } sound files found
}
sound-folder-skipped = { $skipped ->
    [one] 1 file ignored (not a valid WAV)
   *[other] { $skipped } files ignored (not valid WAV)
}
sound-folder-failed = The folder could not be read.
sound-folder-help = Files as in Scid: King.wav, Knight.wav, a.wav to h.wav, 1.wav to 8.wav, x.wav, Check.wav, Mate.wav, Promote.wav, CastleK.wav, CastleQ.wav, Back.wav, move.wav, start.wav, end.wav.
sound-last = Last announcement: { $text }
sound-test = Test
sound-close = Close
sound-pack = Sound pack
sound-pack-default = Default (synthesized)
sound-pack-user = { $name } (user)
sound-pack-author = Author: { $author }
sound-pack-license = License: { $license }
sound-pack-covered = { $count ->
    [one] 1 of { $total } sounds; the rest use the default sound
   *[other] { $count } of { $total } sounds; the rest use the default sound
}
sound-pack-complete = All { $total } sounds are in the pack
sound-pack-loading = Loading...
sound-pack-error-missing = The sound pack was not found.
sound-pack-error-invalid = The sound pack name is not valid.
sound-pack-error-io = The sound pack could not be read.
sound-pack-error = The sound pack could not be loaded.
sound-pack-warnings = { $count ->
    [one] 1 remark about this pack (the affected sounds use the default sound)
   *[other] { $count } remarks about this pack (the affected sounds use the default sound)
}
sound-packs-ignored = { $count ->
    [one] 1 item of the sound folder was ignored
   *[other] { $count } items of the sound folder were ignored
}
sound-open-folder = Open sounds folder
sound-user-folder-hint = Copy the sound packs (one folder each) into this folder:
sound-advanced = Advanced

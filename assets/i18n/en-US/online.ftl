# Playing on Lichess (spec M8-3, phase c). No equivalent in the Scid.

menu-online = Online
menu-online-connect = Connect to Lichess...
menu-online-seek = Find a game...
menu-online-challenge = Challenge a user...
menu-online-incoming = Incoming challenges ({ $number })...
menu-online-disconnect = Sign out
menu-online-fics = FICS...
menu-online-remove-lock = Remove the assistance lock...

action-online-connect = Connect to Lichess
action-online-seek = Find a game on Lichess

# Connect window
online-connect-title = Connect to Lichess
online-connect-steps = 1. Copy the link below and open it in your browser to create a personal token with the scopes "Play games with the board API", "Read incoming challenges" and "Create, accept, decline challenges".
    2. Paste the token here. It is never shown, logged or saved in the preferences.
online-connect-copy-link = Copy the link to create the token
online-connect-token-placeholder = Personal access token
online-connect-store = Where to keep the token:
online-store-keyring = In the keychain of the system (recommended)
online-store-file = In a file only you can read (I agree to keep it on disk)
online-store-session = Do not keep it (this session only)
online-connect-fairplay = While you play, engines, the opening tree, tablebases and position searches are disabled, as the Lichess fair play rules require.
online-connect-connecting = Connecting...
online-connect-saved = Use the saved token ({ $login })
online-connect-button = Connect

# Windows
online-minutes = Minutes
online-increment = Increment (s)
online-rated = Rated game
online-assist-warning = When a game starts, engines and queries are disabled until it ends.
online-close = Close
online-no = No
online-yes = Yes

online-seek-title = Find a game
online-seek-waiting = Looking for an opponent...
online-seek-cancel = Stop looking
online-seek-invalid = Lichess does not accept this time control for a search (rapid or slower).
online-seek-button = Search

online-challenge-title = Challenge a user
online-challenge-user = Lichess user name
online-challenge-waiting = Waiting for { $user } to answer...
online-challenge-cancel = Withdraw the challenge
online-challenge-invalid = Enter a valid user name and a time control of blitz or slower.
online-challenge-button = Challenge
online-color-random = Random
online-color-white = White
online-color-black = Black
online-variant = Variant
online-variant-standard = Standard
online-variant-chess960 = Chess960
online-challenge-start = Position (0-959)
online-challenge-start-hint = random
online-challenge-start-invalid = Enter a position from 0 to 959 (empty: Lichess draws one). A fixed position needs a casual game.

online-incoming-title = Incoming challenges
online-incoming-empty = No challenges.
online-incoming-rated = { $who }: { $time }, rated
online-incoming-casual = { $who }: { $time }, casual
online-incoming-unplayable = MyChess cannot play this one (variant or time control).
online-incoming-chess960 = Chess960 variant.
online-incoming-accept = Accept
online-incoming-decline = Decline

online-confirm-resign-title = Resign?
online-confirm-resign = Do you want to resign this game?
online-confirm-unlock-title = Remove the assistance lock?
online-confirm-unlock = I understand that the game may continue on the server and that I must not use assistance in it.
online-confirm-quit-title = A game is in progress
online-confirm-quit = Closing the program does not end the game: the clock keeps running on the server. Close anyway?

# Panel under the board
online-status-your-turn = Your turn
online-status-opponent-turn = Opponent's turn
online-status-reconnecting = Reconnecting...
online-status-over = Game over
online-versus = vs { $name }
online-info-rated = Rated { $time }
online-info-casual = Casual { $time }
online-abort = Abort
online-abort-tip = Abort the game before it really starts
online-resign = Resign
online-resign-tip = Give up the game
online-offer-draw = Offer draw
online-offer-draw-tip = Offer a draw to the opponent
online-draw-accept = Accept draw
online-draw-accept-tip = Accept the draw the opponent offered
online-draw-decline = Decline
online-draw-decline-tip = Decline the draw the opponent offered
online-takeback-accept = Accept takeback
online-takeback-accept-tip = Let the opponent take back the last move
online-takeback-decline = Decline takeback
online-takeback-decline-tip = Refuse the takeback
online-claim = Claim victory
online-claim-tip = Win the game: the opponent left
online-gone-waiting = The opponent left. You can claim the victory in { $secs } s.
online-gone-claimable = The opponent left. You can claim the victory now.
online-restore = Restore previous game
online-restore-tip = Put back the game that was in the editor before
online-close-tip = Leave this game
online-premove-set = Premove set (Esc cancels it)
online-link-up = Connected
online-link-reconnecting = Reconnecting
online-link-down = Offline
online-ping = { $ms } ms
online-assist-off = Online game: assistance disabled (engines, opening tree, tablebases and searches)
online-remove-lock = Remove lock...

# End of a game
online-end-line = { $outcome } { $reason }
online-end-no-result = The game ended without a result.
online-end-mate = By checkmate.
online-end-resign = By resignation.
online-end-stalemate = By stalemate.
online-end-timeout = On time.
online-end-draw = By agreement or by the rules.
online-end-insufficient = By insufficient material.
online-end-cheat = Rules infraction.
online-end-variant = By the rules of the variant.
online-end-aborted = The game was aborted.
online-end-unknown = The server did not say why.

# Reasons for declining a challenge
online-decline-generic = The user does not want to play now.
online-decline-later = The user asks to play later.
online-decline-too-fast = The time control is too fast for the user.
online-decline-too-slow = The time control is too slow for the user.
online-decline-time-control = The user does not accept this time control.
online-decline-rated = The user only plays rated games.
online-decline-casual = The user only plays casual games.
online-decline-standard = The user only plays standard chess.
online-decline-variant = The user does not play this variant.
online-decline-no-bot = The user does not play against bots.
online-decline-only-bot = The user only plays against bots.

# Remarks of the status bar
online-notice-rejected-wrong-state = That is not possible in the current state of the connection.
online-notice-rejected-not-connected = Connect to Lichess first.
online-notice-rejected-busy = Finish the game against an engine, the tournament or the annotation first.
online-notice-rejected-game-in-progress = A game is in progress.
online-notice-rejected-events-not-ready = The connection is not ready yet; try again in a moment.
online-notice-rejected-time-control = Lichess does not accept this time control.
online-notice-rejected-not-your-turn = It is not your turn.
online-notice-rejected-no-game = There is no game in progress.
online-notice-rejected-illegal-move = That move cannot be made.
online-notice-rejected-challenge = That challenge cannot be played.
online-notice-rejected-not-available = That is not available now.
online-notice-rejected-bad-user = That is not a valid Lichess user name.
online-notice-rejected-move-pending = Wait for the server to confirm your last move.
online-notice-assistance-disabled = Disabled during an online game.
online-notice-assistance-will-be-disabled = When a game starts, engines and queries will be disabled.
online-notice-connected = Connected to Lichess.
online-notice-signed-out = Signed out. To revoke the token, go to lichess.org/account/oauth/token.
online-notice-disconnected = The connection to Lichess is not coming back.
online-notice-another-game = Another game is in progress elsewhere; continue it in the browser.
online-notice-resumed = A game in progress was resumed.
online-notice-unsupported-variant = MyChess plays only standard chess online; continue this game in the browser.
online-notice-move-rejected = The server refused your move: { $reason }
online-notice-move-uncertain = The server may not have received your last move; the board follows the server.
online-notice-resynchronised = The game was synchronised again with the server.
online-notice-desynced = The game on screen cannot follow the server any more; continue in the browser.
online-notice-game-lost = The game could not be followed and was closed here; it may still be on at Lichess. Continue in the browser.
online-notice-search-timed-out = No game started; the search ended.
online-notice-challenge-declined = Challenge declined.
online-notice-challenge-declined-reason = Challenge declined: { $reason }
online-notice-imported = The game was stored in the Clipbase.
online-notice-already-imported = That game is already stored.
online-notice-not-imported = The game ended without moves and was not stored.
online-notice-import-failed = The game could not be stored.
online-notice-lock-released = The assistance lock was removed.
online-notice-marker-cleared = The game that was in progress is over; the lock is gone.
online-notice-previous-restored = The previous game is back in the editor.
online-notice-game-over = The game is over.

# Failures
online-error-unauthorized = Lichess refused the token. It may have expired or been revoked.
online-error-scope-missing = The token lacks a permission. Create it again with the scopes in the link.
online-error-rate-limited = Too many requests. Wait { $secs } s.
online-error-network = No connection to Lichess.
online-error-timeout = Lichess did not answer in time.
online-error-not-found = Not found on Lichess.
online-error-rejected = Lichess refused: { $reason }
online-error-protocol = Lichess sent something that cannot be understood.
online-error-bot-account = This is a Bot account. MyChess plays with human accounts only.
online-error-insecure-url = The server address is not safe for a token. Only https is accepted.
online-error-no-token = There is no saved token. Paste one.
online-error-bad-token = That does not look like a Lichess token.
online-error-token-store = The token could not be saved. Choose another place and connect again.
online-error-time-control = Lichess does not accept this time control.
online-error-server = Lichess has a problem. Try again later.
online-error-other = Something went wrong with Lichess.

menu-online-watch = Watch a game...
menu-online-stop-watch = Stop watching

online-oauth-button = Log in with the browser
online-oauth-hint = Opens lichess.org so you authorize MyChess there; no token to copy.
online-oauth-waiting = Waiting for you to authorize MyChess in the browser...
online-oauth-not-opened = The program could not open the browser. Copy the link and open it yourself.
online-oauth-copy = Copy the authorization link
online-oauth-cancel = Cancel the login
online-error-login-failed = The login in the browser was not completed.

online-watch-title = Watch a game
online-watch-help = Paste a link to a Lichess game or its 8-character id.
online-watch-placeholder = https://lichess.org/abcd1234
online-watch-delay = Games in progress arrive delayed by 3 moves.
online-watch-button = Watch
online-watch-names = { $white } vs { $black }
online-watch-live = Watching. Games in progress are delayed by 3 moves.
online-watch-over = The game is over.
online-watch-incomplete = Watching. The moves before this position are not available.
online-watch-stop = Stop watching
online-watch-stop-tip = Close the game and bring back the one you had

online-promotion-title = Promote the pawn to
online-promotion-queen = Queen
online-promotion-rook = Rook
online-promotion-bishop = Bishop
online-promotion-knight = Knight

online-takeback-propose = Ask for a takeback
online-takeback-propose-tip = Ask the opponent to take back your last move
online-claim-draw = Claim a draw
online-claim-draw-tip = Ask for a draw: the opponent left
online-rematch = Rematch
online-rematch-tip = Challenge the same opponent again with the colors swapped

online-notice-rejected-bad-game-ref = That is not a Lichess game link or id.
online-notice-rejected-own-game = You cannot watch the game you are playing.
online-notice-rejected-bad-variant = The Chess960 position must be a number from 0 to 959, in a casual challenge (a search cannot fix it).
online-notice-watching = Opening the game to watch...
online-notice-watch-ended = The watched game is over.
online-notice-watch-incomplete = The moves before this position are not available.
online-notice-watch-steps-dropped = Moves arrived faster than the history could be loaded and some were skipped.
online-notice-browser-not-opened = Could not open the browser: copy the link of the login window.
online-notice-rematch-sent = Rematch proposed. Waiting for the answer.

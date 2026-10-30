# Créditos: Lichess Futuristic

- **Autor:** Enigmahack (<https://github.com/Enigmahack>)
- **Licença (SPDX):** AGPL-3.0-or-later ("AGPLv3+" no COPYING.md do lila). Aceita só para dados de áudio (ADR 0015).
- **Origem:** <https://github.com/lichess-org/lila/tree/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/futuristic>
- **Commit do lila:** `57de8faf654e66dc331d2efcfdf32cdb20bc2c86` (master em 2026-10-27)
- **Obtido em:** 2026-10-27
- **Modificações:** nenhuma; arquivos OGG sem modificação, byte a byte iguais aos do lila.

## Declaração de licença (COPYING.md do lila, no commit acima)

```
Files | Author(s) | License
public/sounds/futuristic | [Enigmahack](https://github.com/Enigmahack) | AGPLv3+
```

Observação: o COPYING.md escreve `public/sounds/futuristic`, mas o diretório real do repositório é `public/sound/futuristic` (sem o "s"). Entendemos que se refere ao mesmo conjunto.

## Arquivos

| Arquivo | URL | Bytes | Formato |
|---|---|---|---|
| `Move.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/futuristic/Move.ogg> | 15306 | Ogg Vorbis, 44100 Hz, 2 canais, 0.22 s |
| `Capture.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/futuristic/Capture.ogg> | 20515 | Ogg Vorbis, 44100 Hz, 2 canais, 0.34 s |
| `Check.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/futuristic/Check.ogg> | 22092 | Ogg Vorbis, 44100 Hz, 2 canais, 0.41 s |
| `LowTime.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/futuristic/LowTime.ogg> | 27751 | Ogg Vorbis, 44100 Hz, 2 canais, 0.59 s |
| `Victory.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/futuristic/Victory.ogg> | 144739 | Ogg Vorbis, 44100 Hz, 2 canais, 3.23 s |
| `Defeat.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/futuristic/Defeat.ogg> | 143191 | Ogg Vorbis, 44100 Hz, 2 canais, 3.78 s |
| `Draw.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/futuristic/Draw.ogg> | 95341 | Ogg Vorbis, 44100 Hz, 2 canais, 2.40 s |
| `GenericNotify.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/futuristic/GenericNotify.ogg> | 41513 | Ogg Vorbis, 44100 Hz, 2 canais, 0.88 s |

## Links simbólicos do Git

- `Checkmate.ogg` (9 bytes) é link simbólico para `Check.ogg`, do mesmo pacote. Não foi duplicado: o `pack.toml` mapeia `checkmate = "Check.ogg"`.
- `Confirmation.ogg`, `Error.ogg`, `OutOfBound.ogg` são links simbólicos e não foram incluídos (sem uso no MyChess).

## Omitidos

Os demais sons do pacote (CountDown*, Tournament*, Berserk, Explosion, NewChallenge, NewPM etc.) e todos os `.mp3` não foram incluídos por não terem uso no MyChess.

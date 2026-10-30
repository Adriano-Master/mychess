# Créditos: Lichess Piano

- **Autor:** Enigmahack (<https://github.com/Enigmahack>)
- **Licença (SPDX):** AGPL-3.0-or-later ("AGPLv3+" no COPYING.md do lila). Aceita só para dados de áudio (ADR 0015).
- **Origem:** <https://github.com/lichess-org/lila/tree/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/piano>
- **Commit do lila:** `57de8faf654e66dc331d2efcfdf32cdb20bc2c86` (master em 2026-10-27)
- **Obtido em:** 2026-10-27
- **Modificações:** nenhuma; arquivos OGG sem modificação, byte a byte iguais aos do lila.

## Declaração de licença (COPYING.md do lila, no commit acima)

```
Files | Author(s) | License
public/sounds/piano | [Enigmahack](https://github.com/Enigmahack) | AGPLv3+
```

Observação: o COPYING.md escreve `public/sounds/piano`, mas o diretório real do repositório é `public/sound/piano` (sem o "s"). Entendemos que se refere ao mesmo conjunto.

## Arquivos

| Arquivo | URL | Bytes | Formato |
|---|---|---|---|
| `Move.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/piano/Move.ogg> | 7081 | Ogg Vorbis, 44100 Hz, 2 canais, 0.14 s |
| `Capture.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/piano/Capture.ogg> | 12323 | Ogg Vorbis, 44100 Hz, 2 canais, 0.19 s |
| `Check.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/piano/Check.ogg> | 22637 | Ogg Vorbis, 44100 Hz, 2 canais, 0.54 s |
| `LowTime.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/piano/LowTime.ogg> | 39902 | Ogg Vorbis, 44100 Hz, 2 canais, 0.95 s |
| `Victory.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/piano/Victory.ogg> | 43503 | Ogg Vorbis, 44100 Hz, 2 canais, 1.18 s |
| `Defeat.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/piano/Defeat.ogg> | 28476 | Ogg Vorbis, 44100 Hz, 2 canais, 0.75 s |
| `Draw.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/piano/Draw.ogg> | 40255 | Ogg Vorbis, 44100 Hz, 2 canais, 1.07 s |
| `GenericNotify.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/piano/GenericNotify.ogg> | 22322 | Ogg Vorbis, 44100 Hz, 2 canais, 0.53 s |

## Links simbólicos do Git

- `Checkmate.ogg` (9 bytes) é link simbólico para `Check.ogg`, do mesmo pacote. Não foi duplicado: o `pack.toml` mapeia `checkmate = "Check.ogg"`.
- `Confirmation.ogg`, `Error.ogg`, `OutOfBound.ogg` são links simbólicos e não foram incluídos (sem uso no MyChess).

## Omitidos

Os demais sons do pacote (CountDown*, Tournament*, Berserk, Explosion, NewChallenge, NewPM etc.) e todos os `.mp3` não foram incluídos por não terem uso no MyChess.

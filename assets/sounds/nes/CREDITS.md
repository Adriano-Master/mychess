# Créditos: Lichess NES

- **Autor:** Enigmahack (<https://github.com/Enigmahack>)
- **Licença (SPDX):** AGPL-3.0-or-later ("AGPLv3+" no COPYING.md do lila). Aceita só para dados de áudio (ADR 0015).
- **Origem:** <https://github.com/lichess-org/lila/tree/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/nes>
- **Commit do lila:** `57de8faf654e66dc331d2efcfdf32cdb20bc2c86` (master em 2026-10-27)
- **Obtido em:** 2026-10-27
- **Modificações:** nenhuma; arquivos OGG sem modificação, byte a byte iguais aos do lila.

## Declaração de licença (COPYING.md do lila, no commit acima)

```
Files | Author(s) | License
public/sounds/nes | [Enigmahack](https://github.com/Enigmahack) | AGPLv3+
```

Observação: o COPYING.md escreve `public/sounds/nes`, mas o diretório real do repositório é `public/sound/nes` (sem o "s"). Entendemos que se refere ao mesmo conjunto.

## Arquivos

| Arquivo | URL | Bytes | Formato |
|---|---|---|---|
| `Move.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/nes/Move.ogg> | 6823 | Ogg Vorbis, 44100 Hz, 2 canais, 0.14 s |
| `Capture.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/nes/Capture.ogg> | 10493 | Ogg Vorbis, 44100 Hz, 2 canais, 0.18 s |
| `Check.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/nes/Check.ogg> | 11531 | Ogg Vorbis, 44100 Hz, 2 canais, 0.23 s |
| `LowTime.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/nes/LowTime.ogg> | 20176 | Ogg Vorbis, 44100 Hz, 2 canais, 0.50 s |
| `Victory.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/nes/Victory.ogg> | 27342 | Ogg Vorbis, 44100 Hz, 2 canais, 0.70 s |
| `Defeat.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/nes/Defeat.ogg> | 34434 | Ogg Vorbis, 44100 Hz, 2 canais, 0.92 s |
| `Draw.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/nes/Draw.ogg> | 38508 | Ogg Vorbis, 44100 Hz, 2 canais, 1.03 s |
| `GenericNotify.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/nes/GenericNotify.ogg> | 11553 | Ogg Vorbis, 44100 Hz, 2 canais, 0.26 s |

## Links simbólicos do Git

- `Checkmate.ogg` (9 bytes) é link simbólico para `Check.ogg`, do mesmo pacote. Não foi duplicado: o `pack.toml` mapeia `checkmate = "Check.ogg"`.
- `Confirmation.ogg`, `Error.ogg`, `OutOfBound.ogg` e CountDown1 a 10 são links simbólicos e não foram incluídos (sem uso no MyChess).

## Omitidos

Os demais sons do pacote (CountDown*, Tournament*, Berserk, Explosion, NewChallenge, NewPM etc.) e todos os `.mp3` não foram incluídos por não terem uso no MyChess.

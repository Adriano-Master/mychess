# Créditos: Lichess SFX

- **Autor:** Enigmahack (<https://github.com/Enigmahack>)
- **Licença (SPDX):** AGPL-3.0-or-later ("AGPLv3+" no COPYING.md do lila). Aceita só para dados de áudio (ADR 0015).
- **Origem:** <https://github.com/lichess-org/lila/tree/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/sfx>
- **Commit do lila:** `57de8faf654e66dc331d2efcfdf32cdb20bc2c86` (master em 2026-10-27)
- **Obtido em:** 2026-10-27
- **Modificações:** nenhuma; arquivos OGG sem modificação, byte a byte iguais aos do lila.

## Declaração de licença (COPYING.md do lila, no commit acima)

```
Files | Author(s) | License
public/sounds/sfx | [Enigmahack](https://github.com/Enigmahack) | AGPLv3+
```

Observação: o COPYING.md escreve `public/sounds/sfx`, mas o diretório real do repositório é `public/sound/sfx` (sem o "s"). Entendemos que se refere ao mesmo conjunto.

## Arquivos

| Arquivo | URL | Bytes | Formato |
|---|---|---|---|
| `Move.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/sfx/Move.ogg> | 12521 | Ogg Vorbis, 44100 Hz, 2 canais, 0.21 s |
| `Capture.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/sfx/Capture.ogg> | 17011 | Ogg Vorbis, 44100 Hz, 2 canais, 0.38 s |
| `Check.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/sfx/Check.ogg> | 22587 | Ogg Vorbis, 44100 Hz, 2 canais, 0.55 s |
| `LowTime.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/sfx/LowTime.ogg> | 21947 | Ogg Vorbis, 44100 Hz, 2 canais, 0.51 s |
| `Victory.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/sfx/Victory.ogg> | 60323 | Ogg Vorbis, 44100 Hz, 2 canais, 1.86 s |
| `Defeat.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/sfx/Defeat.ogg> | 37136 | Ogg Vorbis, 44100 Hz, 2 canais, 0.85 s |
| `Draw.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/sfx/Draw.ogg> | 29191 | Ogg Vorbis, 44100 Hz, 2 canais, 0.78 s |
| `GenericNotify.ogg` | <https://raw.githubusercontent.com/lichess-org/lila/57de8faf654e66dc331d2efcfdf32cdb20bc2c86/public/sound/sfx/GenericNotify.ogg> | 45003 | Ogg Vorbis, 44100 Hz, 2 canais, 1.29 s |

## Links simbólicos do Git

- `Checkmate.ogg` (9 bytes) é link simbólico para `Check.ogg`, do mesmo pacote. Não foi duplicado: o `pack.toml` mapeia `checkmate = "Check.ogg"`.
- `Confirmation.ogg`, `Error.ogg`, `OutOfBound.ogg` são links simbólicos e não foram incluídos (sem uso no MyChess).

## Omitidos

Os demais sons do pacote (CountDown*, Tournament*, Berserk, Explosion, NewChallenge, NewPM etc.) e todos os `.mp3` não foram incluídos por não terem uso no MyChess.

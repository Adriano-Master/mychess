# ADR 0013: Áudio: `rodio` (só `cpal`) atrás da feature `audio` do `mychess-gui`, sons sintetizados

- **Status:** aceita pelo orquestrador; revertível pelo dono. Especificação: `docs/specs/m8-11-sons.md` (D2800-D2899).

## Contexto

O Scid vs PC toca sons de lance e anuncia o lance falado com arquivos `.wav` de uma pasta do usuário, via o pacote Tcl Snack (sem manutenção). Esses `.wav` têm licença incerta e não podem entrar no repositório. O MyChess precisa de saída de áudio nas três plataformas, sem tornar o código testável dependente de uma placa de som.

## Decisão

1. **`rodio` 0.23** (MIT OR Apache-2.0; usa `cpal`, Apache-2.0), com `default-features = false, features = ["playback"]`. Sem as features de decodificação: o Symphonia (MPL-2.0) e os demais decodificadores não são necessários, porque o WAV é lido por um leitor próprio em `mychess-app` e os sons embutidos são PCM gerado em código.
2. **Só o `mychess-gui` depende dele**, como dependência **opcional** da feature `audio` (padrão ligada). A camada de aplicação define o *trait* `SoundSink`; a GUI instala o `RodioSink`. Sem a feature, ou sem dispositivo de áudio, o sink é nulo: silêncio, sem erro ao usuário.
3. **Sons padrão sintetizados** de forma determinística (nenhum arquivo de áudio no repositório, nada a licenciar). O usuário pode apontar uma pasta com `.wav` (nomes do Scid).
4. **Dependência de sistema:** no Linux o `cpal` usa ALSA e exige `libasound2-dev` na compilação (o programa funciona com PipeWire/PulseAudio pelos *plugins* ALSA do sistema). Windows (WASAPI) e macOS (CoreAudio) não pedem nada. O `README.md` documenta e o `.github/workflows/ci.yml` instala `libasound2-dev` nos jobs de `ubuntu-latest` **no mesmo passo**.

## Licença

`rodio` e `cpal`: MIT OR Apache-2.0 / Apache-2.0; `alsa`/`alsa-sys`: MIT/Apache-2.0 (todos presentes em `deny.toml`). Ver `cargo tree -p mychess-gui -e normal -i rodio` e `cargo deny check licenses`.

## Alternativas consideradas

- **`cpal` direto:** menos código transitivo, mas obriga a escrever mistura, reamostragem e fila; o `rodio` faz isso e é o padrão do ecossistema.
- **`kira`, `miniaudio`:** `kira` é voltado a jogos e puxa mais; `miniaudio` traz C (`cc`) e compila uma biblioteca nativa.
- **Chamar programas do sistema (`aplay`, `afplay`, PowerShell):** viola `multiplataforma.md` (sem garantia de existir) e dá latência alta.
- **Arquivos CC0 embutidos:** viável, mas aumenta o binário e o registro de licenças; a síntese cobre o necessário e um conjunto gravado pode vir depois.
- **TTS do sistema para o anúncio:** exige `libspeechd` (Linux) e uma API por plataforma; adiado (spec D2812).

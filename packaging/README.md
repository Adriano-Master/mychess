# Empacotamento (pacotes de distribuição)

`packaging/build-dist.sh` gera os pacotes que ficam versionados em `dist/` (e que o
README da raiz oferece para download). Um alvo por execução, pois o link com LTO
completo usa muita RAM.

```sh
packaging/build-dist.sh linux      # dist/MyChess-0.1.0-alpha-linux-x86_64.tar.gz
packaging/build-dist.sh windows    # dist/MyChess-0.1.0-alpha-windows-x86_64.zip
packaging/build-dist.sh sums       # reescreve dist/SHA256SUMS.txt (também é feito ao fim de cada alvo)
packaging/build-dist.sh stockfish19 DIR   # opcional: pacote do Stockfish 19 em packaging/work/ (fora do dist/)
```

## Variáveis

| Variável | Padrão | Uso |
|---|---|---|
| `VERSION` | `0.1.0-alpha` | versão no nome dos pacotes |
| `DATA_DIR` | `$HOME/chess base` | bases (`Capablanca.*`, `Carlsen.*`) e `Data Base/openings/`; nunca é alterado |
| `NAJDORF_PGN` | `SicilianNajdorf6Bg5.pgn` | PGN de abertura incluído (trocável: ponha outro nome da pasta `openings/`) |
| `SF_DIR` | `$DATA_DIR/Engines/Stockfish` | arquivos do Stockfish 10 do pacote Linux |
| `CARGO_BUILD_JOBS` | `3` | use `1` se o linker for morto por falta de memória |
| `CARGO_TARGET_DIR` | `target/rel` (Linux), `target/rel-win` (Windows) | |
| `SKIP_BUILD=1` | | reaproveita o binário já compilado |
| `NATIVE_WINDOWS=1` | | compila nativamente no Windows (MSVC); usado por `.github/workflows/release.yml` |

`CARGO_INCREMENTAL=0` é forçado. Apague `target/rel*` ao terminar (ocupam vários GB).

## Ferramentas (sem `sudo`)

- Linux: `cargo build --profile dist -p mychess-gui` (precisa dos cabeçalhos do ALSA,
  `libasound2-dev`).
- Windows a partir do Linux: alvo `x86_64-pc-windows-gnu` com
  [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) e o Zig do pacote pip
  `ziglang`:
  ```sh
  python3 -m pip install --user ziglang
  cargo install --locked cargo-zigbuild
  rustup target add x86_64-pc-windows-gnu
  ```
  Versões usadas na geração dos pacotes atuais: ver `packaging/VERSIONS.md`. Não usa
  nenhum SDK da Microsoft. O resultado **nunca foi executado num Windows real**.

## O que cada pacote contém

Pasta raiz `MyChess-<versão>-<so>/`: executável, `portable.flag` (modo portátil: tudo
fica na pasta), `MyChess/{Databases,Engine,Books}/`, `LICENSE`,
`LICENSES-THIRD-PARTY.txt` (gerado por `third_party.py` a partir de `cargo tree`),
`LICENSES-ASSETS.md`, `CREDITS.md` e `LEIA-ME.txt`. Os textos estão em
`packaging/templates/`. O pacote Linux inclui o Stockfish 10 em
`MyChess/Engine/stockfish/`. `pack.py` gera os arquivos de forma reprodutível (ordem,
datas e modos fixos; o zip grava os textos com CRLF).

## Regenerar

1. Compile e empacote o Linux e depois o Windows (um de cada vez).
2. Confira `dist/SHA256SUMS.txt` e atualize os SHA-256 e tamanhos na tabela do
   `README.md` da raiz.
3. Cada arquivo de `dist/` precisa ter menos de 100 MB (limite do GitHub).

O fluxo `.github/workflows/release.yml` repete o mesmo script no GitHub (Linux e
Windows MSVC), extraindo as bases do pacote Linux versionado em `dist/`. Não foi
executado ainda.

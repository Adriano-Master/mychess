---
paths:
  - "**/*.rs"
  - "**/tests/**"
  - "**/benches/**"
---

# Testes

## Comandos
- Suíte completa: `cargo xtask test` (usa `cargo-nextest` quando instalado).
  Portão completo: `cargo xtask ci`.
- Um crate ou filtro: `cargo test -p mychess-core san` (ou
  `cargo nextest run -p mychess-core san`).
- Compatibilidade com o Scid (exige `reference/`): `cargo xtask compat`.
- Snapshots do `insta`: revise com `cargo insta review` (requer
  `cargo install --locked cargo-insta`). **Nunca aceite snapshot sem ler o diff.**

## Pirâmide
| Tipo | Onde | Para quê |
|---|---|---|
| Unitário | `#[cfg(test)] mod tests` no fim do arquivo | Toda função com lógica |
| Integração | `crates/<crate>/tests/*.rs` (só API pública) | Fluxos do crate: ler banco, importar PGN, sessão de engine |
| Propriedade | `proptest` | Round-trips (FEN, SAN, PGN, SI4) e invariantes |
| Perft | `crates/mychess-core/tests/perft.rs` | Geração de lances através do nosso encapsulamento |
| Snapshot | `insta` | Saídas textuais grandes: PGN exportado, relatórios |
| Compatibilidade | funções `compat_*` com `#[ignore]` | Arquivos e bytes do Scid vs PC |
| GUI | `mychess-app` (regras) + `iced_test` (widgets) | Interações e telas, sem janela |
| Benchmark | `crates/<crate>/benches/` (`criterion`, `harness = false`) | Caminhos críticos |

## Convenções
- O nome descreve comportamento e condição, em inglês:
  `parses_fen_with_chess960_castling`, `rejects_truncated_index_file`.
- Organizar, agir, verificar (*arrange–act–assert*); um comportamento por teste;
  `assert!` com mensagem quando o valor sozinho não explica a falha.
- `pretty_assertions::assert_eq` para estruturas grandes; `rstest` para
  tabelas de casos.
- Teste o **contrato** (especificação, comportamento do Scid), não detalhes
  internos da implementação.
- Todo bug corrigido ganha um teste de regressão que falha sem a correção.
- Determinismo: sem rede, sem relógio real (injete o tempo), sem depender da
  ordem de `HashMap` ou de listagem de diretório. Os arquivos
  `proptest-regressions/` são versionados.
- Multiplataforma: `tempfile` para temporários, `Path::join`, nada de `/tmp`
  fixo nem de finais de linha implícitos.

## Fixtures
- Ficam em `crates/<crate>/tests/fixtures/`, pequenas e com bytes preservados
  (`.gitattributes` marca `-text`).
- Só conteúdo com licença compatível: PGNs de partidas (lances são fatos;
  cabeçalhos e comentários escritos por nós), bancos SI4 **gerados por nós** com
  o Scid vs PC a partir desses PGNs, livros Polyglot gerados por nós.
- Dados que vêm com o Scid vs PC (`bases/`, `books/`, `spelling.ssp`) só são
  usados em testes `compat_*`, lidos direto de `reference/`, nunca copiados.

## Testes de compatibilidade (`compat_*`)
```rust
fn scid_reference() -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../reference/scid_vs_pc-4.27");
    assert!(dir.is_dir(), "Scid vs PC reference not found at {} (see docs/referencia-scid.md)", dir.display());
    dir
}

#[test]
#[ignore = "requires reference/scid_vs_pc-4.27: run with `cargo xtask compat`"]
fn compat_reads_every_game_of_bundled_databases() { /* ... */ }
```

## Casos de borda obrigatórios por área
- **Regras**: roque (perda de direitos ao mover rei ou torre e quando a torre
  é capturada; roque através de casa atacada; Chess960 com rei e torres em
  casas variadas); en passant (só quando legal; casa no FEN); promoção para
  todas as peças, com captura, `e8=Q+`; mate, afogamento, material insuficiente,
  regras dos 50 e 75 lances, repetição tripla e quíntupla; lance nulo `--` (o
  Scid aceita em variações).
- **SAN**: desambiguação por coluna, fileira e ambas; sufixos `+` e `#`;
  anotações `!`, `?`, `!?`; leitura tolerante (`0-0`, `e8Q`, `Nbd7`).
- **PGN**: variações aninhadas; comentários `{}` e `;`; NAGs `$n` e simbólicos;
  escape `%`; aspas escapadas em tags; tags ausentes ou duplicadas;
  `[SetUp "1"]` + `[FEN ...]`; resultado divergente entre tag e lances; BOM;
  CRLF; Latin-1; linhas enormes; partida vazia; lixo entre partidas; comandos
  embutidos `[%clk ...]`, `[%eval ...]`, `[%csl ...]`, `[%cal ...]`.
- **Formatos binários**: ver `formatos-binarios.md` (limites, datas parciais,
  truncamento e corrupção).
- **Engines**: handshake lento, engine que morre, saída inválida,
  `bestmove (none)`, opções fora dos limites (ver `engines.md`).

## Perft (contagens de referência)
- Posição inicial: d1 = 20, d2 = 400, d3 = 8902, d4 = 197281, d5 = 4865609.
- "Kiwipete" `r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1`:
  d1 = 48, d2 = 2039, d3 = 97862, d4 = 4085603.
- Demais posições em https://www.chessprogramming.org/Perft_Results.
  Profundidades caras ficam com `#[ignore]`.

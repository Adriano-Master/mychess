# Benchmarks

Linha de base de desempenho dos caminhos críticos, medida com `criterion`
(`cargo bench -p <crate>`). Regras em `.claude/rules/desempenho-e-unsafe.md`:
uma regressão acima de **10 %** contra a linha de base bloqueia a revisão. O CI
só compila os benchmarks (`cargo bench --no-run --workspace`); os números abaixo
são medidos à mão e **preenchidos pelo dono do projeto**.

Estado em 2026-10-21: todos os benchmarks do M1 existem (`mychess-core`: `eco`,
`position`; `mychess-pgn`: `pgn`, `epd`, `oracle`) e têm a primeira medição
abaixo. Os números são de **amostra curta** (ver "Método da primeira medição");
repita-os com as três execuções recomendadas antes de usá-los como linha de base
definitiva.

## Como medir

1. Feche programas pesados; ligue o computador na tomada e fixe o governador de
   CPU em `performance` (Linux) ou use o plano de alto desempenho (Windows).
2. `cargo bench -p mychess-core` e `cargo bench -p mychess-pgn` no perfil
   `release` do workspace (`lto = "thin"`); compare com `-- --save-baseline m1`
   e depois `-- --baseline m1`.
3. Rode pelo menos três vezes e registre a mediana; anote o desvio se for alto.
4. Os benchmarks que leem `scid.eco` o buscam em `reference/` (não está no
   repositório); sem o arquivo, o benchmark correspondente é pulado.

## Máquina e ambiente (preencher)

| Item | Valor |
|---|---|
| Data da medição | 2026-10-21 |
| CPU (modelo, núcleos/threads) | 13th Gen Intel Core i5-13450HX, 16 threads (2 por núcleo; híbrido P+E) |
| Memória | 15 GiB |
| Armazenamento (SSD/NVMe/HDD) | NVMe (os benchmarks não fazem I/O de disco) |
| Sistema operacional e versão | Linux 7.0.0-38-generic |
| Compilador | `rustc 1.99.0 (b940084d7 2026-09-28)` |
| Perfil | `release` do workspace (`lto = "thin"`), `cargo bench` |
| Commit / estado da árvore | sem repositório git na árvore de trabalho; estado do fechamento do M1 em 2026-10-21 |
| Governador de CPU / alimentação | `powersave` (não foi alterado; é um notebook, alimentação não verificada) |

**Método da primeira medição.** Um benchmark por vez, com
`-- --warm-up-time 2 --measurement-time 4..5 --sample-size 10..30`. A máquina
**não estava totalmente quieta**: durante a medição havia um teste em outro
processo ocupando um núcleo (carga média 1,6 a 3,4 em 16 threads), e uma
primeira rodada com carga ~9 chegou a dobrar os tempos de `legal_moves` e
`zobrist`; essa rodada foi descartada. Nas execuções com carga ~3 os tempos de
`position/*` variaram menos de 10 % entre três repetições. Os números abaixo são
a **mediana do intervalo central** impresso pelo `criterion`; "Desvio" é a
amplitude observada entre repetições (ou o intervalo de confiança do
`criterion`, quando só houve uma).

## Resultados por benchmark

"Meta" é o alvo da especificação; "Medido" e "Desvio" são do dono do projeto.

### `mychess-core`

| Benchmark | O que mede | Meta | Medido | Desvio | Situação |
|---|---|---|---|---|---|
| `eco/load_scid_eco` | carga e indexação de `scid.eco` (10 360 entradas) | < 100 ms (m1-4, D53) | 58,0 ms | IC 56,9 a 58,6 ms | atingida (`benches/eco.rs`) |
| `eco/classify_80_plies` | classificar uma partida de 80 meios-lances | 81 buscas sem alocar; sem meta absoluta, registrar linha de base | 4,67 µs (~58 ns por meio-lance) | IC 4,60 a 4,73 µs | linha de base |
| `eco/classify_batch_10000_random_games` | classificação em lote | linha de base (o lote de 1 milhão de partidas sintéticas da especificação é extrapolado) | 34,4 ms por 10 000 partidas (3,4 µs/partida; ~3,4 s extrapolado para 1 milhão) | IC 34,3 a 34,4 ms | linha de base |
| `core_parse_san/strict` | 210 lances de 5 aberturas reais, cada um na sua posição (`benches/position.rs`) | linha de base; caminho quente da importação | 92 a 99 µs por 210 lances (~440 ns/lance; ~2,2 Mlances/s) | ±4 % | linha de base |
| `core_parse_san/tolerant` | o mesmo corpus | idem | 69 a 73 µs por 210 lances (~335 ns/lance; ~2,9 Mlances/s) | ±3 % | linha de base (o modo tolerante mediu ~25 % mais rápido que o estrito; causa não investigada) |
| `position/from_fen` | 212 posições (as do corpus + Chess960 518 e 959) | linha de base | 50,1 a 52,5 µs (~240 ns/posição) | ±3 % | linha de base |
| `position/fen` | as mesmas 212 posições | linha de base | 60,0 a 60,6 µs (~283 ns/posição) | ±1 % | linha de base |
| `position/legal_moves` | as mesmas 212 posições | linha de base | 49,6 a 56,5 µs (~250 ns/posição) | ±7 % | linha de base |
| `position/zobrist` | as mesmas 212 posições | linha de base | 2,37 a 2,51 µs (~11 ns/posição) | ±3 % | linha de base |

### `mychess-pgn`

| Benchmark | O que mede | Meta | Medido | Desvio | Situação |
|---|---|---|---|---|---|
| `read_games/plain_8MB` (árvore completa, lances validados; PGN sintético de 8 MiB, duas partidas repetidas) | leitura em streaming | >= 15 MB/s por núcleo (m1-3, D38) | 16,11 MiB/s (16,9 MB/s) | IC 15,99 a 16,24 MiB/s | atingida (margem de ~13 %) |
| `read_games/annotated_8MB` (a mesma, com comentários, NAGs e uma variação por partida) | leitura em streaming | >= 15 MB/s por núcleo | 13,47 MiB/s (14,1 MB/s) | IC 13,21 a 13,76 MiB/s | **ABAIXO DA META** (~6 % abaixo de 15 MB/s). Em medições anteriores o dono registrou 12,6 a 16,5 MiB/s. A leitura sem anotações cumpre; a anotada fica no limite |
| `read_vs_oracle/mychess_pgn_full_parse` x `pgn_reader_syntax_only` (`benches/oracle.rs`) | leitura completa do `mychess-pgn` x `pgn-reader` 0.29 só com sintaxe (sem validar lances, sem árvore) | registrar a razão; sem meta | 15,16 MiB/s x 615,7 MiB/s: razão ~40x | uma execução | informativo. A razão é grande porque o oráculo não valida lances nem monta árvore, tags nem posições |
| `scan_tags/tags_only_8MB` | varredura só de cabeçalhos (sobre partidas já cortadas) | >= 300 MB/s | 867 MiB/s (909 MB/s) | IC 860 a 876 MiB/s | atingida |
| `scan_tags/chunker_only_8MB` | só cortar o fluxo em partidas | (apoio; sem meta própria) | 400 MiB/s (419 MB/s) | IC 399 a 402 MiB/s | acima de 300 MB/s |
| `write_game/default_options` | escrita de PGN estrito (2 MiB de partidas ricas) | >= 50 MB/s | 64,8 MiB/s (68 MB/s) | IC 64,0 a 66,0 MiB/s | atingida |
| `write_game/scid_export` | escrita no estilo da exportação do Scid | >= 50 MB/s | 64,8 MiB/s (68 MB/s) | IC 64,6 a 65,0 MiB/s | atingida |
| `parse_san/strict` e `/tolerant` do crate PGN (partida de 85 lances) | lances de uma partida real | linha de base | 40,0 µs (~2,12 Mlances/s) e 30,9 µs (~2,75 Mlances/s) | ±3 % | linha de base |
| `epd/epd_read` | 200 000 linhas sintéticas (com `bm`, `id`, `ce`; 18,7 MiB), reduzido de 1 milhão | linha de base | 244,5 ms (~1,2 µs/linha; 76,6 MiB/s; ~0,82 M linhas/s) | IC 241 a 249 ms | linha de base. Extrapolação para 1 milhão: ~1,2 s |
| `epd/epd_write` | as mesmas 200 000 linhas | linha de base | 307,2 ms (~1,5 µs/linha; 61,0 MiB/s; ~0,65 M linhas/s) | IC 306 a 308 ms | linha de base. A escrita é ~25 % mais lenta que a leitura porque `write` valida o livro inteiro (formata cada registro uma vez para validar e outra para gravar); candidato a otimização, sem meta violada |
| `GameSession::edit` (teste `edit_cost_on_a_thousand_plies` em `crates/mychess-app/tests/m2_1_review_fixes.rs`, `--release -- --ignored`) | uma edição (comentário) em partida de 1000 lances: clone do `Game` + `same_content` + ponto de desfazer | < 1 ms (m2-1, "Desempenho") | 87 µs por edição | uma execução de 200 edições | atingida (~11x de folga); sem necessidade de ADR de snapshots persistentes |
| `build_notation` (teste `notation_perf_300_moves_150_variations` em `crates/mychess-app/src/notation/tests.rs`, `--release -- --ignored --nocapture`) | montar o `NotationDoc` (layout recuado, opções padrão) de 300 lances de linha principal + 150 variantes (590 nós, aninhamento até 3, comentários e NAGs) | <= 5 ms (m2-4, D168) | 251 µs (mediana de 30; mín. 222 µs, máx. 386 µs) | uma execução | atingida (~20x de folga). Não inclui `keep_revisions` nem o *shaping* da GUI |
| `NotationTable::from_doc` (teste `table_perf_300_moves_150_variations` em `crates/mychess-app/src/notation/table_tests.rs`, `--release -- --ignored --nocapture`) | projetar o `NotationDoc` em linhas (layout em tabela, com tempos de `[%clk]`) de 300 lances de linha principal + 150 variantes (590 nós, 745 linhas) | <= 3 ms (m2-5, D253; o teste exige < 5 ms) | 138 µs (mediana de 30; mín. 126 µs, máx. 257 µs) | uma execução | atingida (~20x de folga). Não inclui `build_notation` (320 µs na mesma máquina) nem `keep_revisions` nem o *shaping* da GUI |

As metas de PGN são **iniciais** e devem ser calibradas na primeira medição
(m1-3, D38): se a medição mostrar que a meta era irreal, ajuste a meta aqui e
na especificação, com o motivo.

### `mychess-app`: relatório de abertura

| Benchmark | O que mede | Meta | Medido | Desvio | Situação |
|---|---|---|---|---|---|
| `a_report_over_100000_games_takes_under_five_seconds` (`crates/mychess-app/tests/m7_2b_perf.rs`, `--release -- --ignored --nocapture`) | o `Effect::Compute` (busca de árvore em 2 fases, `FastMode::Slow`, sem cache, e o relatório) sobre 100 000 partidas sintéticas de um PGN aberto em um slot (8 linhas de abertura, ~7 lances cada, posição de referência `1.e4 e5 2.Nf3`) | < 5 s (D3627) | 81 ms (~60x de folga) | uma execução, 2026-10-29, `CARGO_PROFILE_RELEASE_DEBUG=false`, 16 threads (i5-13450HX), NVMe | atingida |
| `a_report_of_a_player_with_100000_games_takes_under_five_seconds` (`crates/mychess-app/tests/m7_3b_perf.rs`, `--release -- --ignored --nocapture`) | o `Effect::Compute` do relatório de jogador (seleção do jogador pelo índice, busca por posição e relatório) sobre 100 000 partidas sintéticas de um mesmo jogador (Brancas) em PGN aberto em slot, do início da partida (100 000 partidas no relatório) e da posição `1.e4 e5 2.Nf3` (87 520) | < 5 s (M7-2/D3627) | 90 ms e 95 ms (~50x de folga) | uma execução, 2026-10-29, `CARGO_PROFILE_RELEASE_DEBUG=false`, 16 threads (i5-13450HX), NVMe; não medido com SI4 em disco | atingida |

Limite da medição: a base é um PGN em memória (o slot decodifica antes); um
SI4 em disco lê os mesmos registros por `record_bytes` e não foi medido. Como a
folga é de 60x, a leitura em lotes (um *lock* por bloco) não foi feita.

## Histórico

| Data | Commit | Mudança | Resumo dos resultados |
|---|---|---|---|
| 2026-10-21 | sem git | primeira medição do M1 (amostra curta, máquina com um núcleo ocupado por outro processo) | `read_games` anotado 13,5 MiB/s abaixo da meta de 15 MB/s; demais metas atingidas; `epd_read` ~1,2 µs/linha, `epd_write` ~1,5 µs/linha |

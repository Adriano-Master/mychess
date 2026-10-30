---
paths:
  - "crates/**/*.rs"
---

# Desempenho e `unsafe`

## Desempenho
O Scid vs PC abre bancos com milhões de partidas e busca neles em segundos. O
MyChess precisa, no mínimo, empatar.

- **Meça antes de otimizar**: benchmark `criterion` em `benches/` do crate e
  comparação antes/depois (`cargo bench -p <crate> -- --save-baseline antes`,
  depois `--baseline antes`). Para profiling, use `samply record` (Linux, macOS
  e Windows) ou `perf` com flamegraph, em `--release` com
  `CARGO_PROFILE_RELEASE_DEBUG=true`.
- **Caminhos críticos**: decodificação SI4, busca e filtro em todas as partidas,
  importação de PGN, estatísticas da árvore de aberturas, geração de lances e
  lista de partidas na GUI.
- Em laços quentes: nenhuma alocação por item (reutilize buffers com `clear()`);
  nada de `format!`/`String` por partida; `SmallVec` para listas curtas; parsing
  sobre `&[u8]` (com `memchr` quando ajudar); índices e `Vec` no lugar de
  `HashMap` para chaves pequenas e densas.
- **Memória**: índice compacto como o do Scid, nomes internados (`NameId` em vez
  de `String` por partida), filtros como `Vec<u8>` ou bitset. Calcule os
  bytes por partida ao desenhar uma estrutura.
- **Paralelismo**: `rayon` (`par_iter`, `par_chunks`) nas buscas, com resultado
  determinístico (mesma ordem da versão sequencial). Nenhum lock dentro do laço.
- **I/O**: `BufReader`/`BufWriter` com buffer grande; `memmap2` para varredura
  somente leitura de arquivos grandes (exige `unsafe`, ver abaixo).
- **Async**: nunca bloqueie dentro de `async`. I/O pesado e CPU vão para
  `spawn_blocking` ou `rayon`.
- **GUI**: `view` barata, dados derivados em cache no estado, listas grandes
  virtualizadas, `canvas::Cache` para camadas estáticas.

## `unsafe`
O workspace nega `unsafe_code`. Uma exceção exige **todos** os itens:

1. **Necessidade comprovada**: FFI, `memmap2::Mmap::map`, ou ganho medido em
   benchmark que o código seguro não alcança.
2. **Isolamento**: módulo pequeno com API segura; `#[allow(unsafe_code)]` só
   nesse módulo, com comentário do motivo.
3. **`// SAFETY:`** em cada bloco, explicando por que as invariantes valem
   (lints `undocumented_unsafe_blocks` e `multiple_unsafe_ops_per_block`).
4. **Testes** cobrindo a fronteira; quando possível, também
   `cargo +nightly miri test -p <crate>` (opcional, local).
5. **Destaque no relatório** da tarefa. O revisor trata `unsafe` sem esses itens
   como **bloqueante**.

Exemplo aceitável: o `mychess-db` mapeia o `.sg4` em memória, com o arquivo
aberto somente para leitura, e descarta o mapeamento antes de qualquer escrita
ou compactação.

---
paths:
  - "crates/mychess-db/**"
  - "crates/mychess-book/**"
  - "docs/formatos/**"
---

# Formatos binários (SI4, Polyglot, caches)

- **Especificação primeiro.** Antes de codificar, `docs/formatos/<formato>.md`
  descreve o layout: offset, largura, ordem de bytes, significado e limites de
  cada campo, com referência ao código do Scid (arquivo e função). O código cita
  a seção da especificação.
- **Decodificação defensiva.** Todo campo lido é validado: limites, valores de
  enum, tamanhos contra o tamanho real do arquivo. Arquivo truncado ou
  corrompido devolve `Err` com contexto (offset, número da partida); nunca
  panic, laço infinito ou alocação desproporcional.
- Leitura e escrita com funções pequenas e testadas (`read_u24_be` e afins)
  sobre `&[u8]`/`Read`. Nada de `transmute` ou struct `#[repr(C)]` sobre bytes.
- **Compatibilidade nos dois sentidos**: o que o MyChess grava, o Scid vs PC
  4.27 lê, e vice-versa. Round-trip *ler → gravar → comparar bytes* com arquivos
  gerados pelo próprio Scid (testes `compat_*`).
- **Atomicidade**: conteúdo novo vai para um temporário no mesmo diretório,
  seguido de `rename`. Atualização no próprio arquivo só onde o Scid também faz
  isso, com o mesmo protocolo. O banco nunca fica inconsistente se o processo
  morrer no meio da gravação.
- **Concorrência**: um único processo grava num banco por vez. Detecte e recuse
  um banco já aberto para escrita, como o Scid; se não for possível, documente o
  comportamento.
- **Desempenho**: ver `desempenho-e-unsafe.md` (mmap, sem alocação por partida).
- **Testes obrigatórios**:
  - proptest de round-trip (codificar e decodificar) com partidas legais
    aleatórias;
  - bordas dos limites: nomes no tamanho máximo, número máximo de partidas e de
    nomes, datas parciais (`????.??.??`), todas as flags;
  - arquivos corrompidos (truncados em cada campo, bytes aleatórios), que devem
    resultar em `Err`.

---
paths:
  - "**/*.rs"
---

# Erros e panics

## Princípio
**Entrada externa nunca derruba o programa.** Arquivos (PGN, SI4, livros,
preferências), saída de engines, rede e entrada do usuário podem estar
corrompidos ou ser maliciosos. O resultado é sempre `Err`, nunca panic, laço
infinito ou alocação desproporcional.

## Bibliotecas (todos os crates, exceto os binários e o `xtask`)
- Erros com `thiserror`: um `enum Error` por crate (ou por módulo grande,
  agregados com `#[from]`), com o alias
  `pub type Result<T, E = Error> = std::result::Result<T, E>;`.
- Variantes carregam contexto de diagnóstico: caminho, número da partida,
  offset, linha e coluna do PGN.
- Mensagens `#[error("...")]` em inglês, minúsculas, sem ponto final; servem
  para log e depuração. A GUI traduz a partir da **variante**, não da mensagem.
- Proibido nas APIs de biblioteca: `anyhow`, `Box<dyn Error>`, `String` como erro.

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("i/o error on {path}")]
    Io { path: PathBuf, #[source] source: std::io::Error },
    #[error("corrupt index entry for game {game}: {reason}")]
    CorruptIndex { game: GameNumber, reason: &'static str },
    #[error("unsupported SI4 version {0}")]
    UnsupportedVersion(u16),
}
```

## Binários (`mychess-gui`, `mychess-cli`, `xtask`)
- `anyhow::Result` com `.context("...")` nas bordas. O `main` registra o erro
  com `tracing` e mostra uma mensagem amigável.

## Quando panic é aceitável
- Só para violação de invariante interna, impossível por construção:
  `expect("invariant: <explicação>")` ou `unreachable!("<explicação>")`, com
  `# Panics` documentado em itens públicos.
- Nunca para validar entrada. Nunca `unwrap()` em código de produção (lint
  `unwrap_used`; testes podem).
- Indexação `slice[i]` só com limite comprovado. Em parsers prefira `get(i)`,
  `split_at_checked` e `chunks_exact`, devolvendo erro.
- Aritmética sobre dados lidos de arquivo: `checked_*`/`try_from`. Overflow em
  release vira valor errado silencioso.
- Tamanhos lidos de arquivo limitam alocações: valide contra o tamanho real do
  arquivo antes de `Vec::with_capacity(n)`.

## Propagação e registro
- Use `?`. Acrescente contexto ao atravessar fronteiras (`map_err` para a
  variante certa).
- Registre o erro **uma vez**, na borda que o trata (`mychess-app`/GUI), com
  `tracing::error!`/`warn!` e campos estruturados. Nada de "loga e repassa" em
  cada camada.
- Erro recuperável na GUI mostra mensagem e mantém o app funcionando. Nenhuma
  falha deixa arquivo do usuário corrompido (escrita em temporário + troca
  atômica).

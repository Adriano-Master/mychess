---
paths:
  - "**/*.rs"
---

# Estilo e idiomas Rust

Complementa o `rustfmt` (aplicado automaticamente por hook após cada edição) e
os lints do workspace (`[workspace.lints]` no `Cargo.toml`). Em caso de dúvida,
siga as [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html).

## Idioma
- Identificadores, comentários, rustdoc e mensagens de erro: **inglês**.
- Texto exibido ao usuário nunca fica no código Rust: vai para os arquivos
  Fluent em `assets/i18n/` (ver `gui-iced.md`).

## Nomes
- `UpperCamelCase` para tipos, traits e variantes; `snake_case` para funções,
  módulos e variáveis; `SCREAMING_SNAKE_CASE` para constantes.
- Siglas são escritas como palavras: `Pgn`, `Fen`, `San`, `Uci`, `Si4`, `Eco`,
  `Nag` (`PgnReader`, não `PGNReader`).
- Conversões: `as_*` (barata, empréstimo), `to_*` (custosa ou cópia), `into_*`
  (consome). Getters sem `get_` (`game.white()`).
- Construtores: `new`, `with_*`, `from_*`. Tipo que se lê de texto implementa
  `FromStr` (`"e4".parse::<Square>()`); notação para humanos implementa `Display`.
- Vocabulário de xadrez em inglês padrão: `ply`, `move_number`,
  `side_to_move`, `castling_rights`, `en_passant`, `mainline`, `variation`,
  `comment`, `nag`, `tag` (par de cabeçalho PGN), `name_base`, `filter`.

## Tipos
- Tipos fortes no lugar de primitivos soltos: `GameNumber(u32)`,
  `NameId(u32)`, `Ply(u16)`, `Elo(u16)`. Construtores validam
  (*parse, don't validate*).
- `enum` no lugar de `bool` em parâmetros (`Orientation::Black`, não
  `flipped: bool`) e no lugar de valores sentinela. O Scid usa `0` para
  "desconhecido"; no MyChess isso vira `Option`, convertido na fronteira do
  formato.
- Derive o que fizer sentido: `Debug` sempre; `Clone`, `Copy` (tipos pequenos),
  `PartialEq`, `Eq`, `Hash`, `Default`; `PartialOrd`/`Ord` quando houver ordem
  natural.
- Conversões numéricas com `TryFrom`/`try_into`. `as` só com prova de que não
  trunca nem muda sinal, explicada em comentário.

## Propriedade e empréstimo
- Parâmetros emprestados (`&str`, `&[T]`, `&Path` ou `impl AsRef<Path>`);
  retorno com posse.
- Nada de `clone()` só para calar o borrow checker: reestruture. Use `Cow`
  quando o caminho comum não precisa alocar.
- Nada de `Rc<RefCell<_>>` ou `Arc<Mutex<_>>` no domínio. Estado mutável
  compartilhado indica desenho errado no MVU (ver `docs/arquitetura.md`).

## Módulos e visibilidade
- Um conceito por módulo; arquivo `foo.rs` + diretório `foo/` (sem `mod.rs`).
- `pub(crate)` por padrão; `pub` só para a API do crate, reexportada em `lib.rs`.
- Imports explícitos; `use super::*` só em módulos de teste.

## Documentação e comentários
- `//!` no topo de cada módulo explicando o propósito.
- `///` em todo item público (o lint `missing_docs` cobra); exemplo executável
  (doctest) nas APIs centrais; seção `# Panics` quando aplicável.
- Comentários explicam o **porquê**. Comportamento herdado do Scid cita a origem:
  `// Scid: src/game.cpp, Game::DecodeMoves()`.
- Sem código comentado "para depois" e sem `todo!()`/`unimplemented!()` em código
  entregue.

## Genéricos, traits e macros
- Genéricos/`impl Trait` em caminhos quentes; `dyn Trait` em fronteiras
  heterogêneas (por exemplo, formatos de banco diferentes atrás de uma interface).
- Traits pequenas e coesas. Não crie trait com uma única implementação "para o
  futuro".
- Evite macros próprias quando função ou genérico resolve.

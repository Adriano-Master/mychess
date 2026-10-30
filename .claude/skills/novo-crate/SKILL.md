---
name: novo-crate
description: Cria um crate novo no workspace do MyChess seguindo as convenções. Inclui manifesto herdando do workspace, lints, documentação do crate e a regra de camada no xtask e em docs/arquitetura.md. Use quando um marco do roteiro exigir um crate novo, como mychess-book ou mychess-cli.
argument-hint: '<nome, ex.: mychess-book> <descrição curta em inglês>'
---

# Novo crate: $ARGUMENTS

1. **Confirme o lugar na arquitetura.** Veja em `docs/arquitetura.md` (seção
   3) a responsabilidade e as dependências permitidas do crate. Se ele não
   estiver planejado lá, a criação é uma decisão de arquitetura e exige uma
   ADR primeiro.
2. **Crie `crates/<nome>/Cargo.toml`:**
   ```toml
   [package]
   name = "<nome>"
   description = "<descrição em inglês>"
   version.workspace = true
   edition.workspace = true
   rust-version.workspace = true
   license.workspace = true
   publish.workspace = true

   [dependencies]
   # somente entradas `nome.workspace = true`

   [lints]
   workspace = true
   ```
   Se for binário, acrescente uma seção `[[bin]]` com nome curto.
3. **Crie `src/lib.rs`** (ou `src/main.rs`) com documentação `//!`:
   responsabilidade, conteúdo planejado (marco do roteiro) e regra de
   dependência.
4. **Registre no workspace**, se outros crates vão depender dele: adicione
   `<nome> = { path = "crates/<nome>" }` em `[workspace.dependencies]` do
   `Cargo.toml` raiz.
5. **Registre a regra de camada** em `ALLOWED_DEPS` (`xtask/src/main.rs`) e
   atualize a tabela da seção 3 de `docs/arquitetura.md` (situação "criado").
   Se outro crate passa a poder usá-lo, atualize a linha desse crate nos dois
   lugares.
6. **Regras próprias.** Se o crate tem regras específicas (um protocolo, um
   formato), crie ou estenda um arquivo em `.claude/rules/` com `paths`
   apontando para `crates/<nome>/**`.
7. **Valide:** `cargo xtask arch` e `cargo xtask lint` devem passar.

# MyChess: instruções do projeto

Reimplementação em Rust do **Scid vs PC 4.27** (banco de dados, análise e
estudo de partidas de xadrez) com interface gráfica moderna, para **Linux,
Windows e macOS**. A meta é ter todas as funcionalidades do Scid vs PC; o plano
está em `docs/roteiro.md`.

## Mapa do repositório
- `crates/`: o workspace (tabela abaixo). `xtask/`: automação (`cargo xtask`).
- `docs/`: `arquitetura.md`, `adr/` (decisões), `roteiro.md`,
  `referencia-scid.md` (mapa do código do Scid), `specs/` (especificações por
  funcionalidade) e `formatos/` (formatos binários).
- `assets/` (a criar): peças SVG, fontes, sons e traduções Fluent, com as
  licenças em `assets/LICENSES.md`.
- `reference/scid_vs_pc-4.27/`: código original do Scid vs PC, **somente
  leitura** e fora do git. Buscas a partir da raiz o ignoram; use caminho
  explícito: `rg -n "<padrão>" reference/scid_vs_pc-4.27/src`.

## Arquitetura (detalhes em `docs/arquitetura.md`)
Workspace em camadas com **MVU** (Elm Architecture), o equivalente do MVC com
fluxo de dados unidirecional:

| Papel | Crate | Conteúdo |
|---|---|---|
| Model | `mychess-core` | Regras sobre `shakmaty`, SAN/FEN/UCI, árvore da partida, ECO |
| Model | `mychess-pgn` | PGN/EPD (leitura sobre `pgn-reader`, escrita própria) |
| Model | `mychess-db` | Bancos SI4 compatíveis com o Scid, buscas, árvore, manutenção |
| Model | `mychess-engine` | Engines UCI e XBoard |
| Controller | `mychess-app` | `AppState`, `Message`, `update(&mut AppState, Message) -> Vec<Effect>` (puro) e `perform(Effect) -> Message` (async) |
| View | `mychess-gui` | iced 0.14, binário `mychess` |

As dependências só apontam para baixo. A GUI só conhece `mychess-app` e
`mychess-core`, e nada abaixo dela depende de iced. O `cargo xtask arch`
verifica isso (tabela `ALLOWED_DEPS` em `xtask/src/main.rs`).

## Comandos
| Para | Comando |
|---|---|
| Portão completo (obrigatório antes de concluir uma tarefa) | `cargo xtask ci` |
| Checagem rápida | `cargo check --workspace --all-targets` |
| Lints (avisos viram erro) | `cargo xtask lint` |
| Testes | `cargo xtask test` ou `cargo test -p <crate> <filtro>` |
| Camadas | `cargo xtask arch` |
| Compatibilidade com o Scid | `cargo xtask compat` |
| Rodar o app (a partir do M2) | `cargo run -p mychess-gui` |

O toolchain está fixado em `rust-toolchain.toml` (1.99.0, edition 2024). Um hook
roda o `rustfmt` após cada edição de arquivo `.rs` e devolve erros de sintaxe
na hora.

## Agentes e fluxo de trabalho
Agentes em `.claude/agents/`, todos com Sonnet 5.5 e esforço médio:
- **implementador**: código de produção a partir de uma especificação.
- **testador**: testes derivados da especificação (não da implementação); não
  altera código de produção.
- **revisor**: revisão somente leitura com veredito; um hook bloqueia comandos
  de escrita dele.

Para uma funcionalidade, use `/nova-funcionalidade <item>`: especificação →
implementador → testador → revisor, com iterações. A sessão principal orquestra,
porque subagentes não chamam outros subagentes. Para pesquisar o Scid, use
`/consultar-scid <tema>`; a especificação vai para `docs/specs/`. Para um crate
novo, use `/novo-crate`.

### Definição de pronto
1. O comportamento segue a especificação (e o Scid, quando aplicável), com os
   desvios documentados.
2. Há testes novos para o comportamento e os casos de borda, e `cargo xtask ci`
   está verde.
3. O revisor não deixou achado bloqueante ou importante em aberto.
4. A documentação está atualizada: rustdoc; `docs/` quando a arquitetura ou um
   formato mudar; item marcado no `docs/roteiro.md`.

## Regras de ouro
1. `reference/` é somente leitura, inclusive via shell. Há bloqueio por
   permissão e por hook.
2. Entrada externa (arquivos, engines, rede, usuário) nunca causa panic. Nada
   de `unwrap()` em código de produção.
3. `update` e `view` nunca bloqueiam: I/O e cálculo longo viram `Effect`.
4. `unsafe` é negado no workspace. Exceções só conforme
   `.claude/rules/desempenho-e-unsafe.md`.
5. Dependência nova só via `[workspace.dependencies]`, justificada e com licença
   compatível com GPL-3.0-or-later (`.claude/rules/dependencias-e-licencas.md`).
6. Formatos do Scid vs PC (SI4 e outros) são compatíveis **byte a byte**.
7. Nada específico de uma plataforma: caminhos, separadores, finais de linha,
   atalhos (`.claude/rules/multiplataforma.md`).
8. Não faça commit, push nem operação destrutiva no git sem pedido explícito
   do usuário. Commits seguem o Conventional Commits, em inglês
   (`feat(core): parse SAN castling`).
9. Decisão estrutural nova exige uma ADR em `docs/adr/`. Não reabra uma ADR
   aceita sem motivo novo.

## Idioma
- Código, identificadores, rustdoc, comentários e mensagens de erro e de
  commit: **inglês**.
- Documentação do projeto (`docs/`, `.claude/`, `README.md`) e conversa com o
  usuário: **português (pt-BR)**.
- Interface do app: Fluent, com en-US (padrão) e pt-BR desde o início.

## Regras detalhadas (`.claude/rules/`)
Carregadas automaticamente conforme os arquivos tocados:
- `rust-estilo`, `erros-e-panics` e `testes`: arquivos `.rs`.
- `desempenho-e-unsafe`: crates.
- `gui-iced`: `mychess-gui`.
- `formatos-binarios`: `mychess-db` e `docs/formatos/`.
- `engines`: `mychess-engine`.
- `dependencias-e-licencas`: `Cargo.toml`, `deny.toml`, `assets/`.
- `referencia-scid`: `reference/`.
- `multiplataforma`: sempre.

## Licença
GPL-3.0-or-later (`docs/adr/0004-licenca.md`, que ainda aguarda a confirmação
do dono do projeto). Código portado do Scid leva a origem e o copyright no
cabeçalho. Dados que vêm com o Scid vs PC (bancos, livros, `spelling.ssp`) não
entram no repositório.

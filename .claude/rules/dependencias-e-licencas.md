---
paths:
  - "**/Cargo.toml"
  - "Cargo.lock"
  - "deny.toml"
  - "assets/**"
---

# Dependências e licenças

## Onde declarar
- Versões **somente** em `[workspace.dependencies]` do `Cargo.toml` raiz. Nos
  crates: `nome.workspace = true`, mais `features = [...]` se precisar.
- `Cargo.lock` é versionado (é uma aplicação). `cargo update` é uma tarefa
  deliberada, seguida de `cargo xtask ci`; nunca efeito colateral de outra
  mudança.
- Proibido: dependências `git`, `path` fora do workspace, versão `*`.

## Antes de adicionar uma dependência nova
Responda no relatório da tarefa:
1. **Precisa mesmo?** A `std` ou uma dependência já existente resolve?
2. **Licença** compatível com GPL-3.0-or-later e presente em `deny.toml`
   (`[licenses] allow`). Verifique a árvore toda: `cargo deny check licenses`
   ou `cargo tree -e normal -i <crate>`.
3. **Manutenção**: release recente, mantenedores ativos, uso amplo.
4. **Multiplataforma**: compila no Linux, no Windows e no macOS sem bibliotecas
   de sistema (Rust puro; TLS com `rustls`, nunca OpenSSL).
5. **Custo**: `default-features = false` quando os padrões puxam peso (por
   exemplo, `image`); veja o impacto com `cargo build --timings`.
6. **Camadas**: crate de GUI só no `mychess-gui`; processos e async no
   `mychess-engine`/`mychess-app`. O `cargo xtask arch` verifica só os crates
   internos; os externos são com você.

As dependências já listadas em `[workspace.dependencies]` foram avaliadas e
estão pré-aprovadas.

## Licença do MyChess e código de terceiros
- O MyChess é **GPL-3.0-or-later** (`docs/adr/0004-licenca.md`).
- Código portado do Scid vs PC leva cabeçalho de origem (ver
  `referencia-scid.md`).
- Copiar código de outro projeto: só com licença compatível, preservando o aviso
  de copyright e registrando a origem.

## Assets (peças, fontes, sons, ícones)
- Todo asset novo é registrado em `assets/LICENSES.md`: arquivo, origem (URL),
  autor e licença (identificador SPDX).
- Licenças aceitas: as compatíveis com GPLv3 (GPL, LGPL, MIT, BSD, Apache-2.0,
  CC0-1.0, CC-BY-4.0, CC-BY-SA-4.0), OFL-1.1 para fontes e
  AGPL-3.0-or-later (somente dados de áudio, ADR 0015; nunca código ou
  dependências). Esses sons são **embutidos** no binário (combinação GPLv3 +
  AGPLv3, permitida pelo §13 de ambas): os `CREDITS.md` acompanham o
  instalador, e a aceitação depende da licença do projeto (ADR 0004).
- **Não copie** para o repositório bancos, livros `.bin`, `spelling.ssp` ou
  fotos que vêm com o Scid vs PC: o `COPYING` exige autorização dos autores.

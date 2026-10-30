---
paths:
  - "reference/**"
---

# Código de referência do Scid vs PC (somente leitura)

`reference/scid_vs_pc-4.27/` é o código-fonte original, **apenas para
consulta**. Ele não é versionado nem compilado pelo workspace.

- **Nunca edite, mova ou apague** nada em `reference/`. Há bloqueio por
  permissão e por hook, e isso vale também para comandos de shell.
- O Scid é a **especificação de comportamento**, não um modelo de código.
  Entenda o que ele faz (regras, casos de borda, mensagens, textos de ajuda) e
  implemente em Rust idiomático, dentro da arquitetura do MyChess. Não traduza
  `tkscid.cpp` ou Tcl linha a linha.
- **Exceção: formatos de arquivo** (SI4, `.ssp`, `.eco`, `.stc`). Aqui a
  fidelidade é byte a byte. Documente a especificação em `docs/formatos/`,
  citando arquivo e função de origem.
- Cite a origem nos comentários e na documentação:
  `// Scid: src/namebase.cpp, NameBase::ReadNameFile()`.
- **Código portado** (tradução direta de um algoritmo específico) leva no topo
  do arquivo Rust a origem e o aviso de copyright **copiado do arquivo do Scid**:
  ```rust
  // Portions of this file are derived from Scid vs PC 4.27, src/<arquivo>:
  // <linha de copyright exatamente como no original>. Licensed under the GNU GPL.
  ```
- Dados que vêm com o Scid vs PC (`bases/`, `books/`, `spelling.ssp`, fotos)
  podem ser lidos por testes locais `compat_*`, mas **nunca** copiados para o
  repositório.

Mapa do código: `docs/referencia-scid.md`. Para pesquisar uma funcionalidade,
use a skill `/consultar-scid`.

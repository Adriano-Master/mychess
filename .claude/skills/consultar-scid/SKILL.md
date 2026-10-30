---
name: consultar-scid
description: Pesquisa no código de referência do Scid vs PC 4.27 (somente leitura) como uma funcionalidade ou um formato funciona (interface em Tcl, lógica em C++, textos de ajuda) e devolve uma especificação de comportamento para o MyChess. Use antes de implementar ou testar algo que exista no Scid vs PC, ou para tirar dúvidas de compatibilidade.
argument-hint: '<funcionalidade ou pergunta, ex.: "busca por material", "formato .sn4">'
context: fork
agent: Explore
model: claude-sonnet-5-5
effort: medium
---

# Consultar o Scid vs PC: $ARGUMENTS

Pesquise no código original do Scid vs PC 4.27, em
`reference/scid_vs_pc-4.27/` (relativo à raiz do projeto), como funciona
**$ARGUMENTS**, para especificar o comportamento do MyChess (uma reimplementação
em Rust). O diretório é somente leitura: não modifique nada.

## Onde procurar
O diretório é ignorado pelo git, então **busque sempre com caminho explícito**:
`rg -n "<padrão>" reference/scid_vs_pc-4.27/<subdiretório>`.

1. **Ajuda** (comportamento visto pelo usuário): `tcl/help/help.tcl`, com
   blocos `set helpText(<Tópico>)`. Tópicos úteis: Menus, Moves, Searches, CQL,
   Tree, TreeMasks, Maintenance, Formats, Analysis, Tourney, Book, BookTuning,
   Reports, Graphs, TB, FICS, CC, ShortCuts, Options, Cmdline.
2. **Menus** (ponto de entrada de cada funcionalidade): `tcl/menus.tcl`.
3. **Interface e fluxo**: `tcl/windows/`, `tcl/tools/`, `tcl/search/`,
   `tcl/file/`, `tcl/main.tcl`, `tcl/board.tcl`, `tcl/game.tcl`, `tcl/end.tcl`.
4. **Lógica**: comandos `sc_*` em `src/tkscid.cpp` (faixas de linhas por comando
   em `docs/referencia-scid.md`), que levam às classes em `src/*.cpp`.
5. **Formatos**: `src/index.*`, `src/namebase.*`, `src/gfile.*`,
   `src/game.cpp`, `src/tree.cpp`, `src/spellchk.cpp`, `src/pbook.cpp`,
   `src/polyglot/`. Inteiros binários são big-endian (`src/misc.h`).
6. **Textos de interface**: `tcl/lang/english.tcl` (referência) e
   `tcl/lang/portbr.tcl` (português do Brasil).

O mapa completo está em `docs/referencia-scid.md`, e as decisões que afastam o
MyChess do Scid estão em `docs/roteiro.md`, seção "Decisões pendentes" (por
exemplo, Syzygy no lugar de Nalimov).

## Regras
- Cite arquivo e linha de cada afirmação relevante.
- Quando a ajuda e o código divergirem, registre os dois. O código vence em
  questões de compatibilidade.
- Não invente. Se não encontrou algo, diga isso.
- Para formatos binários, descreva cada campo: offset, largura, ordem de
  bytes, significado e limites.

## Entrega: sua resposta final é a especificação, neste formato
A sessão principal salva o texto em `docs/specs/<slug>.md`.

```markdown
# Especificação: <funcionalidade>
- Origem no Scid: <arquivos:linhas, funções, comandos sc_*>
- Marco do roteiro: <Mx: item de docs/roteiro.md>

## Comportamento observável
<o que o usuário vê e faz, passo a passo; opções e valores padrão>

## Regras e casos de borda
1. <regra verificável, que vira critério de aceite e teste>

## Dados e formatos
<estruturas, campos e limites; para binários, tabela offset/largura/significado>

## Textos de interface
<strings relevantes de tcl/lang/english.tcl, com o nome da variável>

## Adaptações para o MyChess
<o que muda por causa da arquitetura (MVU, iced, multiplataforma) ou de decisões
registradas; o que fica de fora>

## Critérios de aceite
- [ ] <critério verificável>

## Perguntas em aberto
- <ambiguidade que o usuário ou o orquestrador precisa decidir>
```

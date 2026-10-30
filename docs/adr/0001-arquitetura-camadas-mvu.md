# ADR 0001: Workspace em camadas + MVU na interface

- **Status:** aceita (2026-10-21)

## Contexto

O dono do projeto está acostumado com MVC e pediu que esse padrão fosse usado,
desde que fizesse sentido para um projeto Rust, multiplataforma e com interface
gráfica. O Scid vs PC mistura lógica e interface: boa parte das regras está em
~230 mil linhas de Tcl, com o C++ exposto por comandos `sc_*`. Precisamos de
fronteiras que (a) permitam testar a lógica sem janela, (b) isolem o toolkit
gráfico e (c) aguentem anos de crescimento.

## Decisão

1. **Workspace Cargo em camadas**: domínio (`core`, `pgn`, `db`, `engine`,
   ...), aplicação (`app`) e apresentação (`gui`). As dependências só apontam
   para baixo, e `cargo xtask arch` verifica isso.
2. **MVU (Model–View–Update) na interface**, o padrão nativo do iced:
   `update(&mut estado, Message) -> Vec<Effect>` é o *controller* puro, e
   `view(&estado)` é a *view*. Efeitos são dados executados pela casca (GUI).
3. O `update` e o estado da aplicação ficam em `mychess-app`, **sem dependência
   de toolkit gráfico**.

## Alternativas consideradas

- **MVC clássico com observers**: em Rust exige estado mutável compartilhado
  (`Rc<RefCell<…>>`), gera ciclos de referência e erros de empréstimo em tempo
  de execução, e não combina com nenhum dos toolkits Rust maduros.
- **MVVM com bindings** (modelo do Slint): bom, mas amarra o *view-model* ao
  toolkit escolhido.
- **Crate único**: compila mais devagar, não impõe fronteiras e mistura GUI com
  domínio.

## Consequências

- A lógica de aplicação é testável com testes comuns (`cargo test`), sem janela.
- Trocar ou adicionar outra interface (TUI, web) exige reescrever só a casca.
- Custo: toda interação passa por `Message` e todo efeito por `Effect`, então
  há mais tipos explícitos. Esse custo é aceito em troca da previsibilidade.
- O mapeamento MVC ↔ MVU está documentado em `docs/arquitetura.md` (seção 2.1).

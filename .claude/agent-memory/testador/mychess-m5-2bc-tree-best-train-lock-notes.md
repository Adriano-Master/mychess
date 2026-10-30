---
name: mychess-m5-2bc-tree-best-train-lock-notes
description: Armadilhas dos testes m5_2bc (Best Games, treino, Lock, abrir base como arvore) no app e na GUI
metadata:
  type: project
---

- `TreeUi::seed_training` e inalcancavel de testes de integracao (`AppState.tree` e `pub(crate)`, `tree()` e `&`): treino so se testa com posicao de resposta forcada (jogos iguais) ou conjunto de respostas possiveis (60 sorteios, p de falha 2^-59).
- Apos `BasesMessage::Select(b)` o editor carrega o jogo de `b`; jogar outro lance na raiz abre prompt de variante e NAO joga. Use `Nav::Forward/GameEnd` ou clique em lance que ja e filho.
- Jogo editado + `LoadGame`/Best `Load` pergunta antes (`Message::AnswerDiscard(DontSave)`).
- `open_pgn` de arquivo vazio nao abre slot (erro): para "base sem partidas" use `BasesMessage::Select(SlotId::CLIPBASE)`.
- GUI: `pick_list` do bloco Best abre para CIMA no pane de 900 px; varrer `dy` de -260 a 260 e achar o x da lista por sondagem (labels "Sort"/"Games" colidem com a barra e o cabecalho da tabela, `find` so acha o primeiro). Custa ~20 s nos 4 testes de listas.
- GUI: o pedido de dialogo de "Abrir base como arvore" se obtem chamando `mychess_app::update(&mut rig.rig.shell.app, ...)` e respondendo com `Message::FilePicked(request, Some(path))` via `rig.msg`; `rfd` nunca roda.
- Achado: `BestUi::set_rows` mantem a selecao por numero mesmo quando a lista passa a ser de outra base (best.rs).

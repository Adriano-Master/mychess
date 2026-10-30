---
name: mask-app-lessons
description: M5-4b (mascaras de arvore no app) - armadilhas verificadas de teste e desenho
metadata:
  type: project
---

- `Rig` de `tests/m5_2_common` serve a testes de mascara via `#[path = "../m5_2_common/mod.rs"]`; `Rig::send` devolve so os efeitos de host (PickFile, Exit); efeito `Effect::Mask(Fill)` sem rodar = `rig.raw(...)` + `rig.perform(...)` depois.
- Teste que "falha ao gravar" no tempdir: o caminho `/tmp/.tmpXXXX` nao tem extensao (arquivo oculto), entao o app acrescenta `.stm` e a gravacao DA CERTO; para forcar erro crie um diretorio chamado `x.stm` e salve nele.
- Trocar a partida (`GoTo`/`Replacement::FromPosition`) com a partida tocada no tabuleiro pergunta "salvar?" (`discard_prompt`); o teste precisa responder `AnswerDiscard(DontSave)`.
- `Arc::try_unwrap` no job de preenchimento sempre falha (o estado guarda outra `Arc`): a copia profunda acontece uma vez na thread do job, de proposito.
- Preferencias da mascara ficam em `[tree_mask]` (nao `[tree.mask]`) porque `TreePrefs` e `Copy` e usada com `..self.prefs.tree`.
- Adicionar campo a `TreeView`/`TreeRowView` quebra os dois literais em `mychess-gui/src/windows/tree/tests.rs`.

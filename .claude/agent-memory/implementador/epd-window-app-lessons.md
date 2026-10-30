---
name: epd-window-app-lessons
description: M7-5b (janela EPD, camada de app) - armadilhas verificadas de EpdBook, fixtures de teste in-crate com engine falsa, sync do tabuleiro
metadata:
  type: project
---

- `EpdOperation::sort_cmp` e `strip_opcode` do `mychess-pgn`: `sort_cmp` e `pub(crate)` (ordenar um registro = livro de 1 registro + `sort_operations`); `strip_opcode`/`remove_opcode` diferenciam caixa (a regra 11 da spec pede o contrario: laco proprio no lote). `EpdRecord::id()` devolve o operando SEM aspas.
- Estado do livro como `Arc<EpdBook>` + `Arc::make_mut`: so e barato se nenhum trabalho segura o Arc; por isso `EpdFile::locked()` recusa edicoes enquanto ha job/linhas em voo.
- Testes in-crate de engine: os helpers de `state/annotate_tests.rs` (`setup`, `answer`, `commands`, `last_search`, `send`) sao `pub(super)` e servem a outro `state/*_tests.rs`. A engine da fixture esta `analyzing`, entao TODO update devolve uma busca do tabuleiro: filtre pela etiqueta (`is_epd_tag`) em vez de exigir `effects.is_empty()`.
- Fixture que injeta `EpdFile` direto precisa chamar `refresh_epd` uma vez (o que a primeira mensagem faz): senao o primeiro `update` sincroniza o tabuleiro e comita/atropela rascunho e selecao.
- `Message::PreferencesLoaded` e `clippy::large_enum_variant` (limite 200 bytes de diferenca para `Online`, 304): cada campo novo de `Preferences` vai em `Box`; o outro agente (M7-6b) tambem mexe em `prefs.rs`/`change.rs`/`read.rs` ao mesmo tempo - reler antes de cada edicao.
- `cargo test -p mychess-app --tests` compila ~245 binarios; com `CARGO_PROFILE_DEV_DEBUG=0` e `target/m7e` ficou pequeno (<1 GB), mas demora; rodar em background com log no scratchpad.

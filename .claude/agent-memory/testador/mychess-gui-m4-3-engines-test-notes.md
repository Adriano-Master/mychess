---
name: mychess-gui-m4-3-engines-test-notes
description: Armadilhas dos testes GUI do M4-3 (dialogo Engines, editor, opcoes UCI) com iced_test
metadata:
  type: project
---

- Para achar/clicar texto do dialogo use `engines_dialog::view`/`engine_editor::view`/`engine_options::view`/`delete_view` direto no `Simulator` (a `Rig::sim` inclui a barra de menus: "Edit" e "File" colidem com os botoes). Tecla/estado: pela `Rig` (`rig.press`, `rig.key(.., text_focused, ..)`).
- `radio` nao expoe texto ao seletor: clique pelos ids dos containers (`engine-protocol-xboard`, `engine-key-f2..f4`, `engine-key-none`). `radio` ja marcado nao publica de novo.
- `rig.app_msg`/`rig.msg` descartam os efeitos (nada e executado); `rig.send` executa `perform` (FS real, spawn real). `mychess_app::update` direto devolve os efeitos para inspecao (pegue o `request` de `Effect::TestEngine` etc. e responda com `EngineMessage::Tested`).
- `Stop` de execucao Starting/Running vira `Stopping` (a execucao fica na lista ate `Stopped`); botao fica "Stop" desabilitado.
- Fluent do app usa `set_use_isolating(false)`: nao ha U+2068/2069 nos textos; `has()` do I18n cai no en-US, entao paridade en x pt exige comparar ids dos `.ftl`.
- `ExitInfo`/`ExitReason` nao sao reexportados por `mychess_app::engines` e o GUI nao tem `mychess-engine` como dev-dep: nao da para fabricar `EngineEvent::Exited` nos testes da GUI.
- Outro agente pode criar `crates/target/...` (CARGO_TARGET_DIR relativo a `crates/`), o que quebra o workspace (`crates/*`): esperar sumir.

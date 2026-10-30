---
name: engine-app-flow-lessons
description: Armadilhas do M4-3 fase 2 em mychess-app (EngineHub, clippy do crate, testes de startup, CARGO_TARGET_DIR entre chamadas)
metadata:
  type: project
---

- `export CARGO_TARGET_DIR` não persiste entre chamadas de Bash: repetir em cada comando (senão compila em `target/`).
- `Message::PreferencesLoaded(LoadedPreferences)` está no limite do `large_enum_variant` (200 bytes de folga): campo novo em `Preferences` deve ir em `Box` (como `game_list`, `analysis`).
- `startup` agora emite `LoadBookmarks`, `LoadEngines`, `LoadPreferences` nessa ordem; testes devem procurar o efeito, não assumir o primeiro.
- `perform(StartEngine)` usa `EngineHub::launcher()`: nos testes substitua por `ScriptedEngine::start` (dev-dependency `mychess-engine` com `testing`); `mychess-fake-engine` só existe em `target/<perfil>/` se alguém o construiu (`cargo build -p mychess-engine --features testing --bin mychess-fake-engine`).
- `Command::NewGame` com a partida alterada pergunta antes (`AnswerDiscard`); testes de `NewGame` para a engine precisam responder.
- `Command+Shift+A` já é `ListSelectAll`; o diálogo de engines usa `Command+Shift+E`.
- M4-5: um `Box` de 8 bytes novo em `Preferences` ainda estourou o `large_enum_variant` (Message::PreferencesLoaded): a folga é ~0. Solução usada: `Box<FilesPrefs>` (liberou ~90 bytes). `0.2_f32` como `f64` vira 0.20000000298: compare limiares do semáforo em `f32`.

---
name: mychess-gui-m3-6-bookmarks-test-notes
description: Armadilhas dos testes GUI do M3-6 (marcadores, recentes, somente leitura): notice, SI4 na GUI, atalhos
metadata:
  type: project
---

- O aviso (`app.notice`) so vai para `ui.status_notice` quando o shell processa uma mensagem App/Dialog; `rig.sync()` (mensagem de UI) nao move. Use o helper `notice(&rig)` ou mande uma `app_msg` inocua antes de procurar o texto na barra.
- O texto renderizado com argumento Fluent traz marcas de isolamento (U+2068/2069); para achar no `sim.find` use o texto cru de `app_notice_text`, para comparar conteudo remova as marcas.
- Para SI4 real na GUI foi preciso `mychess-db` como dev-dependency do `mychess-gui` (o `xtask arch` nao restringe dev-deps). Depois do `open`, `current_game` e `None`: mande `BasesMessage::LoadGame`.
- `Rig::new()` usa `Shell::new(None)` (nada gravado); para `bookmarks.toml` troque `rig.shell` por `Shell::new(Some(prefs))`, mande `PreferencesLoaded` e `BookmarksMessage::Loaded`. `Rig::send` roda os efeitos; `rig.msg`/teclas descartam a Task (nada gravado).
- Chaves `action-*` do mapa de atalhos nao existem nos .ftl (nao usadas).

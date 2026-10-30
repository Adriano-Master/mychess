---
name: mychess-app-m4-5-evalbar-test-notes
description: Notas dos testes m4_5 (barra de avaliacao, setas, semaforo no app) — achados e armadilhas verificados
metadata:
  type: project
---

- `Action::all()` NAO lista as acoes de BASE_NAMES (M3-6 em diante, inclusive ToggleEvalBar/TrafficLights/ArrowNumbers); usar `Shortcuts::defaults().iter()` para varrer tudo. `StartEngine(n)` etc. repetem `message_key`.
- Mudanca de preferencia nao-debounced pede salvar (Effect::SavePreferences) so se houver `prefs_path`; sem Startup nao ha como observar `prefs_dirty`.
- `report()` do common precisa de PV legal na posicao do cursor; para posicoes variaveis use `legal_moves().first()`.
- Achados: `fingerprint` do cache ignora cauda da PV (modo Sequencia nao redesenha); `bar_fill(inf)` satura 0/1 em vez de 0,5 (regra 1 da spec); Sequencia nao valida lances apos o 1o.
- `engines::store::tests::list_round_trips` falha isolada (FieldReset last_used com nome "\0"), independente do M4-5.

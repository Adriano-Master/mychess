---
name: mychess-app-m4-4-analysis-test-notes
description: Como montar testes de integração da janela de análise (M4-4) sem processo e armadilhas verificadas
metadata:
  type: project
---

- Run pronta sem engine: `Start(id, Analyze)` -> pegar `Effect::StartEngine{run}` -> `EngineMessage::Started{identity}`; eventos sintéticos via `EngineMessage::Event`. Helper em `tests/m4_4_common/mod.rs`.
- Linhas só "seguem o cursor" depois de `SearchStarted` com o cursor já no nó (o alvo vem de `run.sent`); depois de `replace_game` navegar antes de emitir `started`.
- `Position::parse_uci` de outro nó dá `Move` ilegal no nó testado: serve para PV truncada (cuidado: d2d4 é legal em e5, brancas jogam).
- FENs de roque: peão branco em b7 ataca c8; torre em f1 ataca f8 (roque preto ilegal).
- Rule 13 (execução Failed): AddScore/AddLine/AddLineNumber/AddAll ainda editam a partida (achado 2026-10-23).
- clippy do crate pode falhar por arquivos alheios em src/; usar `--test m4_4_x -- -A clippy::type_complexity` para isolar.

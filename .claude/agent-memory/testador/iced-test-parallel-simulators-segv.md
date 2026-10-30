---
name: iced-test-parallel-simulators-segv
description: Muitos iced_test::Simulator em threads paralelas do harness derrubam o processo de teste com SIGSEGV; serializar com um Mutex global resolve
metadata:
  type: feedback
---

Arquivos de teste do `mychess-gui` com dezenas de `Simulator::with_size`/`LayoutCache` rodando em
paralelo (threads do harness) terminaram com `signal: 11, SIGSEGV` em ~1 de cada 3 execucoes
(`tests/m2_4_game_info.rs`, `m2_4_notation_panel.rs`). Cada teste sozinho (processo isolado, 144
execucoes) e com `--test-threads=1` nunca caiu; `ICED_BACKEND=tiny-skia` nao evita.

**Why:** o sistema de fontes/render e compartilhado; o crash e em dependencia (sem backtrace).
**How to apply:** em testes novos que desenham ou fazem shaping, segurar um `static Mutex<()>`
(`render_lock()` em `tests/m2_4_common/mod.rs`) durante o teste; `Rig` e `Lay` ja o seguram.
Nao rodar `cargo test` em loop em paralelo com outro cargo: o lock do `target/` mascara a medida.

Outro risco: agentes que gravam em `crates/.claude/agent-memory/...` (cwd errado) criam um membro
de workspace invalido (`crates/*` casa diretorios ocultos) e quebram todo `cargo`.

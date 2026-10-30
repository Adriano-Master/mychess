---
name: mychess-app-m6-4-extra-test-notes
description: Notas dos testes m6_4_extra_* (tabela/graficos/rastreador no app): armadilhas do rig, achados do Scid sobre anos e rating
metadata:
  type: project
---

- `Rig::raw` devolve os efeitos do update; para contar computos de uma rajada de debounces, pegue `rig.delays` depois de `send` e alimente cada `Rig::elapsed` com `raw` (so o ultimo gera Compute).
- Tabela em base fixada (`OpenGame`) segue a base fixada mesmo com outra base atual; Clipbase serve para provocar `Stale` (fixar clipbase, copiar outro jogo para ela). Copiar o mesmo jogo duas vezes nao adiciona.
- Rating R2: o Elo minimo filtra ANTES de escolher o "ultimo do mes" (mes de janeiro fica com um jogo anterior de Elo maior).
- Scid `start.tcl` 668-674: anos novos (sem entrada salva) ficam marcados; o app guarda um snapshot em `years_checked`, entao anos novos ficam desmarcados depois do primeiro clique do usuario (achado aberto).
- `cargo clippy -D warnings` do crate pode falhar por codigo de producao em andamento de outro agente; filtrar a saida por `m6_4_extra`.

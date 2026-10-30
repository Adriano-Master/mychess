---
name: pgn-writer-oracle-nags
description: Ao usar mychess_pgn::write como oraculo da notacao, with_comments(false) ainda escreve NAGs; o Scid/spec M2-4 os oculta
metadata:
  type: project
---

`WriteOptions::with_comments(false)` mantem os NAGs; `show_comments=false` da notacao (M2-4 regra 5, Scid `-comments 0`) oculta comentarios **e** NAGs. No oraculo proptest use `with_nags(NagOutput::None)` quando comentarios estiverem desligados.

**Why:** a primeira versao do proptest falhou em `1.a3 *` vs `1.a3 $11 *` e nao era bug de producao.
**How to apply:** ao comparar `NotationDoc::flat_text()` com o escritor, alinhe as opcoes dos dois lados. Tambem: o escritor e o Scid (printDiagrams desligado) nao numeram o lance seguinte ao NAG `D`, ao contrario da regra 2 da spec.

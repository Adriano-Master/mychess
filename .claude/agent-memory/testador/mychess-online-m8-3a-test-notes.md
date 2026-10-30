---
name: mychess-online-m8-3a-test-notes
description: Notas dos testes m8_3 (mychess-online, cliente Lichess) - mutacao com target compartilhado contamina, fake sem Content-Length, sem rstest no crate
metadata:
  type: project
---

- Mutacao em copia do repo com o MESMO `CARGO_TARGET_DIR` do original: o cargo considera a lib do original "fresca" com o artefato mutado (hash por caminho relativo ao workspace). Um teste novo falhou "sem motivo" no original por isso. Rode `cargo clean -p mychess-online` antes da verificacao final, ou use target separado.
- `cargo test` para no primeiro binario com falha: nos mutantes use `--no-fail-fast`, senao um teste vermelho pre-existente "mata" todos os mutantes.
- `mychess-online` nao tem `rstest` como dev-dependency (nao mexa no Cargo.toml): tabelas viram `Vec<(..)>` + `for`. `Result<A, B>` dentro de tupla quebra scripts de conversao.
- `FakeLichess`: `Response::json` sempre manda Content-Length; para testar corpo limitado sem Content-Length use `Response::stream(vec![Step::Raw(..)])` num GET nao-stream (chunked).
- Clippy pedantic em tests/: `assert_is_empty`, `type_complexity` (Vec<(u16, fn(&Error)->bool)>), `let _ = <tipo com Drop>` (use `drop(..)`), `cast_precision_loss`; rustfmt transforma closure de uma linha em bloco e exige `;` final.
- Contagem de tentativas do stream: o reset por dados zera ANTES de contar a queda, entao apos uma conexao com dados a queda e `attempt 1` e a falha seguinte `attempt 2`.
- Defeito achado (M8-3a): `LineFramer` conta o `\r` do `\r\n` no limite de 1 MiB (linha de exatamente 1 MiB com CRLF da `LineTooLong`); spec diz "acima de 1 MiB".

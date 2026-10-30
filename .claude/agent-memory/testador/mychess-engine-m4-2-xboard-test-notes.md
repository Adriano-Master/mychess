---
name: mychess-engine-m4-2-xboard-test-notes
description: Armadilhas dos testes XBoard (m4_2) no mychess-engine - janela de saida antiga, fake com info antes do pong, lints de assert
metadata:
  type: project
---

- Sem `ping=1` na engine, linhas de pensamento com ply > 2 sao descartadas por ~1 s depois de iniciar a busca (janela D419). Com `ping=1`, tudo antes do `pong N` e descartado. Em testes com `ManualEngine`, use `ping=1` e responda o pong, ou mande primeiro uma linha com ply 1.
- Com a engine falsa (`ScriptedEngine`), o primeiro `Info` pode ser o depth 1 ou 2: o fake as vezes escreve a linha antes de ler o `ping`, e o adaptador a descarta corretamente. Compare em profundidade fixa, nunca "o primeiro Info".
- O crate nao tem `rstest` nos dev-deps (e testes nao podem editar Cargo.toml): tabelas viram lacos com mensagem por caso.
- Clippy do workspace nega `assert!(x.is_empty())` (assert_is_empty) e `unwrap` em helpers fora de `#[test]`; helper `quiet()` em `tests/m4_2_common/xb.rs`.
- Exportar `CARGO_TARGET_DIR` no mesmo comando bash a cada chamada (o ambiente nao persiste).

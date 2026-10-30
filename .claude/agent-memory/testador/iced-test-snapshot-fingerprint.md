---
name: iced-test-snapshot-fingerprint
description: Como testar pixels do tabuleiro (mychess-gui) sem imagens salvas nem leitura de pixels, usando iced_test 0.14
metadata:
  type: reference
---

`iced_test::Snapshot` (0.14) nao expoe os pixels. Tecnica que funcionou em
`crates/mychess-gui/tests/m2_3_board_widget.rs`: chamar `snapshot.matches_hash(dir/scene)`
num diretorio unico em `std::env::temp_dir()` (cria `scene-<renderer>.sha256`), ler o arquivo
e comparar as impressoes de duas cenas **na mesma execucao** (iguais quando a spec diz
"equivalente", diferentes quando diz "visivel"). Serve para: show_marks=false == sem marcas,
cor desconhecida == firebrick, Tux nao desenhado, animacao em progresso 1.0 == posicao nova,
Escape volta ao desenho ocioso, 10 temas distintos.

- Passa tambem com `ICED_BACKEND=tiny-skia` (renderizador por software).
- Cada `Simulator::with_size` custa ~0,3 s (sys alto): evite matrizes grandes de snapshots.
- `Simulator::simulate` reaproveita a mesma UserInterface entre chamadas (estado do canvas
  persiste); `point_at` so muda o cursor, nao gera CursorMoved; modificadores vem de
  `ModifiersChanged`, e o widget guarda o estado.
- Captura ao animar: no progresso 0 so e igual a posicao antiga se nao houver captura nem
  promocao (regra 55: a vitima some e a peca promovida desliza desde o primeiro quadro).
- clippy do workspace e rigido em testes: `assert_is_empty`, `float_cmp`,
  `unnecessary_wraps` precisam de `#![allow]` no topo do arquivo de teste.

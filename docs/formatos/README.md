# Especificações de formatos de arquivo

Um documento por formato (`si4.md`, `ssp.md`, `stc.md`, `polyglot.md`...),
escrito antes do código do codec. Para cada campo: offset, largura, ordem de
bytes, significado, limites e a referência ao código do Scid vs PC (arquivo e
função).

Regras completas em `.claude/rules/formatos-binarios.md`. O resumo inicial do
SI4 está em [`../referencia-scid.md`](../referencia-scid.md), seção "Formatos
de arquivo".

| Formato | Documento | Situação |
|---|---|---|
| `.eco` (classificação de aberturas; `scid.eco`, `scid_fr.eco`) | [`eco.md`](eco.md) | implementado (M1) |
| EPD (posições com operações; dialeto do Scid) | [`epd.md`](epd.md) | implementado (M1) |
| PGN | [`../specs/m1-3-pgn.md`](../specs/m1-3-pgn.md) (leitura e escrita) | implementado (M1) |
| SI4 (`.si4`, `.sn4`, `.sg4`) | [`si4.md`](si4.md) | leitura (M3-1) e escrita (M3-2a) implementadas; compactação e `.si3` pendentes (M3-2b) |
| Polyglot (`.bin`, livros de aberturas) | [`polyglot.md`](polyglot.md) | núcleo implementado (M5-5) |
| `.stc` (cache da árvore) | [`stc.md`](stc.md) | implementado (M5-3a) |
| `.stm` (máscara de árvore) | [`stm.md`](stm.md) | implementado (M5-4a) |
| padrão tático (`.toml`, `mychess-pattern`; formato próprio) | [`padrao-mychess.md`](padrao-mychess.md) | implementado (M6-2a/b) |
| `.ssp` (spelling) | [`spelling-ssp.md`](spelling-ssp.md) | núcleo implementado (M6-3a) |
| `.spf` (fotos de jogadores) | [`spf-fotos.md`](spf-fotos.md) | leitor implementado (M6-3a) |

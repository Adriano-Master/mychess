# ADR 0016: Exportação: visualizador HTML próprio, SVG nativo e PNG por `resvg`

- **Status:** aceita pelo orquestrador (2026-10-29), a pedido da entrega M7-4a. Resta a confirmação do dono do projeto. Especificação: `docs/specs/m7-4-exportacao.md` (D3700-D3799). Sem esta ADR a fatia de imagem entrega só SVG; o resto da M7-4 não depende dela.

## Contexto

A M7-4 exporta partidas em HTML (com um visualizador interativo), LaTeX e EPD, a lista de partidas em texto e a **imagem do tabuleiro** em PNG e SVG. Três pontos exigem decisão estrutural:

1. **Visualizador HTML.** O Scid vs PC exporta cada partida como um `.html` que carrega `scid.js`, `scid.css` e GIFs de peças (`html/` e `html/bitmaps/` na árvore de referência) e **copia esses ativos, sobrescrevendo-os, para a pasta do usuário**. Esses ativos fazem parte do Scid vs PC (`COPYING` exige autorização para o que vem com ele, e o repositório não pode conter dados do Scid) e o formato (um arquivo por partida) é pouco prático.
2. **Imagem do tabuleiro.** Não existe no Scid. Precisa de um SVG montado a partir do tema e das peças (`assets/pieces/*.svg`) e, para PNG, de um rasterizador de SVG.
3. **Peças dentro dos arquivos exportados.** Os conjuntos de peças têm licenças diferentes (`assets/LICENSES.md`: GPL-2.0-or-later, Apache-2.0, MIT, CC0...); embuti-las num HTML ou SVG distribuído pelo usuário exige o aviso de autoria.

O `iced` 0.14 (com a feature `svg`) já traz `resvg 0.45.1`, `usvg`, `tiny-skia 0.11.4` e `fontdb 0.23.0` no `Cargo.lock`, e `deny.toml` já aceita `MPL-2.0`.

## Decisão

1. **Visualizador próprio, em arquivo autônomo por volume.** O MyChess escreve o seu `viewer.js` e `viewer.css` (código novo, GPL-3.0-or-later, em `assets/export/`, embutido no binário). Cada volume (`X.html`, `X_2.html`...) é **um único arquivo** com CSS, JavaScript, peças em SVG e os dados das partidas em `<script type="application/json">`. Nenhum ativo do Scid é lido, copiado ou reescrito; nenhum arquivo fora do próprio volume e do `X.pgn` opcional é criado ou sobrescrito. O JavaScript não usa `eval`, não monta HTML com texto do banco e não faz requisições externas.
2. **SVG nativo, em `mychess-app`.** A posição é montada como texto SVG (casas, coordenadas, `<symbol>`/`<use>` das peças, setas, realces). Os SVG das peças e a fonte vêm do **host** (a GUI, que já embute os ativos); o app só os recebe como dados. Os `id` internos de cada peça são prefixados (`wK-…`) para evitar colisões entre gradientes e máscaras.
3. **PNG por `resvg`.** Dependência nova declarada em `[workspace.dependencies]` com `default-features = false` e a feature `text` (a codificação PNG vem do `tiny-skia` que o `resvg` reexporta), só usada pelo `mychess-app`. Fonte embutida (Noto Sans, `assets/fonts/`), **sem carregar fontes do sistema**. O resultado é determinístico entre plataformas, salvo diferenças de arredondamento da rasterização.
4. **Atribuição das peças.** Todo HTML e SVG que embute um conjunto de peças leva um comentário `<!-- Pieces: <nome> by <autor>, <licença> -->` com os dados de `assets/LICENSES.md`.
5. **LaTeX sem executar programas.** O MyChess gera o `.tex` (estrutura do Scid: `xskak`, `longfbox`, `tabularx`), mas **não** compila nem abre o PDF; a pré-visualização com `pdflatex` fica para uma decisão posterior sobre programas externos.

## Licenças

- `resvg` (MPL-2.0), `usvg` (MPL-2.0), `tiny-skia` (BSD-3-Clause), `fontdb` (MIT), `png` (MIT OR Apache-2.0): todas já presentes na árvore por causa do `iced` e aceitas por `deny.toml`. A MPL-2.0 é compatível com a GPL-3.0-or-later (cláusula de licença secundária). Conferir com `cargo deny check licenses` e `cargo tree -p mychess-app -e normal -i resvg` na implementação.
- Nenhuma dependência de sistema (Rust puro nas três plataformas).

## Alternativas consideradas

- **Copiar `scid.js`/`scid.css`/GIFs do Scid.** Descartado: o `COPYING` do Scid exige autorização, o formato é de um arquivo por partida e a cópia sobrescreve arquivos do usuário.
- **Gerar vários arquivos auxiliares (pasta de ativos).** Descartado: um arquivo autônomo por volume é mais fácil de mover, enviar por e-mail e abrir via `file://`.
- **Servidor local para o visualizador.** Descartado: exige rede, porta e processo.
- **PNG com `image` + `imageproc` ou desenhando pixels à mão.** Descartado: as peças são SVG; reimplementar a rasterização é mais código e mais risco.
- **PNG pelo renderizador do `iced` (captura de tela da janela).** Descartado: depende da janela, do GPU e da escala; não é determinístico nem funciona sem GUI (CLI, testes).
- **Só SVG (sem PNG).** Possível como degradação (pergunta 1 da spec); o PNG é muito pedido para publicar e para o LaTeX.
- **Módulo `image` do `iced`.** Não rasteriza SVG para arquivo; serve só para exibir.

## Consequências

- Uma dependência direta nova (`resvg`) no `mychess-app`, que já existe de forma transitiva; o tempo de compilação cresce pouco.
- O visualizador é código novo com superfície de XSS (texto de bases de terceiros): exige revisão de segurança dedicada e testes de escape (spec M7-4, regras 7 e 12).
- Os relatórios (M7-2/M7-3) e a tabela de torneio (M6-4) passam a poder reutilizar o escritor de SVG para exportar gráficos como imagem (D3377 da M6-4), numa fatia futura.
- Se o dono rejeitar a ADR: PNG sai da M7-4; SVG e o resto permanecem.

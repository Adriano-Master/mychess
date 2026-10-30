# ADR 0018: Distribuição, pasta `MyChess` do usuário e primeira execução

- **Status:** aceita pelo dono do projeto (2026-10-29), com as respostas às
  perguntas de escopo registradas abaixo. Especificação:
  `docs/specs/m9-4-distribuicao-e-primeira-execucao.md` (D4000-D4049).
  Revisa o item "Associação de `.si4`/`.pgn`, instaladores, assinatura" do M9.

## Contexto

O código vai para o GitHub e o dono quer que usuários leigos **baixem um
executável e já usem**: com engine (Stockfish), um livro de aberturas e uma base
com algumas partidas, sem configurar nada. Ao instalar, deve existir a pasta
`MyChess` na pasta pessoal do usuário, com `Engine`, `Databases` e `Books`.

Restrições do projeto: regras de ouro 5, 7 e 8 (dependências, multiplataforma,
git), `multiplataforma.md` (diretórios do usuário), `dependencias-e-licencas.md`
(assets) e o `COPYING` do Scid (bancos, livros e `spelling.ssp` do Scid vs PC
**não** podem ser redistribuídos).

## Decisão

1. **Pasta visível do usuário.** `MyChess/` na **pasta pessoal** (`UserDirs`,
   `home_dir()`: `~/MyChess`, `C:\Users\<nome>\MyChess`), com exatamente três
   subpastas: `Engine/`, `Databases/`, `Books/` (nomes fixos, em inglês, não
   traduzidos). Preferências e estado do app continuam na pasta de configuração
   do sistema (`ProjectDirs`); `MyChess/` guarda só o que o usuário vê e
   manipula. Isto estende a regra de `multiplataforma.md` ("diretórios do
   usuário via `ProjectDirs`"): `UserDirs` é permitido para **esta** pasta.
2. **Criada na primeira execução do app, não pelo instalador.** O instalador
   (que no Windows pode rodar como administrador) só coloca o app e os
   **recursos iniciais** (*seeds*) na pasta de recursos do pacote. Na primeira
   execução o MyChess cria `MyChess/` e **copia** os *seeds* que faltam.
   Um registro em `ProjectDirs` (`seed.toml`: item, versão, SHA-256, instalado
   em) garante que **nada que o usuário alterou, renomeou ou apagou é
   sobrescrito ou recriado**; um comando "Restaurar exemplos" (menu Ajuda) refaz
   a cópia sob pedido. Desinstalar o app **preserva** `MyChess/`.
3. **Conteúdo inicial.**
   - **Stockfish** (único engine embarcado; o usuário adiciona outros pela
     janela de engines): binário **oficial**, licença GPLv3, baixado no CI com
     versão fixada e SHA-256 conferido (nada de binário no git). No x86-64 há
     duas variantes (`avx2` e `sse41-popcnt`), escolhidas na primeira execução
     por detecção de CPU; no Linux e no Windows. A licença e o link do
     código-fonte do Stockfish acompanham o binário em `Engine/stockfish/`.
     Registrado automaticamente na lista de engines.
   - **Livro de aberturas**: um Polyglot `.bin` **gerado por nós** (criador de
     livros do M5) a partir de partidas CC0.
   - **Base de partidas**: um subconjunto de partidas CC0 (base do Lichess de
     jogadores fortes) importado para SI4 pelo `mychess-cli` no CI, com créditos.
   - Nada do Scid vs PC (bancos, livros, `spelling.ssp`) entra. As fontes
     (URL, data, SHA-256, licença) ficam em `distribution/seeds.toml` e em
     `assets/LICENSES.md`.
4. **Modo portátil.** Um arquivo `portable.flag` ao lado do executável faz o app
   usar `<pasta do executável>/MyChess/` (dados) e `<...>/MyChess/.config/`
   (preferências), sem tocar na pasta pessoal. Se o local não for gravável, o
   app avisa e volta ao modo normal. Distribuído no `.zip` do Windows.
5. **Plataformas e pacotes (fase 1).**
   - **Windows:** instalador (NSIS por `cargo-packager`) e `.zip` portátil.
   - **Linux:** AppImage e `.deb`.
   - **macOS: sem binários nesta fase** (decisão do dono: a Apple fica de fora;
     só o código-fonte, com instruções de compilação no `README.md`). O código
     continua multiplataforma e o CI continua testando no macOS.
   - **Sem assinatura de código na fase 1.** O `README.md` explica o aviso do
     SmartScreen e publica as somas SHA-256 de cada arquivo. Assinatura (por
     exemplo SignPath, gratuito para código aberto) fica para depois.
6. **Publicação.** Um workflow do GitHub Actions, disparado por tag `v*`, monta os
   *seeds*, empacota nas plataformas da fase 1, calcula as somas e publica no
   GitHub Releases (rascunho até o dono revisar).
7. **Licença.** O dono aceita a [ADR 0004](0004-licenca.md) (GPL-3.0-or-later;
   o `COPYING` do Scid não indica versão da GPL, o que permite escolher qualquer
   uma pelo §9 da GPLv2) e mantém no projeto o leitor de PGN com o cabeçalho de
   origem e os sons do Lichess (ADR 0015), atendendo qualquer pedido de remoção
   de um titular.

## Consequências

- Novo módulo `platform` de primeira execução em `mychess-app` (`seed`), puro e
  testável com diretórios temporários; a GUI só mostra o resultado.
- `cargo xtask dist` monta os *seeds* e os pacotes localmente; o CI o reutiliza.
- Quem compila do código-fonte não recebe *seeds*: o app funciona vazio e a
  janela de engines/livros explica como obter. (Pode haver um `cargo xtask
  seed` que baixa e prepara os mesmos itens.)
- Uma atualização do app pode trazer *seeds* novos (Stockfish mais novo): entram
  ao lado dos antigos (`stockfish-<versão>`), sem sobrescrever; o engine padrão
  só muda se o usuário não o alterou.
- Antivírus podem sinalizar executáveis sem assinatura; documentado no `README.md`.

## Alternativas consideradas

- **Criar a pasta no instalador:** não sabe a pasta do usuário certo (instalação
  com privilégios), não existe nos formatos portáteis (AppImage, `.zip`).
- **`Documentos/MyChess`:** é a convenção do Windows/macOS, mas o dono prefere a
  pasta pessoal; a localização fica numa função única, fácil de mudar.
- **Embarcar livros e bases do Scid, TWIC ou PGN Mentor:** direitos incertos.
- **Vários engines (Lc0, Maia):** redes de centenas de MB; o usuário adiciona.
- **Binários no repositório:** incha o git; baixar no CI com SHA-256 fixado.

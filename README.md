# MyChess

Reimplementação moderna, em Rust, do [Scid vs PC](https://scidvspc.sourceforge.net/)
4.27: banco de dados, analisador e ferramenta de estudo de partidas de xadrez,
para **Linux, Windows e macOS**, com interface gráfica em
[iced](https://iced.rs).

> **Aviso:** este projeto foi desenvolvido com forte assistência de IA. Leia
> [Como este projeto foi feito](#como-este-projeto-foi-feito) antes de confiar
> nele para qualquer coisa importante.

## Estado do projeto

O projeto está em desenvolvimento ativo; há pacotes **alfa** para download (veja a seção seguinte).
O núcleo de xadrez, o formato de banco SI4, a árvore de aberturas, a análise
com engines, as buscas e boa parte da interface existem e têm testes
automatizados. O plano e o que falta estão em [`ROADMAP.md`](ROADMAP.md).

Seja cético com o que não foi exercitado por uma pessoa. Muitas partes só foram
verificadas por testes automatizados (com engines e servidores falsos, dados
sintéticos e tabelas feitas à mão) e **dependem de teste manual do dono, que
ainda não foi feito**:

- jogar no Lichess e no FICS (nunca testados contra os servidores reais);
- engines reais (Stockfish e outras) em análise, jogo, torneios e treinos;
- tabuleiros eletrônicos (Novag Citrine e DGT): sem hardware para testar;
- saída de som em placa de som real;
- tablebases Syzygy reais (só as de 3 peças foram conferidas à mão);
- aparência e usabilidade dos painéis e das janelas;
- bancos muito grandes e arquivos reais do Scid (`spelling.ssp`, máscaras
  `.stm`).

Nada disso deve ser tomado como validado. Também há funcionalidades do Scid
ainda não feitas (mais idiomas, tradução do manual para o inglês, instalador
e assinatura de código) e três itens retirados do escopo por
decisão do dono (veja o roteiro).

## Baixar e executar (alfa)

Pacotes já compilados, versão **0.1.0-alpha**, só para x86-64 (64 bits). Basta extrair
e executar; incluem as bases de partidas descritas abaixo.

| Sistema | Pacote | Tamanho | SHA-256 | Download |
|---|---|---|---|---|
| Linux | [`MyChess-0.1.0-alpha-linux-x86_64.tar.gz`](dist/MyChess-0.1.0-alpha-linux-x86_64.tar.gz) | 26,0 MiB | `01337284b7ee35542d21efc689751e1bc1d55b06e5630ec62432a16b30bf7660` | [baixar](https://github.com/Adriano-Master/mychess/raw/main/dist/MyChess-0.1.0-alpha-linux-x86_64.tar.gz) |
| Windows | [`MyChess-0.1.0-alpha-windows-x86_64.zip`](dist/MyChess-0.1.0-alpha-windows-x86_64.zip) | 23,5 MiB | `b1afde0bdce10d15855ea7b5a9ac0964ca0fea1c9b181ccf19c9314f155b4555` | [baixar](https://github.com/Adriano-Master/mychess/raw/main/dist/MyChess-0.1.0-alpha-windows-x86_64.zip) |

Os hashes também estão em [`dist/SHA256SUMS.txt`](dist/SHA256SUMS.txt).

> **Aviso (alfa):** o programa não passou por um teste manual completo. O pacote do
> **Windows foi gerado por cross-compilação no Linux e NUNCA foi executado em um
> Windows real**; o do Linux só foi aberto no ambiente em que foi compilado (a janela
> abriu, o modo portátil funcionou e o Stockfish incluído foi reconhecido). Relatos de
> problemas, principalmente no Windows, são muito bem-vindos nas
> [issues](https://github.com/Adriano-Master/mychess/issues). Os executáveis não são
> assinados.

### Windows
1. Extraia o `.zip` inteiro numa pasta onde você possa gravar (não rode de dentro do zip).
2. Abra `mychess.exe`. Se o SmartScreen avisar (o binário não é assinado): **Mais
   informações > Executar assim mesmo**.
3. Só 64 bits. O programa desenha com a GPU pelo `wgpu` (Vulkan, DirectX 12 ou OpenGL,
   o que o driver oferecer); as bibliotecas do sistema usadas aparecem no executável
   (`d3d12.dll`, `opengl32.dll` etc.). O `iced` também traz um renderizador por
   software como alternativa, mas **essa alternativa não foi testada no Windows**.
4. Não abre janela de console; por isso `--help` e `--version` não imprimem nada no Windows.
5. O pacote do Windows **não inclui engine**; veja "Engine (Stockfish)" abaixo.

### Linux
```sh
tar xzf MyChess-0.1.0-alpha-linux-x86_64.tar.gz
cd MyChess-0.1.0-alpha-linux-x86_64
./mychess
```
- **glibc mínima: 2.39** (Ubuntu 24.04, Debian 13, Fedora 40 ou mais novos): é a exigência
  real do binário, compilado numa máquina com glibc 2.42; em sistemas mais antigos ele
  não inicia (`GLIBC_2.39 not found`). Compile do código-fonte nesse caso.
- Ligado dinamicamente a `libasound.so.2` (som), `libgcc_s`, `libm` e `libc`. Em
  tempo de execução carrega, se existirem, as bibliotecas gráficas: Vulkan ou
  OpenGL/EGL (drivers Mesa), X11 e/ou Wayland, `libxkbcommon` e `libdbus-1`.
  Pacotes: Debian/Ubuntu `libasound2t64` (ou `libasound2`), `libvulkan1`,
  `mesa-vulkan-drivers`, `libxkbcommon0`, `libwayland-client0`, `libx11-6`,
  `libxcursor1`, `libdbus-1-3`; Fedora `alsa-lib`, `vulkan-loader`,
  `mesa-vulkan-drivers`, `libxkbcommon`, `libwayland-client`, `libX11`, `libXcursor`,
  `dbus-libs`. A maior parte já vem numa instalação gráfica comum.
- Sem placa de som, o programa roda mudo.

### O que vem nas pastas
Os pacotes funcionam em **modo portátil** (arquivo `portable.flag`): preferências, dados
e a pasta `MyChess/` ficam dentro da pasta extraída; nada é gravado fora dela. Apague o
`portable.flag` para usar a pasta pessoal do sistema.

- `MyChess/Databases/`: bases `Capablanca` (599 partidas) e `Carlsen` (7818), cada uma
  com seus três arquivos `.si4/.sg4/.sn4` (mantenha-os juntos), e
  `SicilianNajdorf6Bg5.pgn` (33 298 partidas da Najdorf, linha 6.Bg5, a linha principal
  clássica; as outras variantes não vão no pacote). Abra com **Arquivo > Abrir**.
  A primeira execução não apaga nem sobrescreve nada dessa pasta.
- `MyChess/Engine/` e `MyChess/Books/`: veja abaixo; o `LEIA-ME.txt` de cada uma explica.
- `LICENSE`, `LICENSES-THIRD-PARTY.txt`, `LICENSES-ASSETS.md`, `CREDITS.md`, `LEIA-ME.txt`.

**Procedência e licença das bases** (inspecionadas antes de empacotar): as partidas
vieram do autor do projeto; **a origem original não está documentada e nenhuma licença
de uso foi declarada nos arquivos** (a confirmação do dono está pendente). Capablanca
contém só anotações de avaliação geradas por engine (`Annotator "Stockfish 19"`), sem
comentários humanos; Carlsen e Najdorf têm só cabeçalhos e lances. Parte das partidas
vem de chess.com, chess24.com e lichess.org (campo Site); respeite os termos desses
serviços ao reutilizá-las. Detalhes em `CREDITS.md` dentro do pacote.

### Engine (Stockfish)
- **Linux:** o pacote inclui o **Stockfish 10** (2018, avaliação clássica, sem NNUE,
  ainda forte) em `MyChess/Engine/stockfish/`: build oficial "64 POPCNT" sem
  modificação, sob a **GPL v3**, com o código-fonte correspondente (`stockfish-10-src.zip`
  na mesma pasta; também em <https://github.com/official-stockfish/Stockfish>). O
  MyChess o reconhece sozinho na primeira execução (verificado: aparece em
  `engines.toml` como "Stockfish 10 64 POPCNT").
- **Windows:** não há engine no pacote (não foi possível obter o Stockfish 10 para Windows
  de fonte oficial). Baixe o Stockfish em <https://stockfishchess.org/download/>,
  extraia numa subpasta de `MyChess/Engine/` (o nome do executável deve começar com
  `stockfish`) ou adicione manualmente pela janela de engines.
- Recomendado em qualquer sistema: use o Stockfish atual (muito mais forte) do site oficial.

### Verificar o SHA-256
```sh
sha256sum -c SHA256SUMS.txt          # Linux (na pasta dist/ ou onde baixou os arquivos)
```
Windows (PowerShell): `Get-FileHash .\MyChess-0.1.0-alpha-windows-x86_64.zip -Algorithm SHA256`
e compare com a tabela.

### Compilar do código-fonte
Veja [Compilar e testar](#compilar-e-testar) e [Pasta do usuário, modo portátil e linha
de comando](#pasta-do-usuário-modo-portátil-e-linha-de-comando). Os pacotes são gerados
com `packaging/build-dist.sh` ([`packaging/README.md`](packaging/README.md)).

## Funcionalidades

Resumo por área; o detalhe e o grau de conclusão estão em
[`ROADMAP.md`](ROADMAP.md).

- **Partidas e notação:** tabuleiro com arrastar e soltar, variações,
  comentários, NAGs, marcações (setas e casas), desfazer/refazer, entrada de
  lances por teclado, Chess960 no núcleo e no PGN.
- **Formatos:** PGN (inclusive `.pgn.gz`, codificações antigas), EPD, ECO, bancos
  SI4 do Scid vs PC (leitura e escrita, compatíveis byte a byte como objetivo),
  cache de árvore `.stc`, máscaras `.stm`, livros Polyglot.
- **Bancos:** 8 slots e Clipbase, lista de partidas, filtros, buscas por
  cabeçalho, posição, material, lances e táticas, importação e exportação de
  PGN, ferramenta de linha de comando `mychess-cli`.
- **Aberturas:** árvore com estatísticas, máscaras, livros (criar, mesclar,
  ajustar pesos), navegador ECO, identificação de abertura e novidade.
- **Engines e análise:** UCI e XBoard, várias engines, MultiPV, barra de
  avaliação, setas, anotação automática, relatório da partida estilo
  chess.com, ameaça, tablebases Syzygy (local e online).
- **Estudo e treino:** táticas, treinador de aberturas, cálculo de variações,
  jogo tático e jogo sério contra engine, torneios entre engines.
- **Online e hardware:** Lichess (Board API), FICS, Novag Citrine e DGT.
- **Estatísticas:** tabela de torneio, estatísticas do filtro, gráficos de
  rating, rastreador de peças, localizador de jogadores, *spelling*.
- **Aparência:** temas claro e escuro, cor de realce, peças SVG, menus em
  lista ou em faixa, painéis com abas, interface em en-US e pt-BR.

## Estrutura

```
crates/
  mychess-core/       domínio do xadrez (posição, lances, notação, árvore de partida)
  mychess-pgn/        leitura e escrita de PGN/EPD
  mychess-db/         bancos de dados (SI4), buscas, árvore, relatórios, manutenção
  mychess-book/       livros de abertura Polyglot
  mychess-engine/     engines UCI e XBoard
  mychess-tablebase/  tablebases Syzygy (local e online)
  mychess-online/     cliente de rede (Lichess e FICS)
  mychess-eboard/     tabuleiros eletrônicos (Novag, DGT)
  mychess-app/        camada de aplicação, sem GUI (estado, casos de uso, efeitos)
  mychess-gui/        aplicativo desktop (iced): binário `mychess`
  mychess-cli/        ferramenta de linha de comando para bancos
xtask/                automação do workspace (cargo xtask ...)
docs/                 arquitetura, decisões (ADRs), formatos, mapa do Scid
reference/            código do Scid vs PC para consulta (não versionado)
.claude/              instruções para assistentes de código
```

A arquitetura (camadas + MVU, o equivalente moderno do MVC) está explicada em
[`docs/arquitetura.md`](docs/arquitetura.md); as decisões em
[`docs/adr/`](docs/adr/); os formatos binários em
[`docs/formatos/`](docs/formatos/).

## Pasta do usuário, modo portátil e linha de comando
Na primeira execução o MyChess mostra uma tela de boas-vindas e cria a pasta
`MyChess/` na sua pasta pessoal (`~/MyChess`, `C:\Users\<nome>\MyChess`), com `Engine/`,
`Databases/` e `Books/`. Ele nunca apaga nem sobrescreve nada dentro dela, e
**desinstalar o programa não a remove**. Uma engine conhecida colocada em `Engine/`
(por exemplo `Engine/stockfish/stockfish`) é registrada na lista de engines. As
preferências ficam na pasta de configuração do sistema.

- **Modo portátil:** crie um arquivo vazio `portable.flag` ao lado do executável (ou
  use `--portable`): as preferências, os dados e a pasta `MyChess` passam a ficar em
  `<pasta do executável>/MyChess/`. Se a pasta não for gravável, o programa avisa e
  volta ao modo normal. A variável `MYCHESS_HOME` troca a pasta pessoal.
- **Linha de comando:** `mychess [--portable] [--no-tip] [ARQUIVO]...` abre até oito
  arquivos (bases `.si4`, PGN, `.epd`, livros `.bin`). `mychess --help` e `--version`
  funcionam como esperado.
- **Dica do dia:** aparece ao iniciar (desligue na própria janela); Ajuda > Dica do dia.

## Compilar e testar

Requisito: [rustup](https://rustup.rs). O compilador certo (Rust 1.99.0, fixado
em `rust-toolchain.toml`) é instalado automaticamente na primeira compilação.

```sh
cargo xtask ci        # portão de qualidade completo: formatação, clippy, arquitetura, testes, docs, licenças
cargo xtask test      # só os testes
cargo xtask compat    # testes de compatibilidade com o Scid vs PC
cargo xtask help      # todas as tarefas
cargo run -p mychess-gui   # o aplicativo
```

No Linux, o áudio (`rodio`/`cpal`, feature `audio` do `mychess-gui`, ligada por
padrão) precisa dos cabeçalhos do ALSA para compilar; o programa não exige
placa de som para rodar (sem dispositivo, os sons ficam mudos):

```sh
sudo apt-get install libasound2-dev     # Debian/Ubuntu
sudo dnf install alsa-lib-devel         # Fedora
```

Windows e macOS não precisam de nada. Para compilar sem áudio:
`cargo build -p mychess-gui --no-default-features`.

Ferramentas opcionais, usadas automaticamente quando presentes:

```sh
cargo install --locked cargo-nextest cargo-deny cargo-insta
```

### Testes com tabelas Syzygy reais (opcional)

A lógica das tablebases é testada na CI com valores feitos à mão e com um
servidor falso. Os testes `real_*` do `mychess-tablebase` provam a ligação com o
`shakmaty-syzygy` usando arquivos de verdade; ficam `#[ignore]` e leem a pasta
da variável `MYCHESS_TEST_SYZYGY`. Bastam as tabelas de 3 peças (alguns KB):
`KQvK`, `KRvK` e `KPvK`, cada uma com `.rtbw` e `.rtbz`, baixadas à parte de
`tablebase.lichess.ovh/tables/standard/3-4-5-wdl` e `3-4-5-dtz`. Elas **não**
entram no repositório.

```sh
MYCHESS_TEST_SYZYGY=/caminho/da/pasta cargo test -p mychess-tablebase --test m8_8_local -- --ignored
```

## Código de referência do Scid vs PC

O diretório `reference/` não é versionado. Para recriá-lo:

```sh
mkdir -p reference
curl -L https://sourceforge.net/projects/scidvspc/files/source/scid_vs_pc-4.27.tgz/download | tar xz -C reference
```

Detalhes em [`docs/referencia-scid.md`](docs/referencia-scid.md).

## Como este projeto foi feito

**Este código foi escrito majoritariamente por agentes de IA**, usando o
[Claude Code](https://claude.com/claude-code) (modelos Claude, da Anthropic). O
trabalho segue um fluxo com três agentes, orquestrados por uma sessão principal:
um **implementador** escreve o código a partir de uma especificação, um
**testador** escreve testes derivados da especificação (não da implementação) e
um **revisor** faz uma revisão crítica e dá um veredito. Uma pessoa, o dono do
projeto, define o escopo, a arquitetura e as decisões (registradas em
[`docs/adr/`](docs/adr/), por exemplo a licença na
[ADR 0004](docs/adr/0004-licenca.md) e as funcionalidades fora do escopo na
[ADR 0011](docs/adr/0011-funcionalidades-fora-do-escopo.md)) e aceita ou recusa
os resultados.

O que isso **não** significa: não houve revisão humana linha a linha do código.
Pode haver defeitos que os testes e a revisão automatizada não pegaram, e
partes que dependem de teste manual ainda não foram testadas (veja
[Estado do projeto](#estado-do-projeto)).

O que sustenta a confiança, e que qualquer pessoa pode verificar:

- **Portão de qualidade:** `cargo xtask ci` roda formatação, `clippy` com avisos
  como erro, verificação das regras de camadas entre os crates, documentação
  (`rustdoc`), auditoria de licenças e a suíte de testes (mais de 21 000 testes
  automatizados na última rodada completa).
- **Compatibilidade com o Scid real:** `cargo xtask compat` compara o
  comportamento e os formatos com os do Scid vs PC.
- **Código aberto e auditável:** tudo está no repositório, inclusive as
  decisões e as instruções dadas aos agentes (`.claude/`, instruções para
  assistentes de código).

Críticas são bem-vindas: abra uma *issue* com defeitos, dúvidas de projeto ou
achados de revisão. Uma leitura cética do código é exatamente o que falta.

## Licença e créditos

MyChess é software livre sob a [GNU GPL v3 ou posterior](LICENSE).

Baseado no comportamento e nos formatos do **Scid vs PC** (Steven Atkinson e
colaboradores) e do **Scid** (Shane Hudson, Pascal Georges e colaboradores).
Trechos do leitor de PGN são tradução de código do Scid, com o cabeçalho de
origem nos arquivos. As regras de xadrez usam
[`shakmaty`](https://crates.io/crates/shakmaty) e
[`pgn-reader`](https://crates.io/crates/pgn-reader), de Niklas Fiekas.

Os pacotes de sons embutidos (`sfx`, `nes`, `futuristic` e `piano`, de
Enigmahack) vêm do Lichess e estão sob **AGPL-3.0-or-later**; valem só para
esses arquivos de áudio, não para o código. Veja
[`docs/adr/0015-pacotes-de-sons-e-decodificadores.md`](docs/adr/0015-pacotes-de-sons-e-decodificadores.md)
e [`assets/LICENSES.md`](assets/LICENSES.md), que também lista as licenças das
peças, fontes e ícones.

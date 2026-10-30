# Primeiros passos

**Neste capítulo:** o que é o MyChess, o que você precisa saber antes de usar, como é a tela principal, onde o programa guarda os seus arquivos e como abrir a sua primeira partida.

## O que é o MyChess

O MyChess é um programa para **guardar, estudar e analisar partidas de xadrez**. Ele é uma reimplementação moderna do Scid vs PC, um programa clássico de banco de dados de xadrez. Funciona no Linux, no Windows e no macOS.

Com ele você pode:

- guardar milhares (ou milhões) de partidas em **bases de dados** e achar qualquer uma delas em segundos;
- abrir e salvar arquivos **PGN**, o formato comum de partidas de xadrez;
- consultar a **árvore de aberturas** e os livros de abertura;
- analisar posições com **engines** (programas que jogam xadrez, como o Stockfish);
- jogar contra o computador, contra outras pessoas no Lichess e no FICS;
- treinar táticas, aberturas e cálculo de variantes.

O MyChess abre e grava as bases do Scid vs PC (arquivos `.si4`, `.sg4` e `.sn4`) sem conversão. Quem já usa o Scid vs PC pode continuar com as mesmas bases.

> O MyChess está em desenvolvimento. Neste manual, cada recurso que ainda não está completo traz uma linha em itálico começando por *Estado:*. Recursos sem essa linha estão prontos.

## Termos que você vai encontrar

Alguns nomes aparecem o tempo todo. Cada um é explicado de novo no [Glossário](14-glossario.md).

- **Partida**: uma partida de xadrez com lances, comentários e dados como jogadores, data e resultado.
- **Base de dados** (ou **banco**): um conjunto de partidas guardado em arquivos. Pode ter de uma a milhões de partidas.
- **PGN**: formato de texto para partidas de xadrez. Quase todo programa de xadrez lê e grava PGN.
- **Engine**: programa que calcula lances e avalia posições. O MyChess não vem com cérebro próprio: ele usa as engines que você instalar.
- **Variante**: uma linha alternativa dentro de uma partida ("e se, em vez deste lance, eu jogasse aquele?").

## A tela principal

A janela principal é dividida em **painéis**. O tabuleiro fica à esquerda e a lista de lances (a notação) fica ao lado. Você pode abrir outros painéis, como a lista de partidas, a análise e a árvore de aberturas, e pode arrastar, fechar e reorganizar todos eles.

De cima para baixo, a janela tem:

- **A barra de menus**: Arquivo, Editar, Partida, Banco, Buscar, Estatísticas, Jogar, Visualizar, Ferramentas, Online e Ajuda. Se você preferir uma barra de botões com ícones, veja [Aparência e preferências](11-aparencia-e-preferencias.md#estilo-de-menu).
- **Os painéis**: o tabuleiro, as faixas com os nomes dos jogadores, a notação e os demais.
- **A barra de status**, no rodapé, que mostra de quem é a vez, avisos e o andamento de tarefas longas. Para escondê-la, use **Visualizar > Barra de status**.

### Os painéis mais usados

- **Tabuleiro**: onde você joga os lances. Veja [O tabuleiro e a partida](02-tabuleiro-e-partida.md).
- **Notação** (aba **Lances**): a lista de lances da partida. As abas **Informações** e **Comentário** mostram os dados da partida e o comentário do lance atual.
- **Partidas**: a lista de partidas da base aberta. Mostre ou esconda com `Ctrl+L`.
- **Análise**, **Árvore de aberturas**, **Livros de abertura**, **Tablebases** e outros: abrem pelos menus **Ferramentas** e **Visualizar > Painéis**; o menu **Banco** reúne manutenção, nomes e Clipbase.

Para mostrar ou esconder um painel, use **Visualizar > Painéis** e marque ou desmarque o nome. Cada painel também tem um botão **×** para fechá-lo. Se a tela ficar bagunçada, use **Visualizar > Restaurar layout** para voltar à disposição original.

### Organizar os painéis

- **Mudar o tamanho**: arraste a divisória entre dois painéis.
- **Juntar painéis em abas**: arraste um painel e solte-o sobre outro.
- **Separar uma aba**: use o botão **Separar em um painel novo**.
- **Tabuleiro maior ou menor**: `Ctrl+Shift+Seta direita` e `Ctrl+Shift+Seta esquerda`.
- **Tela cheia**: `F11`.

O MyChess lembra a disposição dos painéis quando você fecha o programa.

## Instalação

*Estado: previsto. Os pacotes de instalação ainda não estão prontos.*

Hoje o MyChess é distribuído como código-fonte, e quem quer usá-lo precisa compilá-lo. O arquivo `README.md` do projeto explica como. Está previsto distribuir:

- no **Windows**: um instalador e uma versão portátil em arquivo `.zip`;
- no **Linux**: um pacote `.deb` e um arquivo AppImage, que roda sem instalar;
- no **macOS**: por enquanto, somente o código-fonte.

Os pacotes ainda não terão assinatura digital. No Windows, o SmartScreen pode avisar que o programa vem de um editor desconhecido. Isso é esperado enquanto não houver assinatura. Cada arquivo publicado terá uma soma de verificação (SHA-256) para você conferir.

## A pasta MyChess e os arquivos de exemplo

*Estado: previsto. Nada desta seção funciona ainda. Quando for implementado, deve se comportar como descrito aqui, mas os detalhes podem mudar.*

A ideia é que quem instala o MyChess já possa usá-lo, sem configurar nada. Na **primeira vez** que o programa abrir, ele deve criar uma pasta chamada `MyChess` na sua pasta pessoal (por exemplo, `C:\Users\seunome\MyChess` no Windows ou `/home/seunome/MyChess` no Linux), com três subpastas:

- `Engine`: as engines. O Stockfish viria aqui, já cadastrado na lista de engines.
- `Databases`: as bases de dados. Viria uma base de exemplo com partidas de domínio público.
- `Books`: os livros de abertura. Viria um livro de exemplo.

Pontos importantes do plano:

- O MyChess **nunca** sobrescreve nem recria um arquivo seu. Se você alterar, renomear ou apagar um arquivo de exemplo, ele não volta sozinho.
- Um comando no menu Ajuda recopiaria os exemplos que faltarem.
- Desinstalar o programa **não** apaga a pasta `MyChess`.
- No **modo portátil** (um arquivo `portable.flag` ao lado do programa), tudo ficaria na pasta do programa. É a opção para levar o MyChess num pendrive.
- Quem compila o programa a partir do código-fonte não recebe os exemplos automaticamente.

Enquanto isso não existe, você mesmo adiciona uma engine (veja [Engines e análise](06-engines-e-analise.md#gerenciar-engines)) e abre as suas bases (veja [Bancos de dados](03-bancos-de-dados.md)).

## Abrir uma base ou uma partida

1. Escolha **Arquivo > Abrir...** (`Ctrl+O`).
2. No diálogo, selecione uma base do Scid (`.si4`) ou um arquivo PGN (`.pgn` ou `.pgn.gz`).
3. Clique em **Abrir**.

O MyChess abre o arquivo em um dos **espaços de base** (os "slots"; veja [Bancos de dados](03-bancos-de-dados.md#os-slots-e-a-clipbase)) e mostra as partidas na lista de partidas. Se a lista não estiver visível, aperte `Ctrl+L`.

Para ver uma partida no tabuleiro, **clique duas vezes** sobre ela na lista. Depois use as setas do teclado para andar pelos lances: `Seta direita` avança, `Seta esquerda` volta, `Home` vai ao início e `End` vai ao fim.

Um arquivo PGN abre como uma base somente leitura, com a lista das suas partidas (veja [Bancos de dados](03-bancos-de-dados.md#abrir-um-arquivo-pgn-como-base)).

Para começar do zero, escolha **Arquivo > Novo** (`Ctrl+Shift+N`): ele cria uma partida vazia.

## Quando algo dá errado

O MyChess mostra avisos na barra de status e, em casos mais sérios, em uma janela. Alguns exemplos:

- **"Arquivo não encontrado"** ou **"Permissão negada"**: o arquivo mudou de lugar ou você não tem permissão para ele.
- **"Os arquivos do banco estão danificados ou têm uma versão não suportada"**: a base está corrompida ou é de um formato que o MyChess não conhece.
- **"Este banco está no formato antigo SI3"**: bases antigas precisam ser atualizadas para o formato SI4 antes de abrir.

Para mais ajuda, veja [Perguntas frequentes e solução de problemas](13-perguntas-frequentes.md).

## Veja também

- [O tabuleiro e a partida](02-tabuleiro-e-partida.md)
- [Bancos de dados](03-bancos-de-dados.md)
- [Aparência e preferências](11-aparencia-e-preferencias.md)
- [Atalhos de teclado](12-atalhos-de-teclado.md)
- [Glossário](14-glossario.md)

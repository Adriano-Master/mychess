# Glossário

**Neste capítulo:** o significado dos termos de xadrez e de informática que aparecem neste manual e no programa, em ordem alfabética.

## A

- **Abertura**: os primeiros lances de uma partida. Cada abertura conhecida tem um nome e um código [ECO](#e).
- **Árvore de abertura**: a tabela que mostra, para uma posição, todos os lances jogados nela em uma base e os resultados de cada um. Veja [Aberturas](05-aberturas.md).
- **Arena**: tipo de torneio do Lichess em que os jogadores jogam várias partidas seguidas durante um tempo. Ainda não é suportado.

## B

- **Banco**: o mesmo que **base de dados**.
- **Base de dados**: conjunto de partidas guardadas em arquivos e organizadas para busca rápida. O MyChess usa o formato [SI4](#s).
- **Bandeira (flag)**: uma marca que se coloca em uma partida de uma base, como "apagada" (D) ou "tática" (T).
- **Book**: veja **livro de abertura**.

## C

- **Cache da árvore**: arquivo `.stc` que guarda árvores de abertura já calculadas para a base responder mais depressa.
- **Centipeão**: a centésima parte de um peão. É a unidade com que muitas engines medem a avaliação (100 centipeões = 1 peão).
- **Chess960**: variante do xadrez em que as peças da primeira fileira começam em ordem sorteada (também chamada Fischer Random). Veja [O tabuleiro e a partida](02-tabuleiro-e-partida.md).
- **Clipbase**: base temporária, só na memória, que serve para juntar partidas. Veja [Bancos de dados](03-bancos-de-dados.md).
- **Coach**: segunda engine que comenta os seus lances durante uma partida contra o computador.

## D

- **Desempenho (Perf)**: o rating que um jogador teria mostrado em um conjunto de partidas, estimado pelos resultados e pelo rating dos adversários.
- **DTZ**: "distância até o zeramento". Nas tablebases Syzygy, é o número de meios-lances até a próxima captura ou lance de peão mantendo o resultado. Não é o número de lances até o mate.
- **Duplicata**: partida repetida em uma base.

## E

- **ECO**: sigla de *Encyclopaedia of Chess Openings*. É o sistema de códigos, de `A00` a `E99`, que classifica as aberturas. Códigos estendidos acrescentam uma letra (`B12j`).
- **Elo**: o sistema de pontuação de força de jogadores. Um Elo de 1500 é de clube iniciante, 2000 é de bom jogador de clube e 2500 ou mais é de grande mestre. Muitos sites chamam o Elo de **rating**.
- **Engine**: programa que calcula lances e avalia posições, como o Stockfish.
- **EPD**: *Extended Position Description*. Formato de texto que lista posições (uma por linha) com marcas chamadas **opcodes**, como `bm` (melhor lance) e `ce` (avaliação).
- **Espeto (skewer)**: tática em que uma peça valiosa é atacada e, ao sair, deixa outra atrás dela exposta.

## F

- **FEN**: *Forsyth-Edwards Notation*. Uma linha de texto que descreve uma posição inteira: as peças, de quem é a vez, os direitos de roque, a casa de en passant e os contadores de lances.
- **Figurinas**: pequenos desenhos de peças usados no lugar das letras na lista de lances (♘ no lugar de N).
- **Filtro**: o conjunto de partidas de uma base que a lista de partidas mostra no momento. As buscas reduzem o filtro.
- **FICS**: *Free Internet Chess Server*, servidor de xadrez gratuito e antigo. Veja [Jogar](07-jogar.md).
- **Final**: a fase da partida com poucas peças. Também o conjunto de posições dessa fase, como "final de torre e peão".

## G

- **Gambito**: abertura em que um lado oferece material (em geral um peão) em troca de desenvolvimento ou iniciativa.
- **Garfo**: tática em que uma peça ataca duas ou mais peças ao mesmo tempo.

## H

- **Hash**: opção das engines que define quanta memória a engine usa para lembrar posições já calculadas. Mais memória costuma dar uma análise melhor.

## I

- **Incremento**: tempo que se acrescenta ao relógio de um jogador a cada lance feito.
- **Informant (valores informantes)**: os símbolos usados pelo *Chess Informant* para avaliar lances e posições (`!?`, `?`, `+=`, `+-`...). A anotação automática os escolhe com limites que você pode ajustar.

## L

- **Lance nulo**: passar a vez, sem mover peça. Não existe no xadrez de verdade, mas é usado na análise (por exemplo, para ver a ameaça do adversário).
- **Lichess**: site gratuito de xadrez online. Veja [Jogar](07-jogar.md).
- **Livro de abertura**: arquivo com os lances recomendados para cada posição, usado para jogar ou analisar aberturas. O formato mais comum é o [Polyglot](#p).
- **LOS**: *likelihood of superiority*, a probabilidade de uma engine ser mais forte que outra, calculada a partir dos resultados de um torneio.

## M

- **Marcador**: atalho guardado para uma partida de uma base. Veja [Bancos de dados](03-bancos-de-dados.md).
- **Máscara (da árvore)**: arquivo `.stm` com cores, símbolos, marcadores e comentários dos lances de uma árvore de abertura.
- **Material**: o conjunto das peças de cada lado.
- **MultiPV**: modo em que a engine mostra as *N* melhores linhas, e não só a melhor.

## N

- **NAG**: *Numeric Annotation Glyph*. Código numérico de um símbolo de anotação (o `$1` é `!`, o `$2` é `?`). Veja [O tabuleiro e a partida](02-tabuleiro-e-partida.md).
- **Nós**: o número de posições que a engine calculou.
- **Novidade**: o primeiro lance de uma partida que nenhuma outra partida da base jogou.

## O

- **Opcode**: marca de um registro EPD, como `bm` ou `id`.

## P

- **PGN**: *Portable Game Notation*. Formato de texto padrão para partidas de xadrez, com um cabeçalho (jogadores, evento, data...) e os lances. Quase todo programa lê e grava PGN. Arquivos `.pgn.gz` são PGN compactados.
- **Polyglot**: formato de livro de abertura em arquivo `.bin`, usado por muitas engines e programas.
- **Ponder**: pensar no tempo do adversário. A engine calcula enquanto você pensa.
- **Premove**: marcar o seu próximo lance antes da vez, em partidas online.
- **Profundidade**: quantos meios-lances a engine olhou à frente.
- **PV**: *principal variation*, a sequência de lances que a engine considera melhor.

## R

- **Rating**: o mesmo que Elo.
- **Repertório**: o conjunto de aberturas que um jogador escolhe jogar.

## S

- **SAN**: *Standard Algebraic Notation*. A forma comum de escrever lances (`Nf3`, `exd5`, `O-O`, `e8=Q+`). As letras das peças são as inglesas: `K` rei, `Q` dama, `R` torre, `B` bispo, `N` cavalo.
- **Score**: a pontuação, em porcentagem, de um lado (vitória vale 1, empate 0,5).
- **SI4**: o formato de base de dados do Scid vs PC (versão 4). Uma base é feita de três arquivos: `.si4` (índice), `.sn4` (nomes) e `.sg4` (partidas).
- **Slot**: um dos oito espaços numerados em que o MyChess mantém bases abertas.
- **Spelling (ortografia)**: o arquivo `.ssp` com os nomes corretos de jogadores, usado para corrigir a grafia dos nomes de uma base.
- **Stockfish**: uma das engines de xadrez mais fortes, gratuita e de código aberto.
- **Syzygy**: o formato das tablebases de finais com até 7 peças.

## T

- **Tablebase**: banco de dados com o resultado exato de todas as posições de finais com poucas peças. Veja [Engines e análise](06-engines-e-analise.md).
- **Tag**: um par nome e valor do cabeçalho de uma partida (por exemplo, `Event "Torneio de Verão"`).
- **Token**: uma "senha" de acesso gerada pelo Lichess para um programa usar a sua conta.
- **Transposição**: chegar à mesma posição por uma ordem de lances diferente.

## U

- **UCI**: *Universal Chess Interface*. O protocolo de conversa entre um programa de xadrez e uma engine, usado pela maioria das engines modernas. Veja também **XBoard**.

## V

- **Variante**: uma linha alternativa dentro de uma partida. Também se usa "variante" para os tipos de xadrez que mudam as regras (como o Chess960).

## W

- **WDL**: *win, draw, loss* (vitória, empate, derrota). Nas tablebases Syzygy, os arquivos `.rtbw` guardam o WDL e os `.rtbz` guardam a DTZ.

## X

- **XBoard**: protocolo mais antigo de conversa entre programa e engine, também chamado CECP.

## Veja também

- [Índice do manual](00-indice.md)
- [Primeiros passos](01-primeiros-passos.md)
- [Perguntas frequentes e solução de problemas](13-perguntas-frequentes.md)

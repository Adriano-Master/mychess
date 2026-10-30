# Jogar

**Neste capítulo:** como jogar contra o computador (partida normal, com relógio ou o jogo tático), como organizar torneios entre engines, como jogar pela internet no Lichess e no FICS, como usar um tabuleiro eletrônico, como ajustar os sons e como desistir, pedir revanche e oferecer empate.

## Jogar contra o computador

Escolha **Jogar > Jogar contra a engine...** (`Ctrl+Shift+P`). Você precisa de pelo menos uma engine UCI cadastrada (veja [Engines e análise](06-engines-e-analise.md#gerenciar-engines)). Se não houver nenhuma, a janela avisa "Não há engine UCI cadastrada" e oferece o botão **Abrir Engines...**.

### Configurar a partida

1. **Engine**: escolha o adversário. **Configurar engine...** abre a janela de engines.
2. **Você joga de**: **Brancas**, **Pretas** ou **Aleatório**.
3. **Controle de tempo**: escolha o **Modo**:
   - **Relógio com incremento**: cada lado tem um tempo total (**Tempo das Brancas**, **Tempo das Pretas**, em minutos) mais um incremento por lance (em segundos). É o jogo sério, com relógio.
   - **Tempo por lance**: a engine pensa o mesmo tempo em todo lance.
   - **Profundidade fixa**: a engine busca até uma profundidade (em meios-lances).
   - **Nós fixos**: a engine calcula um número fixo de posições.
4. **Posição inicial**: **Partida nova**, **Posição atual** (a do tabuleiro) ou **Abertura específica**. Para esta última, escreva uma linha por abertura, no formato `Siciliana: 1.e4 c5 2.Nf3 d6`; a engine joga os lances da linha.
5. **Livro de aberturas**: marque **Jogar com livro**, escolha o arquivo (**Escolher arquivo...**) e se a engine usa o **Sorteio ponderado** ou o **Melhor lance** do livro.
6. **Força da engine**: se a engine declara, você pode **Limitar a força (UCI_Elo)** ou usar o **Skill Level**.
7. **Ajuda e raciocínio**:
   - **Deixar a engine pensar no meu tempo (ponder)**: a engine continua calculando enquanto você pensa. Exige que a engine ofereça a opção Ponder.
   - **O coach acompanha meus lances (uma segunda engine)**: uma segunda engine comenta cada lance seu ("boa jogada", "imprecisão", "erro", "erro grave"). É preciso uma engine diferente da adversária. As opções **Mostrar o lance melhor quando o meu foi fraco** e **Mostrar a perda em peões** ajustam o que ele diz.
8. **Gravação**: **Gravar o relógio nos comentários dos lances**.
9. Clique em **Jogar**.

### Durante a partida

O painel mostra "Contra *nome da engine*", de quem é a vez e os relógios. Os botões são:

- **Desistir**: abandona a partida; ela é salva como derrota (o MyChess pergunta antes).
- **Oferecer empate**: propõe empate à engine. Se ela recusar, aparece "A engine recusou o empate".
- **Desfazer lance**: desfaz o seu último lance e a resposta da engine. Fica desligado quando o coach está ligado.
- **Reiniciar**: recomeça com as mesmas configurações.
- **Sair**: sai da partida sem resultado e sem salvar.
- **Continuar**: retoma uma partida em pausa.

Se você sair do livro enquanto ele ainda tem lances, o MyChess pergunta "Sair do livro?". O mesmo vale ao deixar uma abertura específica ("Sair da linha?").

Enquanto a engine joga, a partida não pode ser editada ("A partida não pode ser editada enquanto a engine joga."). Se a engine para de responder, a partida fica em pausa e você pode **Continuar com** outra engine ou **Encerrar partida**.

**Pré-lance contra a engine.** Na vez da engine você já pode arrastar uma peça para marcar o seu próximo lance (*premove*). `Esc` ou o botão direito do mouse cancelam. A caixa **Premove** sob o tabuleiro liga e desliga o recurso. O roque também é aceito, e uma promoção marcada de antemão vira dama contra a engine.

### Partida Chess960

**Jogar > Nova partida Chess960...** pede o número da posição inicial (de 0 a 959; 518 é o arranjo padrão) ou sorteia uma com **Aleatória**, e **Iniciar partida** coloca a posição no tabuleiro. Para jogar contra a engine, use depois **Jogar > Jogar contra a engine...** com **Posição atual**. A engine precisa oferecer a opção `UCI_Chess960`; se não oferecer, o MyChess avisa e não começa a partida.

### Fim da partida

A partida termina por xeque-mate, afogamento, material insuficiente, regra dos 50 lances, tripla repetição, tempo, desistência ou empate por acordo. Na caixa **Fim da partida**, clique em **Salvar partida** para guardá-la na base do slot atual (ou em um PGN, se a base é somente leitura). O resultado fica na partida. Para jogar de novo com as cores trocadas, use **Revanche**.

## Jogo tático

O **jogo tático** é uma partida contra uma engine com **força limitada a um Elo**, para treinar contra um adversário do seu nível. Escolha **Jogar > Jogo tático...** (`Ctrl+Shift+F7`).

- **Elo da engine**: a força desejada. O MyChess usa a opção `UCI_Elo` da engine; se ela não tem, usa o `Skill Level`; se não tem nem isso, limita a profundidade (e a força fica só aproximada).
- **Mudar o Elo depois de cada partida**: o Elo sobe quando você ganha e desce quando perde, pelo **Passo** escolhido.
- **Posição inicial**: **Partida nova**, **Posição atual**, **Abertura específica**, **Problema do treino de táticas**, um **FEN** ou **posição sorteada** de um arquivo ou da base atual.
- O MyChess guarda seu **placar** (vitórias, empates, derrotas, maior sequência e o maior Elo vencido). **Zerar o placar** pede confirmação.

## Torneios entre engines

Para comparar engines ou testar uma delas contra várias, escolha **Jogar > Torneio entre engines...** (`Ctrl+Shift+R`).

1. Em **Participantes**, **Adicionar engine** (pelo menos duas). Cada uma pode ter suas opções no formato `Nome=Valor; Nome=Valor`.
2. Em **Torneio**, escolha o **Formato**: **Todos contra todos**, **Carrossel** ou **Um contra todos** (neste, escolha a **Engine testada**), e as **Partidas por par**.
3. Em **Tempo**, o mesmo controle do jogo normal (relógio com incremento, tempo por lance, profundidade ou nós). O MyChess mostra uma estimativa ("12 partidas, no máximo 1 h").
4. Em **Aberturas**, as partidas começam da **Posição inicial**, da **Posição atual**, de um **Livro** ou de uma **Suíte de aberturas** (arquivo EPD ou PGN, em ordem ou aleatória). **Mesma abertura nas duas partidas de um par** deixa o duelo justo.
5. Em **Engines**: **Pensar no tempo do adversário (ponder)**, **Reiniciar as engines antes de cada partida**, **Hash** e **Threads**.
6. Em **Adjudicação**, o MyChess pode encerrar partidas decididas sem esperar o fim: **Adjudicar partida ganha** (a nota e o número de lances seguidos), **Adjudicar partida empatada**, **Empate em um número de meios-lances** e **Adjudicar pela tablebase Syzygy**.
7. Em **Falhas**, escolha o que fazer com uma partida em que uma engine quebra ou perde por tempo.
8. Em **Gravação**, escolha o formato do comentário com a nota e se grava o relógio e o tempo de cada lance.
9. Clique em **Iniciar**.

O **Painel do torneio** (**Jogar > Painel do torneio**) tem as abas **Ao vivo**, **Pareamentos**, **Classificação**, **Cruzada** e **Registro**, e os botões **Pausar**, **Continuar**, **Parar depois desta partida**, **Pular partida** e **Encerrar**. A tabela de classificação mostra também o Elo e o LOS (a probabilidade de uma engine ser mais forte que outra).

O torneio fica salvo. Para continuar um torneio interrompido, use **Jogar > Retomar um torneio...**. Ao fim, **Abrir PGN como banco** abre as partidas jogadas.

## Jogar no Lichess

O MyChess joga no [Lichess](https://lichess.org) pela interface de tabuleiro oficial. Você precisa de uma conta de usuário comum (contas de bot não servem). Só o xadrez padrão é aceito; variantes do Lichess não funcionam.

### Conectar

1. Escolha **Online > Conectar ao Lichess...** (`Ctrl+Shift+O`).
2. A forma mais simples é **Entrar pelo navegador**: o MyChess abre o lichess.org para você autorizar o programa. Se o navegador não abrir, copie o link de autorização.
3. A outra forma é criar um **token de acesso pessoal**: clique em **Copiar o link para criar o token**, abra no navegador, crie o token com as permissões indicadas e cole-o em **Token de acesso pessoal**. O token nunca é mostrado nem gravado nas preferências.
4. Escolha onde guardar o token: **No chaveiro do sistema (recomendado)**, **Em um arquivo que só você lê** ou **Não guardar (só nesta sessão)**.
5. Clique em **Conectar**.

**Online > Sair da conta** desconecta. Para revogar o token de vez, acesse lichess.org/account/oauth/token.

### Jogo limpo: o que fica bloqueado

Para respeitar as regras do Lichess, **enquanto você joga uma partida online** o MyChess desativa engines, árvore de aberturas, tablebases e buscas por posição. O aviso é "Partida online: assistência desativada". Isso vale desde que a partida começa até ela terminar. Se uma partida terminar de forma estranha e o bloqueio ficar preso, use **Online > Remover o bloqueio de assistência...** (só depois de ter certeza de que a partida acabou).

### Buscar uma partida

1. **Online > Buscar partida...** (`Ctrl+Shift+U`).
2. Escolha **Minutos** e **Incremento (s)**, se a partida vale rating (**Partida valendo rating**) e a cor (**Sorteio**, **Brancas** ou **Pretas**).
3. Clique em **Buscar**. O MyChess mostra "Procurando oponente...". **Parar de procurar** cancela.

O Lichess só aceita nas buscas ritmos rápidos ou mais lentos.

### Desafiar um usuário e responder desafios

- **Online > Desafiar um usuário...**: digite o nome no Lichess e o ritmo (blitz ou mais lento). Você pode **Retirar o desafio**. Se o usuário recusar, o MyChess mostra o motivo.
- **Online > Desafios recebidos (N)...**: lista os desafios com **Aceitar** e **Recusar**. Desafios de variantes ou ritmos que o MyChess não joga aparecem como "O MyChess não joga este desafio".

### Durante a partida

O relógio é o do servidor. Os botões do painel são: **Desistir**, **Abortar** (só antes da partida começar de fato), **Oferecer empate**, **Aceitar empate** e **Recusar**, **Pedir voltar o lance** (e **Aceitar volta** e **Recusar volta**), **Reivindicar vitória** e **Reivindicar empate** (quando o oponente sai) e **Revanche**.

O **pré-lance** (*premove*) permite marcar o seu próximo lance durante a vez do adversário; `Esc` cancela. Se o lance exigir promoção, o MyChess pergunta a peça.

A partida terminada é guardada na Clipbase ("A partida foi guardada na Clipbase"). Se você fechar o programa no meio de uma partida, o relógio continua correndo no servidor.

### Observar

**Online > Observar uma partida...** abre uma partida em andamento ou terminada. Cole o link ou o código de 8 caracteres. Partidas em andamento chegam com 3 lances de atraso. **Parar de observar** fecha.

### Torneios Arena do Lichess

*Estado: previsto. Ainda não existe.* O plano é entrar em torneios Arena por link ou código, jogar, acompanhar a classificação e criar torneios. Só valeriam os ritmos que a interface de tabuleiro do Lichess aceita (8 minutos ou mais).

## Jogar no FICS

O **FICS** (Free Internet Chess Server) é um servidor de xadrez gratuito, antigo e muito frequentado. Abra com **Online > FICS...**.

1. Deixe o nome vazio para entrar como **convidado**, ou digite o seu nome e senha do FICS. **Atenção**: o protocolo do FICS não é cifrado e a senha viaja em texto claro. Use uma senha que você não usa em nenhum outro lugar.
2. Clique em **Conectar** (ou **Conectar como convidado**).

A janela do FICS oferece:

- **Gráfico de anúncios**: cada ponto é um jogador procurando partida (com o rating e o ritmo). Clique no ponto para ver os detalhes e clique em **Jogar este anúncio** para aceitar.
- **Encontrar oponente**: **Procurar** publica o seu próprio anúncio e **Retirar** o cancela. **Desafiar** convida um jogador pelo nome.
- **Ofertas**: desafios, empates, pedidos de voltar lance e de abortar, com **Aceitar** e **Recusar**.
- **Observar**, **Seguir** e **Examinar**: acompanham partidas dos outros (até 8 ao mesmo tempo), seguem um jogador de partida em partida ou examinam uma partida. **Mostrar** traz uma partida observada para o tabuleiro e **Parar** a deixa.
- **Palestras (canal 24)** e a **linha de comando** para qualquer comando do FICS.
- **Preferências**: **Premove**, **Gravar o tempo gasto em cada lance ([%emt])**, **Gravar o relógio de cada lance ([%clk])** e a **Promoção** automática.

Como no Lichess, as ajudas ficam desativadas enquanto você joga ("Uma partida começou: as ajudas estão desativadas"). No FICS você só pode **desistir**; não há revanche. O MyChess mostra o tempo de resposta ("ping").

## Tabuleiro eletrônico

*Estado: pronto no programa; falta teste com tabuleiros reais.*

**Ferramentas > Tabuleiro eletrônico...** liga o MyChess a um tabuleiro físico, para que os lances feitos nas peças apareçam na tela.

- **Novag Citrine (porta serial)**: escolha a **Porta serial** (**Atualizar** procura as portas) e clique em **Conectar**.
- **DGT ou outro (programa driver)**: escolha o **Programa driver** e os **Parâmetros do driver** (duas letras: `l` ou `r` para o lado do relógio; `a`, `b` ou `w` para os lances enviados por ambos, pretas ou brancas).

Depois de conectado, a janela mostra os relógios, o último lance recebido e se o tabuleiro e a partida estão iguais ("O tabuleiro e a partida estão iguais."). **Sincronizar** corrige diferenças, **Girar** inverte o tabuleiro e **Lances enviados por** filtra o lado. Lances ilegais do tabuleiro são ignorados e aparecem no **Console**.

Só tabuleiros desses dois tipos são suportados.

## Sons

Abra **Visualizar > Sons…**.

- **Tocar sons** liga e desliga todos os sons, e **Volume** ajusta o volume.
- **Som a cada lance novo**, **Som ao avançar** e **Som ao voltar**.
- **Mesmo tique para todo lance** usa um som único para todos os lances.
- **Pacote de sons**: escolha o conjunto de sons. O **Padrão (sintetizado)** vem com o programa; outros pacotes aparecem aqui. Para adicionar um pacote seu, abra a pasta com **Abrir pasta de sons** e copie uma pasta por pacote. O pacote mostra o autor e a licença.
- **Falar o lance** anuncia o lance em voz, se você tiver os arquivos de palavras na **Pasta de sons** (nomes como `King.wav`, `a.wav`, `1.wav`, `Check.wav`, `Mate.wav`, como no Scid vs PC).
- **Testar** toca um som de exemplo.

Sem placa de som, o programa funciona normalmente, só que mudo.

## Desistir, revanche e empate

Estes comandos funcionam em todos os modos de partida viva:

- **Partida > Desistir** (ou o botão **Desistir**): abandona a partida em andamento, depois de uma pergunta. Contra a engine e no Lichess, a partida é salva como derrota.
- **Partida > Revanche** (ou o botão **Revanche**): joga de novo contra o mesmo adversário **com as cores trocadas**. Não existe no FICS.
- **Oferecer empate**: propõe empate. A engine ou o adversário aceita ou recusa.

## Limites

- Não existe xadrez por correspondência (Xfcc, relés, e-mail). Para jogos longos, use o Lichess.
- Online, só o xadrez padrão é aceito.
- Só engines UCI jogam contra você; engines XBoard servem para análise.

## Veja também

- [Engines e análise](06-engines-e-analise.md)
- [Treinar](08-treinar.md)
- [Aberturas](05-aberturas.md)
- [Atalhos de teclado](12-atalhos-de-teclado.md)
- [Perguntas frequentes e solução de problemas](13-perguntas-frequentes.md)

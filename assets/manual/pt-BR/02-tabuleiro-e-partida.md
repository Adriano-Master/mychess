# O tabuleiro e a partida

**Neste capítulo:** como jogar lances no tabuleiro, andar pela partida, criar variantes, escrever comentários, usar símbolos de anotação, desfazer erros, girar o tabuleiro, escolher a forma de exibir os lances, copiar e colar partidas e posições, montar uma posição e o que fazer com Chess960.

## Jogar um lance

Você tem três maneiras de jogar:

1. **Clicar e clicar**: clique na peça (as casas de destino ficam marcadas) e clique na casa de destino.
2. **Arrastar**: segure a peça, arraste até a casa de destino e solte. Se você soltar fora do tabuleiro, nada acontece.
3. **Digitar**: com o tabuleiro em foco, digite o lance, como `Nf3` ou `e2e4`. As letras das peças são as inglesas: `K` rei, `Q` dama, `R` torre, `B` bispo e `N` cavalo (o peão não tem letra). A barra de status mostra o que você digitou e os lances possíveis. Aperte `Enter` para confirmar e `Esc` para apagar o que digitou.

O MyChess só aceita lances legais. Se você tentar um lance ilegal, aparece o aviso **Lance ilegal** na barra de status.

### Promoção de peão

Quando um peão chega à última fileira, aparece uma coluna com as peças **Dama**, **Torre**, **Bispo** e **Cavalo**. Clique na peça desejada. `Enter` escolhe a dama e `Esc` cancela o lance.

### Lance nulo

Arrastar o rei sobre o outro rei joga um **lance nulo** (passar a vez). Ele serve para análises e não é um lance de verdade. Não é permitido quando o rei está em xeque.

### Quando já existe um lance na posição

Se você jogar um lance diferente do que já está na partida, o MyChess pergunta o que fazer (**Substituir lance?**):

- **Substituir lance**: troca o lance e **apaga** todos os lances seguintes.
- **Nova linha principal**: o seu lance vira a linha principal, e o antigo passa a ser uma variante.
- **Adicionar variante**: o seu lance vira uma variante e a linha principal continua como está.
- **Tentar variante**: abre o modo tentativa (veja abaixo).

No fim da partida, o lance é simplesmente acrescentado, sem pergunta. Se você jogar um lance que já existe na partida (na linha principal ou em uma variante), o MyChess só vai até ele.

## Andar pela partida

Use os botões da barra de comandos, abaixo da lista de lances, ou o teclado.

- `Seta direita`: um lance para frente.
- `Seta esquerda`: um lance para trás.
- `Seta para baixo`: dez lances para frente.
- `Seta para cima`: dez lances para trás (ou sai da variante, se você estiver em uma).
- `Home`: início da partida. `End`: fim da partida.
- `Ctrl+G`: pede o número de um lance e vai até ele. Acrescente `...` ou `b` para ir ao lance das pretas.
- Um clique em qualquer lance da lista vai direto para ele.
- O botão **Jogo automático** avança sozinho, um lance a cada 5 segundos por padrão. Clique de novo para pausar.

Quando a partida tem variantes, a barra de comandos oferece **Entrar na variante** (`V`) e **Sair da variante** (`Z`). Ao entrar, se houver mais de uma variante, o MyChess mostra a janela **Variantes** para você escolher a linha.

## Variantes

Uma **variante** é uma linha alternativa a partir de uma posição.

### Criar uma variante

Há dois caminhos:

- Jogue na posição um lance diferente do que já existe e escolha **Adicionar variante** na pergunta.
- Use **Adicionar variante** na barra de comandos (`Ctrl+A`) e depois jogue o primeiro lance da variante.

### Mexer em variantes existentes

Clique com o botão direito em um lance da lista de lances para abrir o menu de contexto:

- **Promover variante a linha principal**
- **Tornar a primeira variante**
- **Apagar variante**

### Modo tentativa

O **modo tentativa** deixa você experimentar lances sem estragar a partida. Ative-o com **Experimentar variante** (`Ctrl+T`). Quando você sai do modo, tudo o que fez nele é descartado. Dentro dele, **Desfazer** fica desligado e algumas ferramentas (como a anotação automática) pedem que você saia primeiro.

## Comentários

Cada lance pode ter um **comentário** antes ou depois dele. Há também um comentário no início da partida.

1. Vá até o lance (ou clique com o botão direito nele e escolha **Editar comentário**).
2. Abra a aba **Comentário** ou aperte `Ctrl+E` para abrir o **Editor de comentários**.
3. Digite o texto na caixa **Digite um comentário…**.
4. O comentário é salvo quando você sai da caixa ou muda de lance. Você também pode clicar em **Salvar**.

Para apagar, clique em **Limpar** ou use **Apagar comentário** no menu do lance.

O comentário pode conter códigos de marcação, que o MyChess desenha no tabuleiro (veja a próxima seção).

### Desenhar setas e casas coloridas

Você pode marcar o tabuleiro para explicar uma ideia:

- **Botão direito em uma casa**: desenha ou apaga um círculo.
- **Botão direito arrastado de uma casa a outra**: desenha ou apaga uma seta.
- A cor depende da tecla segurada: sem tecla, verde; `Shift`, vermelho; `Ctrl` (`Cmd` no macOS), amarelo; `Alt`, azul.

As marcações ficam gravadas no comentário do lance, em códigos como `[%csl ...]` e `[%cal ...]` (padrão usado também por outros programas). Se você não quer ver esses códigos na lista de lances, ligue **Ocultar códigos de casas/setas** nas opções de exibição da notação. Para esconder as marcações do tabuleiro, desmarque **Visualizar > Mostrar casas coloridas e setas**.

## Símbolos de anotação (NAGs)

Os **NAGs** são os símbolos que comentam a qualidade de um lance ou de uma posição, como `!` (bom lance), `?` (erro), `!?` (interessante), `+-` (as brancas ganham) ou `=` (posição igual).

Há três jeitos de colocá-los:

- No **Editor de comentários**, na seção **Símbolos de anotação**, clique no símbolo.
- No menu do lance, em **Anotações**.
- **Digitando** com o tabuleiro em foco. Os símbolos `!`, `!!`, `?`, `??`, `!?`, `?!`, `=` e `~` precisam de `Enter` no fim (por exemplo, digite `!` e `Enter`). Os símbolos `+-`, `+/`, `+=`, `-+`, `-/` e `=+` valem assim que você digita o último caractere. A sequência vale só se você digitar as teclas em até 1,5 segundo.

Um lance pode ter vários símbolos. Se você preferir ver números (`$1`, `$2`) em vez dos símbolos, desligue **Anotações simbólicas** nas opções de exibição da notação.

## Desfazer e refazer

- **Desfazer**: `Ctrl+Z` (ou **Editar > Desfazer**).
- **Refazer**: `Ctrl+Y` ou `Ctrl+Shift+Z`.

O MyChess guarda os últimos 20 passos de edição.

## Apagar partes da partida

No menu **Editar > Remover** (e no menu de contexto da lista de lances) você encontra:

- **Comentários**: apaga todos os comentários da partida.
- **Variantes**: apaga todas as variantes.
- **Lances após esta posição**: apaga o que vem depois do lance atual (também é a tecla `Delete`).
- **Lances antes desta posição**: a partida passa a começar na posição atual.

Cada uma pede confirmação antes de apagar. Você pode desfazer depois com `Ctrl+Z`.

## Girar o tabuleiro

Use o botão **Virar tabuleiro**, **Partida > Virar tabuleiro** ou `Ctrl+F`.

## A lista de lances (notação)

O painel de notação mostra os lances da partida. Clique em um lance para ir até ele. Passe o mouse sobre um lance para ver uma prévia da posição (opção **Prévia do tabuleiro ao passar o mouse**).

Em **Visualizar > Lista de lances** e no botão de opções do painel você escolhe:

- **Disposição**: **Tabela** (duas colunas, brancas e pretas), **Texto recuado** (variantes com recuo) ou **Texto corrido**.
- **Figurinas das peças**: mostra ícones de peças (♘♗) no lugar das letras (N, B). Veja o [Glossário](14-glossario.md).
- **Coluna de tempo**: mostra o tempo gasto, quando a partida o tem.
- Outras opções: **Identar comentários**, **Linha principal em negrito**, **Mostrar comentários**, **Mostrar variantes**, **Mostrar resultado**, **Espaço após o número do lance** e **Cabeçalho** (**Completo**, **Curto (3 linhas)** ou **Oculto**).

Os botões **Copiar o jogo para a área de transferência** e **Salvar o jogo como…** ficam no mesmo painel.

## Informações da partida

A aba **Informações** mostra os jogadores, o evento, o resultado e o código ECO. Para editar esses dados, escolha **Partida > Informações da partida...** (`Ctrl+Shift+H`). A janela tem estes campos:

- **Brancas**, **Pretas**, **Evento**, **Local**, **Rodada**: ao digitar, o MyChess sugere nomes que já existem na base.
- **Data** e **Data do evento**: ano, mês e dia. Use `?` para o que você não sabe (por exemplo, `2024.??.??`). **Hoje** preenche a data de hoje e **=Data** copia a data da partida para a data do evento.
- **Resultado**.
- **Código ECO**: **Classificar** preenche o código a partir dos lances (precisa do arquivo ECO; veja [Aberturas](05-aberturas.md)).
- **Tags extras**: pares nome e valor, como `Anotador "Anand"`.

O MyChess avisa quando um campo tem valor inválido, por exemplo "O ano deve estar entre 0 e 2047." ou "O rating deve estar entre 0 e 4000.".

## Copiar e colar

- **Copiar FEN** (`Ctrl+Shift+C`): copia a posição atual. **FEN** é uma linha de texto que descreve uma posição.
- **Colar FEN** (`Ctrl+Shift+V`): começa uma partida nova a partir da posição copiada. Se o texto não for um FEN válido, o MyChess avisa e não muda nada.
- **Copiar PGN**: copia a partida inteira como texto PGN.
- **Colar PGN...** (`Ctrl+Shift+I`): abre uma janela onde você cola o texto de uma partida. Se o texto tiver mais de uma partida, o MyChess mostra a lista para você escolher. O botão **Inserir a partida atual** coloca no campo a partida que está aberta. A janela mostra os problemas encontrados no texto.

Se a partida atual tem alterações, o MyChess pergunta antes de substituí-la: **Salvar**, **Não salvar** ou **Cancelar**.

## Configurar uma posição

Para estudar um problema ou um final, monte a posição peça por peça:

1. Escolha **Editar > Configurar posição...** (`Ctrl+Shift+B`).
2. Clique em uma peça da paleta e depois nas casas do tabuleiro. Clique de novo na mesma peça para apagá-la. O botão direito apaga uma peça, o botão do meio copia a peça da casa e a roda do mouse troca a peça.
3. Ajuste **Lado que joga**, os direitos de roque (**Brancas O-O**, **Brancas O-O-O**, **Pretas O-O**, **Pretas O-O-O**), a **Coluna en passant**, o **Nº do lance** e os **Meios-lances**.
4. Use os atalhos **Tabuleiro vazio**, **Tabuleiro inicial**, **Trocar cores**, **Inverter** e **Transpor** quando fizer sentido. **Colar FEN** preenche tudo a partir de um texto.
5. Clique em **OK**.

A janela avisa sobre posições impossíveis: "Deve haver exatamente um rei branco", "Os reis não podem se tocar", "O lado que não joga está em xeque", "Um peão não pode ficar na primeira nem na oitava fileira". Corrija o problema para continuar.

Ao confirmar, o MyChess abre uma **partida nova** que começa nessa posição.

## Chess960

O **Chess960** (também chamado xadrez Fischer Random) começa com as peças em uma ordem sorteada. O MyChess lê e grava partidas de Chess960 em PGN e as joga corretamente, inclusive o roque. Ao ler um PGN de Chess960, ele mostra o aviso "A partida é de Chess960".

*Estado: em desenvolvimento (falta montar uma posição de Chess960 à mão e guardá-la em bases SI4).*

Limites atuais:

- Em **Configurar posição**, aparece o aviso "Posições de Chess960 ainda não podem ser montadas à mão".
- O formato de base do Scid (SI4) não prevê Chess960. Use arquivos PGN para guardar essas partidas.
- Livros de abertura Polyglot não aceitam Chess960.

## Veja também

- [Bancos de dados](03-bancos-de-dados.md)
- [Aberturas](05-aberturas.md)
- [Engines e análise](06-engines-e-analise.md)
- [Atalhos de teclado](12-atalhos-de-teclado.md)
- [Glossário](14-glossario.md)

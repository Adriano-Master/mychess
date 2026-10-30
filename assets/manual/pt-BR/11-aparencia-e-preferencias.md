# Aparência e preferências

**Neste capítulo:** como mudar o tema (claro ou escuro), a cor de realce, o tabuleiro e as peças, como escolher entre o menu em lista e o menu em faixa, como trocar o idioma, como organizar os painéis e onde o MyChess guarda as suas preferências.

## Janela de aparência

Escolha **Visualizar > Tema…**. A janela **Aparência** tem quatro abas na lateral e mostra uma **prévia** do que você escolhe. As escolhas ficam em rascunho: **nada muda no programa até você clicar em Aplicar**. **Cancelar** (ou `Esc`) descarta. `Enter` aplica.

Se alguma escolha foi alterada e ainda não aplicada, a aba mostra a indicação "Alterado, ainda não aplicado".

### Aba Tema

- **Modo**: **Sistema** (segue o tema do seu computador), **Claro** ou **Escuro**.
- **Cor de realce**: a cor dos botões, abas e seleções. Há nove amostras: **Verde MyChess** (o padrão), **Azul**, **Roxo**, **Rosa**, **Vermelho**, **Laranja**, **Amarelo**, **Verde** e **Cinza**. A cor selecionada fica marcada. O MyChess ajusta o contraste para o texto continuar legível em cada combinação de modo e cor.

### Aba Tabuleiro

- **Tema do tabuleiro**: as cores das casas. Os temas são **Verde** (padrão), **Azul e branco**, **Azulado**, **M. Thomas**, **Verde e amarelo**, **Marrom**, **Bege**, **Cinza**, **Rosado** e **Azul aço**.
- **Coordenadas** (as letras e os números das colunas e fileiras): **Nenhuma**, **Fora**, **Fora, nos dois lados** ou **Dentro**.

### Aba Peças

- O conjunto de peças: **Cburnett** (padrão), **Merida**, **Chessnut**, **RhosGFX**, **Fantasy**, **Spatial**, **Celtic** e **MPChess**. As peças são imagens vetoriais, então ficam nítidas em qualquer tamanho de tela.
- **Material**: o que mostrar sobre as peças capturadas. **Desligado**, **Diferença** (só a vantagem de material) ou **Peças capturadas** (as próprias peças).

### Aba Jogadores

Controla as **faixas de jogador**, as barras com o nome de cada lado, acima e abaixo do tabuleiro:

- **Mostrar as faixas**: liga e desliga. Desligadas, aparece "As faixas de jogador estão ocultas.".
- **Avatares**, **Relógios** e **Ratings**: o que cada faixa mostra.
- **Faixas compactas**: uma versão mais baixa.

## Outras opções de aparência do menu Visualizar

Estas opções ficam direto no menu **Visualizar** e valem na hora:

- **Lista de lances**: a forma de mostrar a notação (veja [O tabuleiro e a partida](02-tabuleiro-e-partida.md)).
- **Sons…**: abre as opções de som (veja [Jogar](07-jogar.md#sons)).
- **Lado a jogar**: mostra um círculo ao lado do jogador que tem a vez.
- **Destacar o último lance**.
- **Destacar lances legais**: mostra, ao selecionar uma peça, as casas para onde ela pode ir.
- **Mostrar casas coloridas e setas**: liga ou desliga as marcações desenhadas nos comentários.
- **Tempo de animação**: **Desligada** ou uma duração em milissegundos para o movimento das peças.
- **Barra de avaliação**, **Numerar as setas da engine**, **Opções da barra de avaliação** e **Setas da engine**: veja [Engines e análise](06-engines-e-analise.md).
- **Barra de status**: mostra ou esconde a barra do rodapé.

## Estilo de menu

Há duas formas de ver os comandos do programa:

- **Lista suspensa**: a barra de menus tradicional (Arquivo, Editar, Partida...). É o padrão.
- **Faixa**: uma fileira de abas (os mesmos menus) com os comandos agrupados em botões grandes com ícone, no estilo dos programas de escritório. Passe o mouse sobre um botão para ver a descrição e o atalho.

Para trocar, use **Visualizar > Estilo de menu** e escolha **Lista suspensa** ou **Faixa**. A mudança vale na hora, sem reiniciar. Na faixa, **Recolher a faixa** (ou um duplo clique na aba ativa) esconde os botões e deixa só as abas, para sobrar mais espaço; **Expandir a faixa** traz de volta. Clicar em uma aba recolhida mostra a faixa por um momento.

Os atalhos de teclado funcionam nos dois estilos. Na faixa, os botões ainda não podem ser alcançados com a tecla `Tab`.

## Idioma

**Visualizar > Idioma** oferece **Sistema** (usa o idioma do seu computador), **English** e **Português**. A mudança vale na hora.

*Estado: em desenvolvimento (só estes dois idiomas existem; o plano é trazer também os idiomas do Scid vs PC).*

## Painéis e layout

A tela é feita de painéis que você pode reorganizar (veja [Primeiros passos](01-primeiros-passos.md)):

- **Visualizar > Painéis** mostra e esconde cada painel: **Lances**, **Informações**, **Comentário**, **Análise**, **Gráfico da partida**, **Árvore de aberturas**, **Livros de abertura** e **Lista de partidas**. Outros painéis (tablebases, relatório, torneio, EPD, estatísticas) abrem por seus próprios menus.
- Cada painel tem um **×** para fechar.
- Arraste uma divisória para mudar o tamanho de um painel.
- Arraste um painel e solte-o sobre outro para juntá-los em **abas**. **Separar em um painel novo** desfaz a junção.
- **Visualizar > Restaurar layout** volta à disposição original.
- `F11` liga e desliga a tela cheia.
- `Ctrl+Shift+Seta direita` e `Ctrl+Shift+Seta esquerda` aumentam e diminuem o tabuleiro.

O MyChess lembra o layout, o tamanho da janela e as abas abertas.

*Estado: em desenvolvimento (falta guardar vários layouts com nome, os "cinco slots" do Scid vs PC, e painéis que se soltam da janela principal).*

## Preferências

O MyChess guarda as suas escolhas em um arquivo de texto chamado `preferences.toml`, na pasta de configuração do seu usuário no sistema (no Linux, dentro de `~/.config`; no Windows, dentro de `AppData`). Ele guarda o tema, o idioma, as colunas da lista de partidas, as opções da notação, as engines e muito mais. Não é preciso editá-lo à mão; o programa o atualiza sozinho.

Se o arquivo estiver danificado, o MyChess guarda uma cópia (`preferences.toml.corrupt`) e volta aos valores padrão, avisando "Não foi possível ler o arquivo de preferências; os padrões estão em uso". Se ele vier de uma versão mais nova, o aviso é "As preferências vêm de uma versão mais nova".

### Atalhos de teclado personalizados

Os atalhos podem ser trocados na seção `[shortcuts]` do `preferences.toml`, com o nome da ação e a tecla. Por exemplo: `FlipBoard = "Command+F"`. `Command` é `Ctrl` no Linux e no Windows e `Cmd` no macOS. Para desligar um atalho, deixe o valor vazio (`""`). Se a nova tecla já pertence a outra ação, o MyChess ignora a troca e avisa "A tecla de *ação* já é usada por *outra ação*". A lista dos nomes das ações e das teclas padrão está em [Atalhos de teclado](12-atalhos-de-teclado.md).

*Estado: em desenvolvimento (ainda não há uma janela de preferências para editar os atalhos; a troca é feita no arquivo).*

## Veja também

- [Primeiros passos](01-primeiros-passos.md)
- [O tabuleiro e a partida](02-tabuleiro-e-partida.md)
- [Atalhos de teclado](12-atalhos-de-teclado.md)
- [Perguntas frequentes e solução de problemas](13-perguntas-frequentes.md)

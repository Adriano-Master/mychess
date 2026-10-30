# Engines e análise

**Neste capítulo:** o que são engines, como cadastrá-las e iniciá-las, como usar o painel de análise, a barra de avaliação e as setas, como ver várias linhas (MultiPV), como anotar uma partida automaticamente, como gerar o relatório da partida e como consultar tablebases.

## O que é uma engine

Uma **engine** é um programa que calcula lances e avalia posições. O MyChess não calcula lances sozinho: ele conversa com a engine que você escolher. A mais conhecida é o **Stockfish**.

As engines falam dois "idiomas" (protocolos). O MyChess entende os dois:

- **UCI**: usado pela grande maioria das engines modernas, como o Stockfish.
- **XBoard**: um protocolo mais antigo, usado por algumas engines clássicas.

Para usar uma engine, você precisa ter o programa da engine no computador (um arquivo executável) e **cadastrá-la** no MyChess.

## Gerenciar engines

Abra **Ferramentas > Engines...** (`Ctrl+Shift+E`). A janela **Engines** mostra a lista com **Nome**, **Tecla**, **Tipo** (protocolo), **Elo** e **Último uso**.

### Adicionar uma engine

Há três caminhos:

1. **Procurar engines instaladas**: o MyChess procura engines conhecidas neste computador. Clique na que quiser em **Engines encontradas (clique para adicionar)**. Para identificá-la, o MyChess executa o programa uma vez.
2. **Novo**: preencha à mão. Os campos em negrito são obrigatórios.
   - **Nome**: como a engine aparece nas listas.
   - **Comando**: o arquivo executável da engine. Use **Procurar...** para escolher.
   - **Parâmetros**: argumentos de linha de comando, se a engine pedir algum.
   - **Pasta**: a pasta de trabalho (por padrão, a do programa).
   - **Página**: o endereço do site da engine (opcional).
   - **Elo**: a força aproximada (0 a 4000, opcional).
   - **Protocolo**: **UCI** ou **XBoard**. O botão **Detectar protocolo** executa a engine para descobrir.
   - **Tecla**: associa a engine a uma tecla de função (veja abaixo).
   - **Testar engine** executa o programa, lê o nome e as opções e mostra "OK: Stockfish (UCI, 30 opções)".
3. **Importar engines.dat do Scid...**: traz a lista de engines que você tinha no Scid vs PC. O MyChess avisa o que foi importado, o que já existia e o que não serve.

Os outros botões da janela: **Editar**, **Copiar**, **Excluir**, **Subir** e **Descer** (a ordem da lista).

Se o programa da engine não for achado, a janela avisa "O comando não foi encontrado neste computador" ou "O arquivo não pode ser executado". No Linux e no macOS, confirme que o arquivo tem permissão de execução.

### Opções da engine

Com a engine selecionada, **Opções UCI...** abre a lista de opções que a engine oferece (Hash, Threads, MultiPV e outras). Cada opção mostra o valor padrão e valida o que você digita. **Restaurar padrões** volta ao início. Os botões de ação das opções só valem com a engine em execução. Alterações valem na próxima vez que a engine iniciar: o aviso é "Reinicie a engine para aplicar a mudança".

### Opções de análise

**Opções de análise** abre as preferências comuns a todas as engines:

- **Tamanho do log (linhas, 0 = desligado)**
- **Profundidade máxima (0 = ilimitada)**
- **Incluir o nome da engine nos comentários**
- **Prioridade baixa de CPU**
- **Quebrar linhas longas**
- **Mostrar informações extras**

## Iniciar e parar uma engine

- Em **Ferramentas**, os itens **Iniciar engine 1**, **Iniciar engine 2** e **Iniciar engine 3** iniciam as engines associadas às teclas `F2`, `F3` e `F4`. Na janela **Engines**, use **Iniciar**, **Parar** e **Reiniciar**.
- Várias engines podem rodar ao mesmo tempo; cada uma ganha uma seção no painel de análise. O MyChess avisa quando há muitas ("4 engines estão em execução; isso usa muita CPU").
- `Espaço` liga ou desliga a análise da primeira engine em execução (não funciona enquanto você digita em uma caixa de texto).

## O painel de análise

Abra com **Ferramentas > Janela de análise** (`Ctrl+Shift+Y`) ou **Visualizar > Painéis > Análise**. Cada engine em execução aparece em uma seção. A engine analisa a posição do tabuleiro e se atualiza quando você muda de lance.

### Ler as linhas

Cada linha mostra a **avaliação**, a **profundidade** (quantos meios-lances a engine olhou), os **nós** (posições calculadas), o **tempo** e a **variante principal** (a sequência de lances que a engine considera melhor).

A avaliação é em **peões**: `+1,50` quer dizer que as brancas estão melhor por 1,5 peão; `-0,80` favorece as pretas; `#5` é mate em cinco lances. A engine não é infalível: os números são estimativas.

### Botões do painel

- **Iniciar engine** e **Parar engine**.
- **+ Lance**: adiciona à partida o melhor lance da engine. O botão direito adiciona a pontuação.
- **+ Linha**: adiciona a variante inteira como uma variante da partida.
- **+ Todas**: adiciona todas as linhas (ao usar MultiPV).
- **Linhas**: o número de linhas (MultiPV) que a engine mostra, para comparar alternativas.
- **Travar**: fixa a engine na posição atual, mesmo que você se mova pela partida. Aparece **Travada** e, ao clicar de novo, destrava.
- **Excluir**: impede a engine de considerar certos lances (digite-os em SAN, como `Nf3, d4`), útil para ver a segunda melhor ideia.
- **CPU baixa**: roda a engine com prioridade baixa para não travar o computador (reinicia a engine).
- **Info**: mostra informações extras (NPS, Hash, carga, tablebase).
- **Sem quebra / Com quebra / Oculto**: como as linhas aparecem.
- **Log**: mostra a conversa entre o MyChess e a engine, com **Buscar** e **Salvar como...**.
- **Setas**: mostra as jogadas da engine no tabuleiro (veja abaixo).

Nem toda engine oferece tudo. O painel avisa "Não suportado por esta engine" quando for o caso.

### Modos especiais

- **Ameaça**: passa a vez ao adversário (um lance nulo) e analisa. Mostra o que o adversário ameaça fazer. O aviso "Não há ameaça em xeque" aparece quando o seu rei está em xeque.
- **Terminar partida** (*shoot-out*): a engine joga pelos dois lados até o fim da partida, a partir da posição atual.
- **Treino**: a engine responde a cada lance que você joga, depois do tempo escolhido (0,1 a 300 segundos por lance).

Só um desses modos, a anotação automática ou o jogo automático roda por vez.

## Barra de avaliação e setas

A **barra de avaliação** é uma barra vertical ao lado do tabuleiro que mostra quem está melhor: mais branca quando as brancas estão melhor, mais preta quando as pretas estão. Ligue em **Visualizar > Barra de avaliação** (`Ctrl+Shift+S`). Em **Opções da barra de avaliação** você ajusta **Escala (peões)**, **Logarítmica**, **Marcas** e **Rótulo numérico**.

As **setas da engine** desenham no tabuleiro os lances sugeridos. O botão **Setas** de cada engine liga e desliga. Em **Setas da engine** você escolhe entre:

- **Setas**: uma seta por linha;
- **Sequência**: a variante principal desenhada como uma série de setas, para as **Sequência: os dois lados**, **lances das brancas** ou **lances das pretas** (e **Lances na sequência** define o comprimento).

**Visualizar > Numerar as setas da engine** (`Ctrl+Shift+M`) coloca um número em cada seta. **Clicar na seta joga o lance** faz o clique em uma seta jogar aquele lance.

## Várias linhas (MultiPV)

O **MultiPV** faz a engine mostrar as *N* melhores linhas em vez de uma só. Use o botão **Linhas** do painel de análise ou a opção **Número de linhas (MultiPV)** nas opções da engine. Quanto mais linhas, mais lenta fica cada uma.

## Anotar a partida automaticamente

**Ferramentas > Anotar partida...** (ou o botão **Anotar** do painel) faz a engine analisar a partida e escrever os resultados nos comentários.

1. Escolha a **Engine** (**Automática** usa a que está em execução ou a da tecla `F2`).
2. Em **Controle do lance**, escolha **Profundidade** (**Profundidade por lance**) ou **Tempo** (**Segundos por lance**). Engines XBoard só aceitam tempo.
3. **Limiar**: a queda de avaliação, em peões, a partir da qual um lance é considerado erro (padrão 1,0). **Corte**: avaliação, em peões, a partir da qual a posição já é considerada decidida e novas quedas deixam de contar como erro (padrão 5,0).
4. **Adicionar pontuação**: **Todos os lances**, **Erros/não melhor**, **Variantes** ou **Não**.
5. **Adicionar variantes**: a engine acrescenta a melhor alternativa **Quando o lance é um erro**, **Quando o lance não é o melhor**, em **Todos os lances** ou **Não**.
6. **Mates perdidos/mais curtos**: avisa quando existia um mate mais rápido.
7. **Qual lado**: **Ambos**, **Brancas** ou **Pretas**.
8. **Formato da pontuação**: `+1.5`, `[% +1.5]` ou `[%eval +1.5]`. O último é o padrão usado por outros programas, e o relatório da partida lê.
9. **Usar livro**: pula os lances que estão no livro de abertura (escolha o livro em **Ferramentas > Livros de abertura**). **Só erros de abertura** e **Lances da abertura** limitam a análise ao começo.
10. Opções extras: **Acrescentar tag Annotator**, **Processar variantes** e **Marcar exercícios táticos** (marca partidas com um erro grave como exercício para o [treino de táticas](08-treinar.md)).
11. **Configurar valores informantes...** ajusta os limites usados para atribuir os símbolos `!?`, `?`, `??`, `?!`, `+=`, `+/-`, `+-` e `++-`.
12. Clique em **Iniciar**. O painel mostra "Anotando... lance 12 de 40" e uma estimativa de tempo.

**Parar** (ou `Esc`) interrompe a anotação e **mantém** as edições feitas até ali. Enquanto anota, a partida não pode ser editada. Não é possível anotar no modo tentativa.

### Anotar todas as partidas do filtro

Marque **Anotar todas as partidas do filtro (grava no banco)**. O MyChess anota cada partida e grava na base. Exige uma base SI4 editável no slot atual e que a partida aberta não tenha alterações pendentes ("Salve ou descarte a partida atual antes de anotar em lote.").

## Relatório da partida

O **relatório da partida** analisa a partida inteira e avalia a qualidade de cada lance, como nos sites de xadrez. Abra com **Ferramentas > Relatório da partida**.

1. Clique em **Gerar relatório**. Antes, **Configurar...** permite escolher a engine, a profundidade ou o tempo (as predefinições **Rápido**, **Padrão** e **Profundo**) e os limites das classes.
2. Acompanhe o progresso ("Analisando... lance 12 de 40, faltam cerca de 3 min"). A partida não pode ser editada nesse tempo.
3. O resultado mostra a **Precisão** de cada lado e a classe de cada lance.

As classes de lance são:

- **Brilhante**, **Excelente**, **Melhor**, **Ótimo**, **Bom**: lances que vão de muito bons a ótimos;
- **Livro**: lance conhecido da teoria;
- **Imprecisão**, **Erro**, **Capivarada**: lances que perdem pontos de chance de vitória, do mais leve ao mais pesado;
- **Chance perdida**: o lance deixou escapar uma boa oportunidade.

As classes e a precisão são **estimativas** feitas a partir de modelos públicos e **não coincidem com as de nenhum site**.

### Rever os erros

A lista **Lances para rever** mostra os momentos críticos. Escolha de quem quer rever (**Rever os lances de:** com **Os dois**, **Brancas** ou **Pretas**; **Joguei de:** define o seu lado). Em cada momento:

- O tabuleiro vai até a posição e mostra o lance que foi jogado.
- O MyChess pede "Ache no tabuleiro um lance melhor que ...". Jogue o que achar melhor.
- Se errar, aparece uma dica. **Tentar de novo** recomeça e **Ver o melhor lance** revela a resposta (com uma seta verde, se **Mostrar a seta do melhor lance** estiver ligado).
- **Anterior**, **Próximo** e as teclas `Seta esquerda` e `Seta direita` passam entre os momentos. **Sair** encerra a revisão.

Outras ações:

- **Gravar na partida**: escreve as avaliações, as marcas e a melhor linha de cada falha na própria partida (pode ser desfeito).
- **Usar as avaliações da partida**: monta o relatório sem engine, com os comentários `[%eval]` que a partida já tem. Nesse caso o melhor lance não é conhecido.
- **Continuar análise**: se você interrompeu, analisa só o que faltou.
- **Analisar de novo** e **Limpar**.

## Tablebases

Uma **tablebase** é um banco de dados com o resultado exato de todas as posições de finais com poucas peças. O MyChess usa as tablebases **Syzygy**.

Abra com **Ferramentas > Tablebases** (`Ctrl+Shift+F8`). O painel mostra, para cada lance da posição atual, o **Resultado** (**Vitória**, **Empate**, **Derrota**, e também **Vitória amaldiçoada** e **Derrota abençoada**, que a regra dos 50 lances transforma em empate) e a **DTZ**, a distância até o próximo "zeramento" (captura ou lance de peão). A DTZ não é o número de lances até o mate. O melhor lance vem marcado.

Há duas fontes, que você liga em **Configurações**:

- **Tablebases locais (arquivos Syzygy)**: arquivos `.rtbw` e `.rtbz` no seu computador. Os arquivos **não vêm** com o MyChess. Baixe-os em `tablebase.lichess.ovh/tables` (as de 3, 4 e 5 peças ocupam cerca de 1 GB; as de 6 peças, cerca de 150 GB). Adicione a pasta em **Pastas com arquivos .rtbw e .rtbz**. A extensão precisa estar em letras minúsculas.
- **Tablebase online (Lichess)**: consulta pela internet. Ao ligar, o MyChess envia a posição (FEN) ao site tablebase.lichess.ovh. Nada é enviado enquanto a opção está desligada.

O painel avisa quando não consegue consultar: "N peças: a tablebase cobre até M", "Ainda há direito de roque: as tablebases não cobrem essa posição", "A tabela KRvKP não está disponível".

**Treinar** joga a posição atual contra a tablebase, com os resultados ocultos. Veja [Treinar](08-treinar.md#treino-de-finais).

As tablebases ficam indisponíveis durante uma partida online, para não dar vantagem.

## Limites

- O MyChess **não tem** gráfico de pontuação (avaliação) nem semáforo MultiPV. O relatório da partida e a barra de avaliação cobrem o mesmo propósito.
- A tablebase Syzygy não informa a distância ao mate (DTM). Só a fonte online dá esse valor, e só para algumas posições.
- Durante uma partida online (Lichess ou FICS), engines e ferramentas de ajuda ficam bloqueadas.

## Veja também

- [Aberturas](05-aberturas.md)
- [Jogar](07-jogar.md)
- [Treinar](08-treinar.md)
- [Atalhos de teclado](12-atalhos-de-teclado.md)
- [Perguntas frequentes e solução de problemas](13-perguntas-frequentes.md)

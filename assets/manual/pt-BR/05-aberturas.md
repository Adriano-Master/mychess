# Aberturas

**Neste capítulo:** como estudar aberturas com a árvore de aberturas, com os livros de abertura (Polyglot), com a classificação ECO e com as máscaras da árvore. No fim, uma visão rápida do treinador de aberturas.

## A árvore de aberturas

A **árvore de aberturas** mostra, para a posição atual do tabuleiro, **todos os lances que já foram jogados nela** nas partidas da base aberta. Para cada lance ela informa quantas partidas o escolheram e como elas terminaram. É a ferramenta mais usada para estudar aberturas.

### Abrir a árvore

Use **Ferramentas > Árvore de aberturas** (`Ctrl+Shift+T`) ou **Visualizar > Painéis > Árvore de aberturas**. A árvore age sobre a base do slot atual. Cada vez que você muda a posição, a tabela se atualiza.

### Ler a tabela

Cada linha é um lance. As colunas são:

- **Lance**: o lance em notação.
- **Partidas**: quantas partidas jogaram esse lance, com uma barra de três cores (vitórias das brancas, empates e vitórias das pretas).
- **Score**: o aproveitamento das brancas, em porcentagem (vitória vale 1, empate vale 0,5).
- **Empates**: porcentagem de empates.
- **Elo**: média dos Elos dos jogadores nessas partidas.
- **Perf**: o desempenho (performance) médio, estimado a partir do Elo dos adversários.
- **Ano**: o ano médio das partidas.
- **ECO**: o código da abertura.

A linha **TOTAL** soma tudo.

### Usar a árvore

- **Clique em um lance** para jogá-lo no tabuleiro e ver o próximo nível da árvore.
- **Ordenar**: por **Alfabética**, **Código ECO**, **Frequência** ou **Score**.
- **Busca**: **Rápida** responde na hora, com valores aproximados; **Lenta** examina todas as partidas e dá o resultado exato; **Rápida, depois lenta** mostra primeiro a rápida e depois a exata.
- **Atualizar**: refaz a tabela. A árvore também se atualiza sozinha quando a posição muda.
- **Ajustar filtro**: reduz o filtro às partidas que passaram pela posição atual. Assim a lista de partidas mostra só elas.
- **Exibição curta**: deixa a tabela mais compacta.
- **Travar**: mantém a árvore na base atual mesmo que você mude de slot.
- **Abrir base como árvore**: usa outra base como fonte da árvore, sem trocar o slot.

Se uma posição tem muitos lances (mais de 60), o MyChess avisa. Se nenhuma partida chega à posição, a tabela diz "Nenhuma partida chega a esta posição".

### Melhores partidas

A seção **Melhores partidas** lista partidas que passaram pela posição. Escolha o **Resultado** (**Todos**, **1-0**, **0-1**, **1-0 e 0-1**, **Empates**, **Sem resultado**), a **Ordem** (**Melhores (Elo)** ou **Ordem da base**) e quantas **Partidas** mostrar. Os botões **Carregar**, **Navegar** e **Copiar** agem sobre a partida escolhida.

### Treino

O botão **Treino** transforma a árvore em adversária: depois de cada lance seu, ela responde com um lance sorteado entre as partidas da base, em proporção à frequência. É uma forma de ver como as aberturas costumam seguir.

### Cache da árvore

Para bases grandes, o MyChess guarda um resumo ("cache") das árvores já calculadas num arquivo `.stc` ao lado da base, compatível com o do Scid. O menu **Cache** da árvore oferece:

- **Salvar cache** e **Informações do cache**;
- **Preencher com a base**, **Preencher com a partida atual** e **Preencher com as linhas padrão**, que calculam antes as árvores mais prováveis;
- **Salvar o cache automaticamente**, que grava ao fechar a árvore e ao sair do programa.

Só bases SI4 têm arquivo de cache. O MyChess descarta o cache sozinho quando a base é alterada.

## Máscaras da árvore

Uma **máscara** é um arquivo que guarda, para cada posição, a sua opinião sobre os lances: cores, símbolos de anotação (NAGs), marcadores e comentários. Ela aparece na árvore ao lado das estatísticas. Serve para montar e revisar um repertório de aberturas. O MyChess usa o formato `.stm` do Scid vs PC, então máscaras do Scid funcionam.

O botão **Máscara** da árvore oferece:

- **Nova**, **Abrir...**, **Abrir recente**, **Salvar**, **Salvar como...** e **Fechar**;
- **Exibir máscara**: mostra a máscara inteira como uma lista que você expande e recolhe;
- **Buscar na máscara**: acha posições por NAG, marcador, cor ou texto de comentário;
- **Preencher com a linha**, **Preencher com a partida** e **Preencher com o filtro**: acrescentam lances à máscara a partir de uma linha, da partida atual ou das partidas do filtro;
- **Mostrar marcadores**, **Mostrar dicas** e **Carregar a última máscara ao abrir a árvore**.

Clique com o botão direito em um lance da árvore para **Adicionar à máscara**, **Remover da máscara**, **Comentar o lance**, **Comentar a posição** e escolher **Marcador 1**, **Marcador 2**, **Cor** ou **NAG**. Os marcadores disponíveis incluem **Incluir**, **Excluir**, **Linha principal**, **Marcador**, **A verificar**, **Para treinar**, **Duvidosa** e **Para remover**.

As máscaras valem só para o xadrez padrão. A máscara que tiver alterações pergunta se deve ser salva antes de fechar.

## Livros de abertura (Polyglot)

Um **livro de abertura** é um arquivo que diz, para cada posição, quais lances são bons e com que frequência jogá-los. O MyChess usa o formato **Polyglot** (arquivos `.bin`), que é o padrão de fato: quase toda engine e todo programa de xadrez lê esse formato.

### Abrir a janela dos livros

Use **Ferramentas > Livros de abertura** (`Ctrl+Shift+K`). O painel pode mostrar **dois livros** lado a lado (**Livro 1** e **Livro 2**); o botão **Comparar** abre o segundo.

Para cada livro:

1. Clique em **Abrir livro...** e escolha um arquivo `.bin`, ou em **Novo livro** para começar um vazio.
2. A tabela mostra, para a posição do tabuleiro, os **Lances** do livro, o **Peso** de cada um (de 1 a 65520) e a porcentagem (%). Quanto maior o peso, mais o livro recomenda o lance.
3. Se a partida em exibição joga um lance que o livro não tem, ele aparece em **Outros lances**. Clique nele para adicioná-lo ao livro com peso 1.

Um livro Polyglot só guarda xadrez padrão. Ele não aceita Chess960 nem posições montadas à mão.

### Editar um livro

Abra as ferramentas com o botão **Mostrar as ferramentas**. Elas servem para:

- **Ajustar pesos**: digite um peso e aperte `Enter`; **x2** dobra, **/2** divide por dois e o botão de remover tira o lance do livro.
- **Adicionar lance**: acrescenta o lance que a partida joga em seguida, com peso 1.
- **Adicionar linha** e **Remover linha**: acrescentam ou removem a linha principal da partida a partir da posição atual.
- **Importar ramo**: acrescenta todas as linhas da partida a partir da posição, com as variantes.
- **Copiar do outro**: troca os lances desta posição pelos do outro livro.
- **Salvar livro** e **Salvar livro como...**: gravam as alterações. O livro mostra **Alterado** enquanto houver alterações não salvas, e o MyChess pergunta ao fechar o programa.

Limites do formato: uma posição aceita no máximo 100 lances; o peso não pode ser zero.

### Criar um livro a partir de uma base

Em **Criar a partir do filtro**, o MyChess monta um livro com as partidas do filtro da base atual. Os campos são:

- **Meios-lances por partida**: até que profundidade ler cada partida.
- **Mínimo de partidas**: um lance só entra se aparecer pelo menos esse número de vezes.
- **Pontuação mínima (%)**: descarta lances com aproveitamento baixo.
- **Só Brancas** e **Só Pretas**: guarda só os lances de um lado.
- **Mesmo peso**: dá o mesmo peso a todos os lances.

Escolha **No livro 1** ou **No livro 2** e clique em **Criar livro**. **Parar** interrompe e nada é guardado.

### Mesclar livros

Em **Mesclar livros**, escolha **Manter o livro 1** ou **Somar os pesos** e clique em **Mesclar no livro 1**.

### Exportar um livro para PGN

**Exportar ramo para PGN…** grava em um arquivo as linhas do livro que partem da posição atual, uma partida por linha, com os lances de maior peso primeiro. Você escolhe os **Meios-lances por linha**, o **Máximo de linhas** e o **Peso mínimo**.

### Usar um livro como livro de abertura

**Usar como livro de abertura** marca o livro para dois fins: a [anotação automática e o relatório da partida](06-engines-e-analise.md) pulam os lances que estão no livro, e o jogo contra a engine pode usá-lo. Veja também [Jogar](07-jogar.md).

## ECO e classificação

O **ECO** (Encyclopaedia of Chess Openings) é um sistema de códigos que dá a cada abertura um nome curto, de `A00` a `E99`. Por exemplo, `B12` é a Defesa Caro-Kann. Os códigos podem ter uma letra extra para mais detalhe (`B12j`).

Para o MyChess entender os códigos, ele precisa de um arquivo ECO (o `scid.eco` que vem com o Scid vs PC). O MyChess não traz esse arquivo. Carregue o seu no **Navegador ECO** (botão **Carregar arquivo ECO...**); o programa lembra o caminho.

### Identificar a abertura da partida

**Partida > Identificar abertura** diz qual é a abertura da partida atual, por exemplo "Abertura B12: Caro-Kann (depois do meio-lance 4)". Se nenhuma posição da partida está no arquivo, o aviso é "Nenhuma posição desta partida está no arquivo ECO".

### Navegador ECO

**Ferramentas > Navegador ECO...** mostra, para cada código ou seção, quantas partidas da base têm essa abertura e como elas terminaram (barras de vitórias, empates e derrotas).

- Clique em uma seção (A, B, C, D, E) para ver os códigos dentro dela, e em **Subir** para voltar.
- **Procurar**: ache uma abertura pelo nome ou pelo código.
- **Todas as partidas** ou **Só o filtro**: escolhe se as estatísticas contam a base inteira ou só o filtro.
- Clique em uma abertura da lista para **importar a linha** para o tabuleiro.

### Classificar as partidas de uma base

**Banco > Classificar partidas por ECO...** grava o código ECO em partidas da base.

1. Em **Partidas a classificar**, escolha: **Só as partidas que ainda não têm código ECO**, **Todas as partidas (substitui os códigos antigos)**, **Só as partidas do filtro**, **Todas as partidas jogadas no último ano**, **no último mês** ou **desde** uma data (`AAAA.MM.DD`).
2. Em **Códigos ECO a usar**, escolha **Só códigos básicos ("B12")** ou **Extensões ("B12j")**.
3. Clique em **Prévia** para ver quantas partidas mudariam, sem gravar.
4. Clique em **Classificar** para gravar.

Só bases SI4 editáveis podem ser gravadas. Em um PGN ou na Clipbase, apenas a prévia é possível.

### Encontrar novidade

**Partida > Encontrar novidade...** acha o primeiro lance da partida atual que **nenhuma outra partida** da base escolhida jogou. Serve para ver onde uma partida sai da teoria conhecida.

1. Escolha a **Base** de referência.
2. Marque **Só partidas mais antigas** para comparar apenas com partidas anteriores à data da partida.
3. Clique em **Encontrar novidade**.

O resultado é "Novidade: 12.Nxe5" ou "Nenhuma novidade foi encontrada nesta partida". Partidas que começam de uma posição montada à mão não têm busca de novidade.

## Treinador de aberturas

O **treinador de aberturas** sorteia linhas do seu repertório para você repetir de memória. Ele fica no menu **Jogar > Treinador de aberturas...** (`Ctrl+Shift+F6`) e é descrito no capítulo [Treinar](08-treinar.md#treinador-de-aberturas).

## Veja também

- [Bancos de dados](03-bancos-de-dados.md)
- [Buscas](04-buscas.md)
- [Engines e análise](06-engines-e-analise.md)
- [Treinar](08-treinar.md)
- [Glossário](14-glossario.md)

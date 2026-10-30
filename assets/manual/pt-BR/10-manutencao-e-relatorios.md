# Manutenção e relatórios

**Neste capítulo:** as ferramentas para cuidar de uma base (compactar, ordenar, achar duplicatas), os relatórios de abertura e de jogador, o trabalho com arquivos EPD, a mesclagem de partidas e a exportação em outros formatos.

*Estado deste capítulo: em desenvolvimento. Os itens abaixo estão em fases diferentes, e cada seção diz em que ponto está. Onde ainda não há janela, o capítulo mostra o que usar enquanto isso.*

## Manutenção de bases

*Estado: a janela de manutenção abre pelo menu **Banco > Manutenção...**, e as tarefas (**Compactar...**, **Ordenar...**, **Procurar duplicatas...**, **Limpeza geral...**, **Verificar partidas...**, **Reparar banco...**, **Tipo do banco...**, **Descrição...**, **Flags personalizadas...**, **Tags extras...** e **Editor de nomes...**) também têm item próprio nesse menu. As tarefas principais continuam disponíveis no programa de linha de comando `mychess-cli`.*

A **manutenção** reúne as tarefas que mantêm uma base saudável:

- **Compactar** a base: remover de vez as partidas marcadas como apagadas e os nomes sem uso, diminuindo os arquivos.
- **Ordenar** as partidas por até 15 critérios (data, evento, local, rodada, brancas, pretas, ECO, resultado, tamanho, rating e outros).
- **Achar partidas duplicadas** (mesmos jogadores, data, resultado e lances).
- **Verificar** se as partidas da base estão legíveis.
- **Editar nomes em lote**, **marcar partidas em lote**, **remover comentários e variantes** de várias partidas, **editar tags extras**, mudar o **tipo da base** e a descrição.

Para quem prefere o terminal, estas tarefas também funcionam no programa de linha de comando. Para usá-lo, abra um terminal (no Windows, o Prompt de Comando ou o PowerShell) e digite os comandos. `BASE` é o nome da base com ou sem a extensão (`minhabase` ou `minhabase.si4`).

- `mychess-cli info BASE`: mostra a descrição, o número de partidas e de nomes. Com `--compact`, mostra quanto a base diminuiria se fosse compactada.
- `mychess-cli compact BASE`: compacta as partidas (remove as apagadas). Com `--names`, remove os nomes sem uso.
- `mychess-cli sort CAMPOS BASE`: ordena. Por exemplo, `mychess-cli sort date,white minhabase`. Com `-r`, a ordem é invertida.
- `mychess-cli describe BASE "Texto"`: troca a descrição da base.
- `mychess-cli names BASE player Kasp`: lista os nomes de jogadores que começam com `Kasp`.
- `mychess-cli import arquivo.pgn BASE`: cria uma base a partir de um PGN.
- `mychess-cli merge NOVA ORIGEM1 ORIGEM2...`: junta várias bases em uma nova.
- `mychess-cli export BASE arquivo.pgn`: grava todas as partidas em PGN.

Dicas:

- **Faça sempre uma cópia da base antes de compactar ou ordenar.** O MyChess faz as duas operações com segurança (se algo falhar no meio, a base volta como estava), mas uma cópia protege contra outros problemas.
- Compactar e ordenar não funcionam com a base aberta para escrita em outro programa.
- O `mychess-cli` é um programa separado do MyChess; ele é compilado junto com o código-fonte e deve acompanhar os pacotes de instalação quando eles existirem.

Mais sobre apagar partidas está em [Bancos de dados](03-bancos-de-dados.md#partidas-apagadas).

### Classificar partidas por ECO

*Estado: pronto no menu **Banco > Classificar partidas por ECO...**; descrito em [Aberturas](05-aberturas.md).*

## Relatório de abertura

*Estado: em desenvolvimento (a janela existe; falta o teste manual completo e a revisão final).*

O **relatório de abertura** reúne tudo o que a base sabe sobre uma posição: quantas partidas chegam a ela, quem as joga, como terminam, os temas típicos, as ordens de lances e uma tabela teórica com as principais continuações. Abra com **Estatísticas > Relatório de abertura...**. O relatório parte da posição atual do tabuleiro.

### Ler o relatório

A janela mostra o relatório em seções numeradas:

- **Estatísticas e histórico**: o resultado (pontuação) das brancas em todas as partidas do relatório, nas de rating alto dos dois lados e nas mais recentes; as partidas mais antigas e mais recentes; a **Popularidade atual** (a frequência em todos os anos, nos últimos 10, 5 e 1 ano, e se está subindo ou caindo); os **Jogadores mais frequentes**.
- **Ratings e desempenho**: rating médio e desempenho de cada lado, e as partidas de maior rating médio.
- **Tendências de resultado**: comprimento das partidas, frequência dos resultados e as **Vitórias mais curtas**.
- **Lances e temas**: as **Ordens de lances** que chegam à posição, os lances seguintes e os **Temas posicionais** (por exemplo, **Roques opostos**, **Damas trocadas**, **Peão isolado da dama**).
- **Finais**: o material que sobra no fim das partidas.
- **Partidas do relatório**.
- **Tabela teórica**: as continuações principais, com notas numeradas. Clique em uma nota ("Nota [3]") para selecionar as partidas daquela linha.

Clique com o botão direito em uma linha da tabela para **Copiar linha** (os lances). Os números e as partidas clicáveis definem o filtro da base, e o MyChess avisa "O filtro agora tem as partidas escolhidas". Se a base mudou depois de gerar o relatório, aparece "O relatório está desatualizado": clique em **Atualizar**.

### Botões

- **Opções**: escolha o que entra no relatório (**Estatísticas**, **Popularidade**, **Rating médio e desempenho**, **Resultados**, **Lances a partir da posição**, **Temas posicionais**, **Finais**, **Ordens de lances** e outros) e o **Máximo de partidas na tabela teórica**. **Padrões** restaura.
- **Excluir**: tira da tabela teórica as partidas que jogam um lance escolhido.
- **Buscar no relatório**: acha um texto e conta as ocorrências.
- **Mesclar partidas**: junta as melhores partidas do relatório (até 50) em uma só partida, com variantes.
- **Salvar**: grava o relatório. Em **Parte do relatório**, escolha **Relatório completo**, **Relatório compacto** ou **Só a tabela teórica**. Em **Formato**, **Texto**, **HTML** ou **LaTeX**. O MyChess não grava por cima de uma base aberta ("Esse arquivo é um banco aberto; escolha outro nome").
- **Ver HTML**: abre o relatório no navegador. **Ver LaTeX** exige um programa externo e ainda não está disponível.
- **Favoritos**: guarda posições de abertura para gerar relatórios depois. Dê um **Nome desta posição** e **Adicionar esta posição**; reordene com **Subir** e **Descer**; **Gerar relatórios...** produz todos de uma vez, em uma pasta que você escolhe; **Importar reports.dat...** traz os favoritos do Scid vs PC.
- **Parar**: interrompe a geração. O relatório fica parcial ("O relatório foi interrompido").

A geração tem duas fases, mostradas na janela: "1. Procurando no banco as partidas do relatório" e "2. Gerando as informações do relatório".

## Relatório de jogador

*Estado: previsto. O motor do relatório já está pronto, mas ainda não há janela nem item de menu.*

O **relatório de jogador** será parecido com o de abertura, mas do ponto de vista de uma pessoa: estatísticas, adversários frequentes, rating e desempenho, aberturas por grupo ECO, temas, finais e uma tabela teórica das aberturas que ela joga. A janela ainda não pode ser aberta.

Enquanto isso, veja as **Informações do jogador** em [Jogadores e torneios](09-jogadores-e-torneios.md).

## Arquivos EPD

*Estado: em desenvolvimento (a janela funciona; falta o teste manual completo e a revisão final).*

Um arquivo **EPD** (Extended Position Description) é uma lista de posições, uma por linha, cada uma com **opcodes**: marcas como `bm Qxh7+` (melhor lance), `ce 35` (avaliação em centipeões) ou `id "posição 12"`. É o formato usado por coleções de problemas e por testes de engines.

### Abrir e criar

- **Arquivo > EPD > Abrir arquivo EPD...**, **Novo arquivo EPD...**, **Salvar arquivo EPD**, **Salvar arquivo EPD como...** e **Arquivos EPD recentes**.
- Ou abra o painel por **Ferramentas > Arquivos EPD** (grupo **Anotação e EPD**).

Você pode ter até 4 arquivos EPD abertos ao mesmo tempo, cada um em uma aba. Linhas que o MyChess não consegue ler são ignoradas, e o aviso "N linhas foram ignoradas" leva a um relatório (**Linhas ignoradas**).

### Trabalhar com as posições

- **Primeira posição**, **Posição anterior**, **Próxima posição**, **Última posição** (`Ctrl+Shift+Home`, `Ctrl+Shift+Seta para cima`, `Ctrl+Shift+Seta para baixo`, `Ctrl+Shift+End`) e **Posição aleatória** levam o tabuleiro à posição do registro.
- Edite os opcodes da posição na caixa **Opcodes da posição**: um opcode por linha.
- **Adicionar posição** cria um registro com a posição do tabuleiro e **Remover posição** o apaga (com confirmação).
- **Procurar** acha texto nas posições, e a caixa **Mostrar só as posições que contêm...** filtra.
- **Ordem**: **Arquivo**, **Posição**, `id` ou `ce`.
- **Colar análise**, **Ordenar opcodes**, **Remover opcodes** (de todas as posições) e **Achar a posição mais profunda da partida** completam as ferramentas.

### Analisar com uma engine

**Analisar posições** pede a uma engine UCI que analise as posições do arquivo. Escolha:

- a **Engine** e os **Segundos por posição** (de 1 a 300);
- o **Modo**: **Contar melhores lances** (confere quantas posições têm o `bm` achado pela engine), **Anotar** (grava o resultado nos opcodes) ou **Ambos**;
- as **Posições**: **Todas**, **A partir da selecionada** ou **Só as sem ce**.

O andamento aparece na janela. **Parar a análise** interrompe. **Desfazer análise** restaura o arquivo como estava antes da última análise.

O arquivo EPD só é gravado quando você escolhe **Salvar**. Ao fechar uma aba alterada, o MyChess pergunta se deseja salvar.

## Mesclar partidas

*Estado: em desenvolvimento (a janela e os menus existem; faltam as entradas no navegador de partidas, nas melhores partidas da árvore e nas tabelas de torneio, além do teste manual).*

**Mesclar** acrescenta à partida atual, como variantes, os lances de outra partida que passa pela mesma posição. Serve para juntar várias partidas de uma abertura em um só estudo.

1. Selecione uma partida na lista de partidas.
2. Escolha **Editar > Mesclar na partida atual...**, ou o mesmo comando no menu de contexto da lista.
3. A janela **Prévia da mesclagem** mostra a partida resultante, com os lances acrescentados em destaque. Em **Até o lance (da partida mesclada)**, escolha **Todos** ou um número, para mesclar só o começo.
4. Clique em **Mesclar**.

Com várias partidas selecionadas, o MyChess pergunta "Mesclar N partidas na partida atual?" e informa quantas foram mescladas e quantas ignoradas, com o motivo de cada tipo: sem posição coincidente, partida que termina na posição coincidente, a própria partida atual ou partida ilegível.

Não é possível mesclar no modo tentativa.

## Exportação

*Estado: em desenvolvimento. A exportação em PGN está pronta; HTML, LaTeX, EPD, texto da lista e imagem do tabuleiro ainda não têm janela.*

Hoje você exporta partidas em **PGN** com **Arquivo > Exportar partidas...** (veja [Bancos de dados](03-bancos-de-dados.md#exportar-partidas)). Para os relatórios, a janela de relatório salva em texto, HTML e LaTeX (veja acima). A tabela de torneio salva em texto, HTML e LaTeX (veja [Jogadores e torneios](09-jogadores-e-torneios.md)).

Estão previstos, em uma janela única de exportação:

- partidas em **HTML**, com um visualizador de partidas que roda no navegador;
- partidas em **LaTeX**;
- o filtro em **EPD**;
- o texto da lista de partidas (e CSV);
- a imagem do tabuleiro (**PNG** ou **SVG**).

## Veja também

- [Bancos de dados](03-bancos-de-dados.md)
- [Aberturas](05-aberturas.md)
- [Jogadores e torneios](09-jogadores-e-torneios.md)
- [Perguntas frequentes e solução de problemas](13-perguntas-frequentes.md)

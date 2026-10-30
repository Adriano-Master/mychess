# Bancos de dados

**Neste capítulo:** como abrir, criar e fechar bases de dados, como funcionam os espaços de base (os "slots") e a Clipbase, como usar a lista de partidas, o filtro, as colunas e a ordenação, como guardar marcadores, importar e exportar PGN e copiar partidas entre bases.

## O que é uma base de dados

Uma **base de dados** (ou **banco**) guarda muitas partidas e permite achá-las depressa. O MyChess usa o formato **SI4** do Scid vs PC. Uma base SI4 é feita de **três arquivos** que andam juntos:

- `nome.si4`: o índice, com os dados principais de cada partida;
- `nome.sn4`: os nomes (jogadores, eventos, locais);
- `nome.sg4`: os lances e comentários de cada partida.

Se você copiar ou mover uma base, leve os três arquivos. Você pode abrir qualquer um dos três: o MyChess encontra os outros.

Bases do Scid vs PC funcionam sem conversão. Bases no formato antigo SI3 precisam ser atualizadas para SI4 antes; o MyChess avisa "Este banco está no formato antigo SI3; atualize-o para SI4 primeiro".

## Os slots e a Clipbase

O MyChess pode ter até **oito bases abertas ao mesmo tempo**, cada uma em um **slot** (um espaço numerado de 1 a 8). Há também um nono espaço, a **Clipbase**.

A **Clipbase** é uma base temporária que existe só na memória. Ela serve como área de transferência de partidas: você junta nela partidas de várias bases e depois trabalha com elas. A Clipbase guarda até 5 milhões de partidas e não é salva em disco. Ela nunca é fechada, só esvaziada.

Para trocar de base:

- `Ctrl+1` a `Ctrl+8`: vai para o slot de mesmo número (só os que estão em uso).
- `Ctrl+9` ou `` Ctrl+` ``: vai para a Clipbase.
- `Ctrl+Tab` e `Ctrl+Shift+Tab` (ou `Ctrl+Page Down` e `Ctrl+Page Up`): passam pelas bases abertas, em círculo.
- **Banco > Bancos abertos**: lista os slots e a Clipbase; clique na base desejada. O comando também tem **Próximo banco** e **Banco anterior**.

A lista de partidas mostra uma aba por base aberta, com o nome, o aviso "somente leitura" quando for o caso e o contador de partidas no filtro (por exemplo, `120/5000`). Cada base lembra a sua própria partida atual, filtro, ordenação e seleção quando você troca de aba.

## Abrir uma base

1. Escolha **Arquivo > Abrir...** (`Ctrl+O`).
2. Selecione um arquivo `.si4` ou um arquivo PGN (`.pgn`, `.pgn.gz`) e clique em **Abrir**.
3. O MyChess mostra o progresso ("Abrindo nome...") e coloca a base no próximo slot livre.

Se todos os oito slots estiverem ocupados, o MyChess avisa "Há bancos demais abertos; feche um antes".

Bases abertas recentemente ficam em **Arquivo > Abrir recente**. O item **Limpar lista** esvazia a lista.

Quando uma base tem uma operação inacabada de uma sessão anterior (por exemplo, o programa foi fechado no meio de uma gravação), o MyChess diz "O banco tem uma operação inacabada e precisa ser reparado antes".

### Abrir um arquivo PGN como base

Um arquivo `.pgn` ou `.pgn.gz` (PGN compactado) abre como uma base **somente leitura**, carregada na memória. Você pode pesquisá-la, abrir partidas e copiá-las para outras bases, mas não pode alterá-la. Limites: até 5 milhões de partidas e 2 GiB de partidas na memória. Acima disso o MyChess avisa "O arquivo tem mais partidas do que se pode abrir" ou "As partidas ocupam memória demais".

Para editar um PGN grande, importe-o para uma base SI4 (veja abaixo).

## Criar uma base

*Estado: em desenvolvimento (falta um comando "Novo banco" no menu; hoje a base nova nasce da importação de PGN).*

A forma de criar uma base nova hoje é **importar um arquivo PGN** (veja a seção seguinte) quando o slot atual não aceita partidas. O MyChess pede o nome da nova base ("Nome da nova base"), cria os três arquivos `.si4`, `.sn4` e `.sg4` e abre a base. O MyChess nunca sobrescreve uma base existente: se o nome já existir, ele avisa "Já existe um banco com esse nome; nada foi criado".

## Importar arquivos PGN

Use **Arquivo > Importar arquivo PGN...**.

1. Selecione um ou mais arquivos `.pgn` ou `.pgn.gz`.
2. Se o slot atual tem uma base SI4 editável, as partidas entram **no fim dela**. Se não, o MyChess pede o nome de uma base nova.
3. A janela **Importando arquivos PGN** mostra o andamento ("Arquivo 1 de 3 - 500 partidas gravadas - 40%"). O botão **Parar** interrompe: as partidas já gravadas ficam.
4. No fim aparece um resumo: partidas encontradas, importadas, não importadas e avisos. A janela lista os primeiros problemas, com o arquivo, o número da partida e a linha. Se foi um arquivo só e não houve problema, a janela fecha sozinha.

Dicas:

- Importar o mesmo arquivo duas vezes **duplica** as partidas. O MyChess não confere repetições na importação (veja [Manutenção e relatórios](10-manutencao-e-relatorios.md) para a busca de duplicatas).
- Partidas de variantes de xadrez sem suporte e partidas grandes demais são ignoradas, e o relatório informa.
- Textos fora das partidas são ignorados.

## Exportar partidas

Use **Arquivo > Exportar partidas...** para gravar partidas em um arquivo PGN.

1. Em **O que exportar**, escolha: **Todas as partidas da base**, **As partidas do filtro**, **As partidas selecionadas** ou **A partida atual**.
2. Marque as opções: **Incluir comentários**, **Incluir variantes**, **Incluir comandos como [%eval] e [%clk]** e, se quiser, **Somente partidas com a bandeira Tática (T)**.
3. Marque **Acrescentar ao fim do arquivo** para adicionar ao final de um PGN que já existe, em vez de substituí-lo.
4. Clique em **Escolher destino...** e dê o nome do arquivo.

O MyChess grava primeiro em um arquivo temporário e só então troca pelo arquivo final. Se você clicar em **Parar**, o arquivo original fica como estava. O MyChess recusa exportar para o próprio arquivo de uma base aberta.

Para exportar em HTML, LaTeX ou EPD, veja [Manutenção e relatórios](10-manutencao-e-relatorios.md).

## Salvar a partida atual

- **Salvar** (`Ctrl+S`) e **Salvar como...** gravam a partida atual em um arquivo PGN.
- **Anexar a um arquivo PGN...** acrescenta a partida ao fim de um PGN existente.

*Estado: em desenvolvimento (falta gravar a partida editada de volta na base SI4, como os comandos "Adicionar partida" e "Substituir partida" do Scid).* Por enquanto, para levar uma partida editada para uma base SI4, salve-a como PGN e importe o arquivo.

## A lista de partidas

A lista mostra as partidas do **filtro** da base atual. Abra-a com `Ctrl+L` ou **Visualizar > Painéis > Lista de partidas**. Só as linhas visíveis são carregadas, por isso a lista é rápida mesmo com milhões de partidas.

- **Clique duas vezes** (ou `Enter`) em uma linha: carrega a partida no tabuleiro. Se a partida atual tem alterações, o MyChess pergunta se quer salvá-las.
- **Clique** seleciona; `Shift` e `Ctrl` selecionam várias.
- `Ctrl+Shift+A` seleciona todas as partidas visíveis.
- A roda do mouse rola a lista; `Page Up`, `Page Down`, `Home` e `End` também.
- O contador mostra `partidas no filtro / total`.

### Navegar entre as partidas do filtro

`Ctrl+Seta para baixo` carrega a próxima partida do filtro, `Ctrl+Seta para cima` a anterior, `Ctrl+Home` a primeira e `Ctrl+End` a última. Os mesmos comandos estão no menu **Partida**.

### Colunas e ordem

Clique no cabeçalho de uma coluna para ordenar por ela; clique de novo para inverter. Algumas colunas não ordenam (por exemplo, **Nº** e **Marcas**).

Clique com o botão direito no cabeçalho de uma coluna (ou use o botão **Escolher as colunas** da barra da lista) para:

- **Colunas**: escolher quais aparecem. Existem 22: **Nº**, **Brancas**, **Pretas**, **Resultado**, **Data**, **Evento**, **ECO**, **Lances**, **Elo B**, **Elo P**, **Rodada**, **Local**, **Apg** (apagada), **Var** (variantes), **Com** (comentários), **Ano** (anotações), **Abertura**, **Marcas**, **Início**, **País**, **Data do evento** e **Material final**.
- **Alinhar à esquerda**, **Centralizar** ou **Alinhar à direita**.
- **Mais larga** e **Mais estreita**.
- **Mover para a esquerda** e **Mover para a direita**.
- **Ocultar esta coluna** e **Restaurar as colunas**.

O MyChess lembra as colunas escolhidas.

## O filtro

O **filtro** é o conjunto de partidas que a lista mostra. Ele começa com todas as partidas. As buscas (veja [Buscas](04-buscas.md)) reduzem o filtro, e a árvore de aberturas também o usa.

Acima da lista há uma caixa de busca rápida (**Brancas, Pretas, Evento, Local ou Data**) e botões:

- **Achar a próxima partida que contém o texto**: move a seleção para a próxima partida que combina.
- **Manter só as partidas que contêm o texto**: tira do filtro quem não combina. Junte termos com `+` (o filtro exige todos). Por exemplo, `Kasparov+Karpov`.
- **Ignorar maiúsculas**: quando marcado, a busca não diferencia maiúsculas de minúsculas.
- **Restaurar o filtro: todas as partidas**.
- **Inverter o filtro**: passa a mostrar as partidas que estavam fora.
- **Tirar as partidas selecionadas do filtro** (`Shift+Delete`).
- **Deixar só as partidas selecionadas no filtro**.
- **Copiar as partidas selecionadas para a Clipbase**.
- **Marcar ou desmarcar as partidas selecionadas como apagadas** (`Ctrl+Delete`).
- `Ctrl+Shift+J` leva o cursor para a caixa de busca.

Uma busca, ordenação ou cópia longa mostra o andamento ("Filtrando: 40%") e pode ser cancelada.

O menu de contexto de uma partida (botão direito) reúne os mesmos comandos e traz **Carregar partida**, **Navegar**, **Tirar do filtro as partidas acima** e **Tirar do filtro as partidas abaixo**.

### Partidas apagadas

**Apagar** uma partida só coloca nela uma **marca de apagada** (coluna **Apg**). A partida continua no arquivo, esmaecida na lista, e você pode desmarcá-la. As partidas só somem de verdade quando a base é **compactada** (veja [Manutenção e relatórios](10-manutencao-e-relatorios.md)). Bases somente leitura não aceitam essa marca.

## O navegador de partidas

O botão **Navegar pelas partidas selecionadas** (ou a tecla `B` com o ponteiro sobre a lista) abre uma janela com a partida em um tabuleiro. Com várias partidas selecionadas, mostra uma grade de mini-tabuleiros.

A janela tem botões para o início, o lance anterior, o próximo, o fim, **Reproduzir os lances**, **Girar o tabuleiro**, tabuleiros maiores ou menores e **Carregar esta partida no tabuleiro principal**. O navegador mostra no máximo as primeiras partidas pedidas e avisa quando corta a lista.

## Copiar partidas entre bases

Há três formas:

- **Para a Clipbase**: botão **Copiar as partidas selecionadas para a Clipbase**, ou **Banco > Copiar partida para a Clipbase** para a partida atual. **Banco > Colar partida da Clipbase** traz uma partida da Clipbase.
- **Arrastando**: arraste a aba de uma base sobre a aba de outra. O MyChess copia **todas as partidas do filtro** da base de origem para a de destino. Aparece a mensagem "Solte para copiar N partidas de A para B".
- **Pelo menu de contexto da aba**: **Copiar o filtro para** *nome*.

O MyChess pede confirmação antes de copiar para uma base que não é a Clipbase (a opção **Confirmar antes de copiar** do menu da aba controla isso). A cópia mostra progresso e pode ser interrompida; o que já foi copiado fica.

Cópias recusadas, com o motivo:

- origem e destino são a mesma base;
- o destino é somente leitura;
- o filtro da origem está vazio, ou não há partida selecionada.

Em bases SI4, nomes mais longos que o formato comporta são encurtados, e o aviso informa quantos.

## Somente leitura

**Banco > Somente leitura** marca a base atual como somente leitura: ela não aceita mais alterações. Você pode desmarcar para voltar a editar. Arquivos PGN, a Clipbase e bases que não podem ser gravadas ficam sempre somente leitura. Um aviso aparece se você tentar alterar: "Este banco não pode ser alterado".

## Fechar uma base

**Arquivo > Fechar banco** fecha a base do slot atual. Na Clipbase, o comando vira **Limpar a Clipbase**. Se a partida atual tem alterações, o MyChess pergunta se quer salvar.

## Marcadores

Os **marcadores** guardam a posição de uma partida para você voltar a ela depois, mesmo com a base reordenada.

- **Arquivo > Marcadores > Adicionar partida atual** (`Ctrl+D`): cria um marcador da partida aberta. Só vale para partidas de uma base aberta.
- **Arquivo > Marcadores > Gerenciar...** (`Ctrl+Shift+D`): abre o editor.
- Clique em um marcador no menu para ir até ele. Se a base não estiver aberta, o MyChess a abre.

No editor de **Marcadores** você pode **Adicionar**, **Excluir**, **Subir**, **Descer**, **Recuar** e **Avançar** itens (para organizar em pastas), criar uma **Nova pasta** e **Ir** até a partida. Digite o nome na caixa **Título da entrada selecionada**.

Se a base de um marcador sumiu, o aviso é "Não foi possível abrir o banco do marcador".

## Atalhos desta parte

- `Ctrl+O`: abrir. `Ctrl+L`: mostrar ou ocultar a lista de partidas.
- `Ctrl+1` a `Ctrl+9`: trocar de base.
- `Ctrl+Delete`: marcar como apagada. `Shift+Delete`: tirar do filtro.

A lista completa está em [Atalhos de teclado](12-atalhos-de-teclado.md).

## Veja também

- [Buscas](04-buscas.md)
- [Manutenção e relatórios](10-manutencao-e-relatorios.md)
- [Aberturas](05-aberturas.md)
- [Perguntas frequentes e solução de problemas](13-perguntas-frequentes.md)
- [Glossário](14-glossario.md)

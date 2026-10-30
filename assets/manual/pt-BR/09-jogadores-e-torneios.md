# Jogadores e torneios

**Neste capítulo:** como achar jogadores e torneios dentro de uma base, ver as informações de um jogador, corrigir a grafia de nomes, montar a tabela de um torneio, ver estatísticas e gráficos do filtro e usar o rastreador de peças.

*Estado deste capítulo: em desenvolvimento. As janelas descritas aqui já existem, mas ainda falta a revisão final e a conferência visual em janela real. Os pontos pendentes de cada recurso estão indicados abaixo.*

Todas essas ferramentas ficam no menu **Estatísticas** e agem sobre a base do slot atual.

## Localizador de jogadores

*Estado: em desenvolvimento (a janela funciona; falta a revisão da última rodada de correções).*

O **localizador de jogadores** lista os jogadores de uma base, com o número de partidas de cada um. Abra com **Estatísticas > Localizador de jogadores...** (`Ctrl+Shift+F5`).

As colunas são **#** (posição), **Partidas**, **Mais antiga**, **Mais recente**, **País**, **Elo** e **Nome**. Clique no cabeçalho para ordenar.

Os campos acima da lista filtram o que aparece:

- **Partidas**: o mínimo e o máximo de partidas do jogador.
- **Jogador**: parte do nome.
- **Elo**: o maior rating do jogador na base. Alguns valores vêm do arquivo de ortografia (e são estimados).
- **País**: `no` não mostra a coluna; `yes` mostra o país de todos; ou digite o início de um código de país (como `BRA`).
- **Tamanho da lista**: quantos jogadores mostrar.

**Padrões** restaura os valores iniciais e **Atualizar** refaz a lista. Se a base mudou durante a montagem, o aviso é "O banco mudou enquanto a lista era feita; peça de novo."

Um clique duplo em um jogador abre as informações do jogador (veja a seção **Informações do jogador**, abaixo).

## Localizador de torneios

*Estado: em desenvolvimento (mesma situação do localizador de jogadores).*

**Estatísticas > Localizador de torneios...** (`Ctrl+Shift+F4`) lista os torneios que a base contém. Cada torneio é um conjunto de partidas com o mesmo evento, local e datas próximas. As colunas são **#**, **Data**, **Jogadores**, **Partidas**, **Elo**, **Evento**, **Local**, **Vencedor** e **Segundo**.

Filtre pelos campos **Partidas**, **Jogadores**, **Elo médio**, **Data**, **Evento**, **Local**, **Jogador** e **País**.

Clique em um torneio e escolha:

- **Tabela do torneio** (ou clique duas vezes): abre a [tabela do torneio](#tabela-do-torneio).
- **Mostrar no filtro**: o filtro passa a ter só as partidas do torneio.
- **Carregar a primeira partida**.

## Informações do jogador

*Estado: em desenvolvimento (falta a foto do jogador e o botão Editar nome).*

**Estatísticas > Informações do jogador...** abre o resumo de um jogador, o último que você consultou ou o das brancas da partida atual.

A janela mostra:

- o **Título**, o **País**, o **Nascimento**, o **Melhor rating** e a **Biografia**, quando o arquivo de ortografia (veja a próxima seção) tem esses dados;
- um link para o **Perfil na FIDE**, que abre no navegador;
- o total de partidas, o período delas e quantas são vitórias, derrotas e empates, com as **Brancas** e as **Pretas** à parte;
- as **Aberturas mais comuns com as Brancas** e **com as Pretas**;
- o **Histórico de rating**;
- botões **Procurar nome**, **Torneios** (abre o localizador de torneios do jogador), **Editar ratings** e **Jogadores recentes**.

Os números podem ser de **Todas as partidas** ou só das **Partidas do filtro**. **Contra** *nome* mostra o retrospecto contra um adversário.

Se a base não tem aquele nome, o aviso é "Este nome não está no banco de dados". Sem arquivo de ortografia, não há título, país nem biografia. Quando o nome só combina em parte, a janela avisa "Dados aproximados: o nome só começa como o do jogador" e lista as "Possíveis correspondências".

## Corrigir nomes (ortografia)

*Estado: em desenvolvimento (falta testar com o arquivo de ortografia real do Scid vs PC e uma barra de progresso na leitura do arquivo).*

Uma base de dados cheia de partidas importadas costuma ter o mesmo jogador escrito de várias formas: "Kasparov, Garry", "Kasparov, G." e "Garry Kasparov". A **correção ortográfica** troca todas as variantes pela forma correta.

Ela usa um **arquivo de ortografia** (`spelling.ssp`), que é uma lista de nomes corretos. O arquivo vem com o Scid vs PC; o MyChess **não o inclui**. Também aceita `.ssp.gz`.

### Carregar o arquivo de ortografia

1. Escolha **Banco > Arquivo de ortografia > Carregar arquivo de ortografia...** e selecione o `spelling.ssp`.
2. O MyChess lê o arquivo e avisa "Arquivo de ortografia carregado: N jogadores, N eventos, N locais, N rodadas".
3. Outros comandos do menu **Arquivo**: **Recarregar arquivo de ortografia**, **Descarregar arquivo de ortografia** e **Cancelar a carga do arquivo de ortografia**.

### Corrigir os nomes

1. Escolha **Banco > Corrigir nomes (ortografia)** e depois **Nomes de jogadores...**, **Nomes de eventos...**, **Nomes de locais...** ou **Nomes de rodadas...**.
2. O MyChess varre a base e lista as correções: **Nome no banco**, **Nome correto**, **Partidas** e **Vida** (anos de vida do jogador, usados para não trocar nomes de épocas diferentes).
3. Marque as correções que quer aplicar (**Marcar todas** e **Desmarcar todas** ajudam). As **Ambíguas** (nomes que combinam com mais de um nome correto) nunca vêm marcadas, e você as vê ligando **Ambíguas**. **Só sobrenome** inclui nomes que são só um sobrenome.
4. Clique em **Fazer correções** e confirme. **Atenção: não é possível desfazer.** Faça uma cópia da base antes.

O MyChess só corrige bases SI4 editáveis; em uma base somente leitura, o aviso é "Este banco de dados não pode ser alterado".

### Adicionar Elo às partidas

**Banco > Adicionar Elo às partidas...** acrescenta o Elo dos jogadores às partidas que não o têm, usando os ratings do arquivo de ortografia (as linhas `%Elo`). Escolha **Todas as partidas** ou **Partidas do filtro**, marque **Sobrescrever ratings existentes** se quiser e clique em **Conferir** para ver quantos ratings seriam adicionados antes de **Adicionar ratings**.

## Tabela do torneio

*Estado: em desenvolvimento (falta revisar a interface, mostrar título, país e idade dos jogadores e acelerar torneios com centenas de jogadores).*

A **tabela cruzada** (ou tabela do torneio) mostra o resultado de cada jogador contra cada outro. Abra com **Estatísticas > Tabela cruzada** com uma partida de torneio carregada, ou pelo localizador de torneios. Se a partida atual não for de um torneio, o aviso é "Carregue uma partida de uma base para ver o torneio dela".

### Formatos

Em **Formato**, escolha:

- **Todos contra todos**: uma grade com os resultados de cada par;
- **Suíço**: uma linha por jogador, com o adversário e o resultado de cada rodada;
- **Rodadas** (eliminatória);
- **Automático**: o MyChess escolhe.

### Opções

- **Ordenar por**: **Pontuação**, **Nome**, **País** ou **Rating**.
- Colunas extras: **Ratings**, **Títulos**, **Idade em anos**, **Nacionalidades** e **Vitórias/derrotas/empates**.
- **Pontuação de desempate**, com os critérios **Desempate por confronto direto** e **Desempate por vitórias** (e também Sonneborn-Berger e Buchholz).
- **3 pontos por vitória**.
- **Incluir partidas excluídas**.
- **Informação de cor (só suíço)** e **Colunas numeradas (só todos contra todos)**.
- **Agrupar pontuações** e **Colorir linhas alternadas**.

### Usar a tabela

- Clique em uma célula para ver a partida correspondente e **Carregar** ou **Listar**.
- **Definir filtro** deixa no filtro só as partidas do torneio; **Somar ao filtro** acrescenta.
- **Copiar** copia a tabela como texto. **Salvar como texto**, **Salvar como HTML** e **Salvar como LaTeX** gravam em arquivos.
- **Atualizar** refaz a tabela depois de mudanças na base.

A tabela comporta até 1000 jogadores. Passando disso o aviso é "Limite de 1000 jogadores atingido. A tabela está incompleta." Para torneios grandes demais para uma grade, use o formato **Suíço**.

## Estatísticas e gráficos do filtro

*Estado: em desenvolvimento (falta revisão e conferência visual).*

O menu **Estatísticas** mostra números e gráficos sobre as partidas do filtro.

### Estatísticas

**Estatísticas > Estatísticas** mostra uma tabela com a pontuação das brancas em grupos de partidas: todas as partidas, partidas em que os dois jogadores têm rating mínimo (**Ambos com rating:** 2200, 2300...) e intervalos de anos. As colunas são **Critério**, **Jogos**, **1-0**, **=-=**, **0-1** e **Pontuação**. Em **Opções** você escolhe os valores de rating e anos.

### Gráficos do filtro

**Estatísticas > Gráfico de filtro relativo** e **Gráfico de filtro absoluto** mostram a distribuição das partidas do filtro por **Década**, **Ano**, **Rating** ou **Lances**:

- O **relativo** mostra a frequência por mil partidas de cada faixa, para comparar com o resto da base.
- O **absoluto** mostra a quantidade de partidas.

Em **Opções**: o eixo (**De**, **Até**, **Passo**), **Estimar** e **Mostrar pontos**.

### Rating dos jogadores

**Estatísticas > Rating dos jogadores** desenha a evolução do rating de até 9 jogadores ao longo dos anos. Digite o nome e clique em **Incluir**, ou use **Jogadores da partida** para incluir os dois da partida atual. Em **Opções**: **Ano inicial**, **Menor rating**, **Só partidas do filtro** e **Mostrar pontos**.

## Rastreador de peças

*Estado: em desenvolvimento (falta revisão e conferência visual).*

O **rastreador de peças** mostra, em um mapa de calor sobre o tabuleiro, **para onde uma peça costuma ir** nas partidas do filtro. Serve para ver, por exemplo, onde o bispo de casas claras das brancas fica nas partidas de uma abertura. Abra com **Estatísticas > Rastreador de peças**.

1. Clique em uma peça no tabuleiro de seleção: o botão esquerdo escolhe uma peça; o botão direito escolhe também as suas "irmãs" (os dois cavalos, por exemplo).
2. Em **Estatística**, escolha **% de partidas com lance para a casa** ou **% de tempo em cada casa**.
3. Em **Lances**, escolha de qual lance até qual considerar.
4. Em **Cor**, escolha o esquema de cores (**Preto**, **Vermelho**, **Amarelo**, **Ciano**, **Azul** e as versões **invertidas**).
5. Clique em **Atualizar**. O mapa de calor pinta cada casa; passando o mouse você vê a contagem e a porcentagem. A lista **Casas por contagem** ordena as casas.

A contagem usa as partidas do filtro; o painel informa quantas partidas foram seguidas e quantas ficaram de fora ("posição inicial própria, curtas demais ou ilegíveis").

## Veja também

- [Bancos de dados](03-bancos-de-dados.md)
- [Buscas](04-buscas.md)
- [Manutenção e relatórios](10-manutencao-e-relatorios.md)
- [Glossário](14-glossario.md)

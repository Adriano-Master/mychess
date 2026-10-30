# Buscas

**Neste capítulo:** como achar partidas em uma base: pelos dados da partida (cabeçalho), pela posição no tabuleiro, pelo material e por padrões, por uma sequência de lances e por táticas. Também mostra como combinar buscas e como salvar uma busca para repetir depois.

## Como as buscas funcionam

Toda busca age sobre o **filtro** da base atual (veja [Bancos de dados](03-bancos-de-dados.md#o-filtro)). Depois da busca, a lista de partidas mostra só as partidas que combinaram, e o contador diz quantas são ("12/5000").

Cada janela de busca pergunta o que fazer com o filtro que já existe:

- **Limpar filtro** (ou **Procurar em todas as partidas**): ignora o filtro atual e busca em tudo. É a opção inicial.
- **Restringir filtro (E)**: busca só entre as partidas que já estão no filtro. Serve para refinar: primeiro por posição, depois por material, por exemplo.
- **Adicionar ao filtro (OU)**: soma as partidas encontradas ao filtro atual.

Durante uma busca longa aparece uma barra de progresso ("1200 / 5000 partidas") e o botão **Parar**. Se você parar, o filtro fica como estava. Se a base mudar durante a busca, o resultado é descartado e o MyChess avisa.

As buscas ficam no menu **Buscar** e só funcionam com uma base aberta. A janela **Busca avançada** nomeia a base onde buscar no campo **Buscar na base**.

## Busca por cabeçalho

*Estado: em desenvolvimento (falta a janela de busca por cabeçalho; o motor de busca está pronto).*

A busca por cabeçalho acha partidas pelos dados do cabeçalho: jogadores, evento, local, data, resultado, Elo, código ECO e outros campos. Ainda não há uma janela para isso no menu **Buscar**.

Enquanto a janela não existe, use a **busca rápida da lista de partidas**, que tem uma caixa de texto e dois botões:

1. Digite parte de um nome, evento, local ou data na caixa **Brancas, Pretas, Evento, Local ou Data**.
2. Clique em **Achar a próxima partida que contém o texto** para saltar de uma partida para a seguinte, ou em **Manter só as partidas que contêm o texto** para reduzir o filtro.
3. Para exigir vários termos ao mesmo tempo, separe-os com `+`: `Kasparov+Karpov`.

Detalhes no capítulo [Bancos de dados](03-bancos-de-dados.md#o-filtro).

## Busca por posição

Escolha **Buscar > Tabuleiro corrente...** para achar partidas que passaram pela posição que está no tabuleiro. A janela **Busca avançada** abre na aba **Posição**.

1. Em **Posição**, escolha de onde vem a posição: **Do tabuleiro** (a posição atual) ou **De um FEN** (digite ou cole um FEN na caixa **Digite um FEN**).
2. Em **Tipo de busca**, escolha o rigor:
   - **Exata**: todas as peças nas mesmas casas.
   - **Peões**: mesmo material, com todos os peões nas mesmas casas.
   - **Colunas**: mesmo material, com os peões nas mesmas colunas.
   - **Qualquer**: mesmo material, com peças e peões em qualquer casa.
3. Marque **Procurar nas variantes** se quiser que a busca olhe também as variantes das partidas, e não só a linha principal.
4. Marque **Ignorar cores** para achar também a posição com as cores trocadas (a posição "espelhada").
5. Escolha a operação no filtro e a base a pesquisar.
6. Clique em **Buscar**.

Observação importante: a busca **não compara** os direitos de roque nem a casa de en passant. Duas posições que só diferem nisso contam como iguais.

O resultado aparece abaixo, como "340 / 5000 (1,2 s)". Se algumas partidas não puderam ser lidas, o MyChess informa quantas ("3 ilegíveis"). Para olhar uma partida encontrada, clique duas vezes nela na lista; o tabuleiro vai para a posição pesquisada.

Dicas:

- Para estudar uma abertura, vá até a posição na partida e busque. A [árvore de abertura](05-aberturas.md) faz isso automaticamente a cada lance.
- Em **Peões**, a busca acha todas as partidas com a mesma estrutura de peões, útil para estudar planos típicos de uma estrutura.

## Busca por material e padrões

Escolha **Buscar > Material e padrões...** (aba **Material**). Esta busca acha partidas pela **quantidade de peças** ou por **padrões** de colocação.

### Material

Em **Material** você fixa, para as **Brancas** e as **Pretas**, quantas peças de cada tipo (**Dama**, **Torre**, **Bispo**, **Cavalo**, **Menores**, **Peão**) a partida deve ter. Cada tipo tem uma faixa (de tantas a tantas peças). Os atalhos ajudam:

- **Zero**: nenhuma peça daquele tipo.
- **Qualquer**: sem restrição.
- **Tabuleiro corrente**: usa o material da posição atual.
- **Finais comuns**: preenche a faixa para um final conhecido. Há 13, como **Finais de peões**, **Torre e peão contra torre**, **Finais de torres**, **Finais de bispos**, **Bispo contra cavalo**, **Finais de cavalos**, **Finais de damas**, **Dama e peão contra dama** e **Par de bispos contra par de cavalos**.

Outras opções:

- **Diferença de material**: brancas menos pretas, contando dama 9, torre 5, bispo 3, cavalo 3 e peão 1.
- **Bispos em casas**: **Qualquer**, **Mesma cor** ou **Cor oposta**.
- **Faixa de lances**: o número de lances em que a posição precisa valer.
- **Conferem por pelo menos**: quantos meios-lances seguidos o material precisa durar (para ignorar passagens rápidas).
- **Só a posição final**: olha apenas o fim da partida.

### Padrões

Em **Padrões** você marca, para cada peça (Rei branco, Dama branca... Peão preto), uma coluna, uma fileira ou ambas, e escolhe **Sim**, **Não** ou **?** (tanto faz). O botão **Padrões comuns** oferece 14 padrões prontos, como **Peão da dama isolado das brancas**, **Estrutura Maroczy**, **Fianqueto em casas claras**, **Reis em c1 e g8** e **Par de peões c3 e d4**. **Limpar** apaga tudo. Um padrão precisa de coluna, fileira ou as duas.

## Busca por sequência de lances

Escolha **Buscar > Lances...** (aba **Lances**) para achar partidas que tenham uma sequência de lances.

1. Digite os lances na caixa **Lances**, em notação SAN, separados por espaço: `e4 e5 Nf3`. As letras das peças são as inglesas: `K` rei, `Q` dama, `R` torre, `B` bispo e `N` cavalo.
2. Use `?` no lugar de um lance que pode ser qualquer um. O primeiro lance não pode ser `?`.
3. Em **Lado que joga**, escolha **Brancas**, **Pretas** ou **Ambos**.
4. **Teste de xeque**: com **Xeque** ou **Mate**, os sinais `+` e `#` também são comparados (mais lento).
5. Marque **Procurar nas variantes** se quiser.
6. Clique em **Buscar**.

Se um lance não for compreendido, o aviso diz qual ("`Xyz` não é um lance em SAN").

## Táticas e padrões

Escolha **Buscar > Táticas e padrões...** para achar partidas com um motivo tático ou uma estrutura. É uma busca própria do MyChess, e você não precisa escrever nenhum código: escolhe e clica.

A janela tem duas abas: **Prontas** e **Avançada**.

### Prontas

Escolha uma tática e clique em **Procurar**. As opções são:

- **Mate em N** (escolha o número de lances até o mate, e se é um mate que foi jogado ou qualquer mate disponível)
- **Cravada**, **Garfo**, **Espeto**, **Ataque descoberto**, **Ataque duplo**
- **Sacrifício**, **Peça pendurada**, **Peão passado**, **Rei exposto**, **Mate do corredor**, **Promoção**
- Estruturas: **Peão isolado da dama** (Brancas, Pretas ou os dois lados), **Peões pendentes**, **Centro Maróczy**, **Fianchetto** (bispo de casas claras, escuras ou duplo) e **Roques opostos**

Cada tática mostra uma explicação curta ("Uma peça ataca, ao mesmo tempo, dois alvos que valem a pena."). Algumas têm parâmetros, como **Quem** (Brancas, Pretas ou Qualquer lado), **Peça** e **Peça-alvo**.

### Avançada

Para descrever o seu próprio cenário, abra a aba **Avançada** (ou clique em **Personalizar** em uma tática pronta para copiá-la para cá). Você monta o padrão com **cartões**:

1. Clique em **Adicionar condição** e escolha uma condição, agrupada por tipo: **Peças e casas** (por exemplo, **Peça em casas**, **Material**), **Relações** (**Ataca**, **Defende**, **Crava**, **Espeta**, **Dá garfo**), **Táticas** (**Ataque descoberto**, **Ataque duplo**, **Sacrifício**, **Mate em N**), **Estrutura e segurança do rei** (**Peão passado**, **Peão isolado**, **Peões dobrados**, **Rei exposto**, **Par de bispos**) e **Lance e estado** (**Vez de jogar**, **Fase**, **Estado**, **Lance anterior**, **Próximo lance**, **Resultado da partida**).
2. Clique no cartão e preencha os campos. Para escolher casas, clique nelas no tabuleiro (**Clique nas casas para escolhê-las**) ou use as casas que você marcou no tabuleiro (**Usar as casas marcadas**).
3. Agrupe condições com os grupos **Todas estas (E)**, **Qualquer uma destas (OU)** e **Não isto (NÃO)**. **Subir**, **Descer**, **Envolver em**, **Desembrulhar** e **Remover** reorganizam os cartões.
4. Dê um **Nome** ao padrão e, se quiser, uma descrição.

Em **Onde procurar**, ajuste a operação sobre o filtro, a base, o **Número do lance** (faixa de lances) e **Vale por pelo menos** (quantos meios-lances a condição precisa durar). **Só a posição final** olha só o fim da partida.

Antes de rodar na base inteira, clique em **Testar nesta partida**: o MyChess diz em quantas posições da partida do tabuleiro o padrão vale ("Vale em 3 posições desta partida"), sem mexer no filtro.

### Resultados e ocorrências

Terminada a busca, a janela mostra "N / total partidas (tempo)". Se a busca for grande, o MyChess avisa antes ("Esta busca pode demorar...") e você escolhe **Buscar mesmo assim** ou **Cancelar**.

Ao carregar uma partida encontrada, uma barra mostra **Ocorrência 1 de 3**, com botões para a **Ocorrência anterior** e a **Próxima ocorrência**. O tabuleiro destaca as peças e casas envolvidas. **Ver o tabuleiro** esconde a janela de busca para você olhar o tabuleiro, e **Táticas e padrões** a traz de volta.

### Meus padrões

O botão **Salvar padrão...** guarda o padrão com o nome escolhido, e a seção **Meus padrões** lista os salvos para abrir de novo ou apagar. **Exportar...** e **Importar...** trocam padrões com outras pessoas, em arquivos do MyChess.

*Estado: pronto; ainda sem validação manual completa em janela real e com bases grandes.*

## Buscas salvas

Na janela **Busca avançada**, a seção **Buscas salvas** guarda as buscas de cada aba (posição, material e lances):

1. Monte a busca.
2. Digite um nome em **Nome da busca** e clique em **Salvar**.
3. Para repetir, escolha a busca na lista e clique em **Carregar esta busca**. O botão **Apagar esta busca** a remove.
4. **Importar .sso** lê uma busca de material salva pelo Scid vs PC.

Para as táticas e padrões, use **Meus padrões** (acima).

## Limites

- O MyChess não reproduz o CQL, a linguagem de busca de padrões do Scid vs PC. No lugar dela, ele oferece a busca de táticas e padrões descrita acima.
- A busca em bases muito grandes pode demorar. A barra de progresso e o botão **Parar** estão sempre à mão.

## Veja também

- [Bancos de dados](03-bancos-de-dados.md)
- [Aberturas](05-aberturas.md)
- [Treinar](08-treinar.md)
- [Perguntas frequentes e solução de problemas](13-perguntas-frequentes.md)

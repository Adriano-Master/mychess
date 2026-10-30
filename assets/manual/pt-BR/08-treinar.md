# Treinar

**Neste capítulo:** os quatro treinos do MyChess: táticas, cálculo de variantes, aberturas e finais. Para cada um você vê como abrir, as opções, como é a sessão e o que a pontuação significa.

Os treinos ficam no menu **Jogar** (grupo **Treino**), exceto o treino de finais, que fica no painel **Tablebases**. Durante qualquer treino, o tabuleiro fica "travado" (você só joga as respostas do exercício) e a sua partida em edição é guardada e devolvida quando você fecha o treino. Os treinos não rodam durante uma partida online, e o MyChess pede que você termine antes uma partida, um torneio ou uma análise em andamento.

## Treino de táticas

O treino de táticas mostra uma posição de problema e você precisa achar a solução. Abra com **Jogar > Treino de táticas...** (`Ctrl+Shift+X`).

### Configurar

1. Em **Treino**, escolha o tipo:
   - **Mate em N**: dá-se uma posição em que existe mate forçado e você precisa jogar a sequência correta.
   - **Melhor lance**: ache o melhor lance da posição, conforme a partida de onde o problema veio.
2. Em **Problemas de**, escolha a fonte:
   - **Arquivo (SI4 ou PGN)**: clique em **Escolher...** e selecione o arquivo.
   - **Base atual (filtro)**: usa as partidas do filtro da base aberta.
3. **Só partidas com a bandeira Tática (bases)**: usa apenas partidas marcadas como exercícios (por exemplo, pela anotação automática, que marca exercícios táticos).
4. **Procurar mates nas partidas sem marcadores**: o MyChess acha os mates nas partidas, mesmo as que não foram marcadas.
5. **Ordem**: **Em sequência** ou **Aleatória**.
6. Para mates, **Mate mais curto** e **Mate mais longo** limitam o tamanho do problema. Se houver problemas maiores que o limite, o aviso diz quantos foram ignorados.
7. Filtros de nível: **Dificuldade mínima**, **Dificuldade máxima**, **Elo mínimo**, **Elo máximo**.
8. **Tentativas por problema** e **Limite de tempo (segundos)**; "sem" desliga o limite.
9. **Mostrar os problemas já resolvidos**, **Permitir dicas** e **Aceitar um mate até dois lances mais longo**.
10. **Perguntar à engine quando meu lance não é o da partida** (segunda opinião): se o seu lance for diferente do da partida original, uma engine confere se ele também é bom.
11. Clique em **Iniciar**.

### Durante o treino

O título mostra "Mate em 3" ou "Encontre o melhor lance". Jogue o lance no tabuleiro. O MyChess responde:

- **Certo! Continue: mate em 2**: o lance está certo; continue.
- **Esse não: o mate forçado desapareceu**, **Existe um mate mais curto** ou **Não é o melhor lance**: tente de novo.

Os botões:

- **Dica**: mostra qual peça mover e, no segundo clique, o próprio lance.
- **Ver solução**: desiste e mostra a solução no tabuleiro.
- **Tentar de novo**: monta o mesmo problema outra vez.
- **Próximo problema** (`Enter`): vai ao próximo (pula este se não foi resolvido).
- **Continuar**: continua jogando contra a engine a partir da posição.
- **Fechar**: encerra o treino e devolve a sua partida.

O resultado de cada problema é "Resolvido de primeira!", "Resolvido, depois de alguns erros", "Resolvido, com dica" ou "A solução está no tabuleiro".

### Pontuação

O MyChess guarda o seu histórico: "Resolvidos 40 de 50 (80%), sequência 6 (melhor 11), média 24 s". **Zerar pontuação** apaga o histórico do modo e os problemas resolvidos da fonte atual (clique de novo para confirmar).

*Estado: pronto; ainda sem validação manual completa em janela real.*

## Cálculo de variantes

O **cálculo de variantes** treina uma habilidade rara: calcular linhas de cabeça, sem mover as peças. O método vem do treinador Stoyko. Abra com **Jogar > Cálculo de variantes...** (`Ctrl+U`).

### Como funciona

1. Escolha a **Posição**: **Posição atual da partida** ou um **FEN**.
2. Escolha a **Engine** (**Automática** usa uma engine UCI da lista) e quantas **Linhas pedidas** você precisa dar.
3. Opções da engine: **Linhas da engine no início (MultiPV)**, **Tempo da engine no início (segundos)** e **Tempo de cada busca de uma linha (segundos)**. **Limite de tempo** define quanto tempo você tem.
4. **Mostrar no tabuleiro as peças da linha**: desligado, o tabuleiro fica parado e você calcula de cabeça (o modo mais difícil).
5. Clique em **Iniciar**.

Durante o exercício, a engine analisa a posição em segundo plano. Você faz o seguinte:

1. Digite uma linha na caixa **Lances** em notação SAN ou UCI, com as letras de peças em inglês (`N` cavalo, `B` bispo, `R` torre, `Q` dama, `K` rei), por exemplo `Nf3 d5 d4`. Aperte `Enter`. O tabuleiro não se mexe.
2. Termine a linha com a sua **avaliação**, clicando num dos símbolos: `-+` (as pretas estão ganhando), `-/+` (as pretas estão claramente melhor), `=+` (um pouco melhor para as pretas), `=` (igualdade), `+=`, `+/-` e `+-` (as brancas estão ganhando).
3. Repita até dar as linhas pedidas. **Voltar lance** desfaz o último lance da linha em andamento, e **Apagar** tira uma linha inteira.
4. Clique em **Terminei**. O MyChess pede à engine que avalie as suas linhas.

### O resultado

O MyChess mostra:

- **Nota: N de 100**, formada por três partes: os candidatos que você considerou, a precisão das suas avaliações e a qualidade das linhas, menos uma penalidade se você perdeu o lance crucial.
- **Bons lances encontrados**: quantos dos bons lances da engine você incluiu.
- **Lance crucial perdido**, se for o caso.
- **Outros bons lances que você não considerou**.
- Para cada linha, o que você disse, o que a engine diz e se a linha se sustenta ("a linha se sustenta" ou "a linha perde terreno").

**Adicionar à partida** acrescenta as suas linhas e os lances perdidos como variantes da partida. **Novo exercício** prepara outro. A seção **Exercícios anteriores** guarda o histórico, e **Rever** mostra de novo as linhas de um exercício, sem engine. **Limpar histórico** apaga tudo.

*Estado: pronto; falta ajustar os pesos da nota com partidas reais e testar com uma engine de verdade.*

## Treinador de aberturas

O treinador de aberturas sorteia linhas do seu repertório e você as repete de memória. Abra com **Jogar > Treinador de aberturas...** (`Ctrl+Shift+F6`).

### O repertório

Em **Repertório**, escolha de onde vem:

- **Pelo tipo da base**: usa as bases abertas que têm o tipo "Aberturas para Brancas", "Aberturas para Pretas" ou "Aberturas para ambas as cores". Esse tipo é uma característica da base no formato do Scid (no Scid vs PC ele se chama "Openings for White", "Openings for Black" e "Openings for either color"). O MyChess ainda não tem uma janela para mudar o tipo de uma base. Se não houver nenhuma base desse tipo, aparece "Nenhuma base do tipo 'Aberturas para ...' está aberta".
- **Base atual**: usa as partidas da base atual como repertório.
- **Arquivo...**: um livro Polyglot (`.bin`), uma máscara da árvore (`.stm`), uma base SI4 ou um PGN. Com máscara, só os lances marcados "Para treinar" ou verdes valem (a opção **Em máscara: só os lances marcados** controla isso).

### As opções

- **Lado a treinar**: **Brancas**, **Pretas** ou **Ambos**.
- **Profundidade máxima (meios-lances)**.
- **Permitir só os melhores lances**: só aceita os lances mais fortes do repertório.
- **O adversário joga os melhores lances**: o computador responde sempre com o lance principal, sem sortear.
- **Não repetir a linha**: sorteia linhas diferentes a cada vez.
- **Só linhas marcadas**.
- **Mostrar os lances candidatos**: mostra a lista de lances do repertório (uma ajuda; ligue só para estudar).
- **Zerar estatísticas**: apaga a pontuação do repertório.

### Durante o treino

O treinador sorteia uma linha, com mais frequência as que você sabe menos (um sistema de repetição espaçada), e joga o lado do adversário. Na sua vez ("Sua vez"), você joga o lance do repertório.

- Um lance fora do repertório é recusado: "Lance fora do repertório".
- Um lance do repertório marcado como duvidoso é recusado com "Esse lance está no repertório, mas marcado como duvidoso".
- **Dica**: mostra a peça a mover e depois o lance.
- **Repetir linha** recomeça a mesma linha; **Próxima linha** (`Enter` no fim de uma linha) sorteia outra.
- **Relatório**: mostra quanto do repertório você sabe, com posições nunca jogadas, jogadas, vencidas (precisam ser revistas) e aprendidas.
- **Fechar**: encerra o treino e devolve a sua partida.

A pontuação fica guardada no seu arquivo de treino. Se o repertório só tiver lances de um lado, o MyChess avisa e treina esse lado.

*Estado: pronto; ainda sem validação manual completa com bases, livros e máscaras reais.*

## Treino de finais

O treino de finais joga uma posição de final contra uma **tablebase**, a base de dados que sabe o resultado exato de qualquer posição com poucas peças. A tablebase sempre responde com o melhor lance, então o treino mostra se você sabe converter uma vitória ou segurar um empate. Veja como ligar as tablebases em [Engines e análise](06-engines-e-analise.md#tablebases).

1. Abra o painel **Tablebases** (**Ferramentas > Tablebases**) e ligue uma fonte de tablebase.
2. Vá até uma posição de final (não terminada) e clique em **Treinar**. Os resultados da tabela ficam ocultos durante o treino.
3. Para uma posição aleatória, digite o material em **Material, por exemplo KRvKP** (`K` rei, `R` torre, `P` peão, `v` separa os lados) e clique em **Ir**; **Nova posição** troca a posição por uma aleatória do mesmo material.
4. Jogue. A cada lance a tablebase responde ("A tablebase está respondendo...") e o painel mostra seus lances e "escorregadas" (lances que pioram o resultado teórico).
5. Ao fim, o veredito pode ser "Você converteu a vitória", "Você segurou o empate", "Você deixou o resultado escapar" ou "A posição estava perdida e a tablebase ganhou".

**Parar o treino** encerra. Em uma partida online, o treino fica bloqueado.

*Estado: pronto no programa; falta teste manual com tabelas reais.*

## Veja também

- [Buscas](04-buscas.md)
- [Aberturas](05-aberturas.md)
- [Engines e análise](06-engines-e-analise.md)
- [Jogar](07-jogar.md)
- [Glossário](14-glossario.md)

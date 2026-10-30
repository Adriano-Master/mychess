# Perguntas frequentes e solução de problemas

**Neste capítulo:** respostas para as dúvidas mais comuns e o que fazer quando uma mensagem de erro aparece. As perguntas estão agrupadas por assunto.

## Começando

### Por onde começo?

Abra um arquivo com **Arquivo > Abrir...** (`Ctrl+O`) e clique duas vezes em uma partida da lista. Veja [Primeiros passos](01-primeiros-passos.md).

### Onde está a pasta MyChess com o Stockfish e a base de exemplo?

Esse recurso ainda não está pronto: o MyChess ainda não cria a pasta nem instala o Stockfish sozinho. Veja a seção "A pasta MyChess" em [Primeiros passos](01-primeiros-passos.md). Por enquanto, você mesmo baixa uma engine e a cadastra em **Ferramentas > Engines...**.

### A lista de partidas sumiu. Como volto a vê-la?

Aperte `Ctrl+L`, ou marque **Visualizar > Painéis > Lista de partidas**. Se a tela estiver bagunçada, use **Visualizar > Restaurar layout**.

### Posso usar as bases do Scid vs PC?

Sim. O MyChess abre e grava bases `.si4` (com os arquivos `.sn4` e `.sg4`) sem conversão. Bases no formato antigo `.si3` precisam ser atualizadas antes, no próprio Scid vs PC.

## Bases e partidas

### Por que não consigo alterar a base?

Ela deve estar marcada como somente leitura. Arquivos PGN, a Clipbase e bases gravadas com proteção são sempre somente leitura. Se a base é um `.si4` comum, desmarque **Banco > Somente leitura**. A mensagem típica é "Este banco não pode ser alterado".

### "Há bancos demais abertos; feche um antes"

O MyChess abre até oito bases ao mesmo tempo. Feche uma com **Arquivo > Fechar banco**.

### "O banco tem uma operação inacabada e precisa ser reparado antes"

A base estava sendo gravada quando o programa ou o computador parou. O MyChess faz as operações grandes de forma que elas possam ser concluídas ou desfeitas, mas esta base ficou no meio. A janela de manutenção que repara isso ainda não existe; veja [Manutenção e relatórios](10-manutencao-e-relatorios.md). Abrir a base para escrita (por exemplo, com `mychess-cli compact`) conclui a operação. Se não resolver, restaure uma cópia.

### "Os arquivos do banco estão danificados ou têm uma versão não suportada"

Confira se os três arquivos da base (`.si4`, `.sn4` e `.sg4`) estão juntos, na mesma pasta e com o mesmo nome. Se um faltar ou estiver truncado, o MyChess não abre a base. Restaure de uma cópia.

### "O banco está aberto para gravação em outro lugar"

Outro programa (ou outra cópia do MyChess) está usando a base. Feche-o e tente de novo.

### "O banco mudou no disco; foi recarregado e nada foi gravado"

Alguém alterou a base por fora enquanto você a usava. Repita a operação.

### Apaguei uma partida, mas ela continua na base

Apagar só **marca** a partida. Ela some do arquivo quando a base é compactada. Veja [Bancos de dados](03-bancos-de-dados.md#partidas-apagadas).

### Importei o mesmo PGN duas vezes e as partidas ficaram repetidas

O MyChess não confere repetições ao importar. A busca de duplicatas ainda está em desenvolvimento (veja [Manutenção e relatórios](10-manutencao-e-relatorios.md)).

### Como gravo a partida que editei de volta na base?

Esse comando (como o "Substituir partida" do Scid) ainda não existe no menu. Salve a partida em PGN (**Arquivo > Salvar como...**) e importe o arquivo com **Arquivo > Importar arquivo PGN...**.

### O PGN tem letras estranhas no lugar dos acentos

O MyChess reconhece a codificação (UTF-8, Latin-1, Windows-1252) automaticamente. Se mesmo assim o texto vier errado, o arquivo provavelmente está em outra codificação. Abra-o num editor de texto, salve como UTF-8 e abra de novo.

## Buscas

### A busca não acha uma partida que eu sei que existe

- Confira se o filtro está completo: use **Restaurar o filtro: todas as partidas** e busque com **Limpar filtro**. Uma busca com **Restringir filtro (E)** só procura entre as partidas que já estão no filtro.
- Na busca por posição, o MyChess não compara direitos de roque nem en passant, e a busca em variantes está desligada por padrão.
- Na busca por sequência de lances, os lances precisam estar em notação SAN com as letras de peças em inglês (`Nf3`, e não `Cf3`). Se aparecer o aviso de lance não compreendido, confira a escrita.

### A busca está demorando

Em bases grandes isso é normal. Veja o andamento na barra de progresso; **Parar** interrompe e deixa o filtro como estava. Na busca de táticas e padrões, o MyChess avisa antes quando a busca pode ser demorada.

## Engines e análise

### "O comando não foi encontrado neste computador"

O caminho da engine mudou ou está errado. Abra **Ferramentas > Engines...**, clique em **Editar** e escolha o arquivo certo com **Procurar...**.

### "O arquivo não pode ser executado"

No Linux e no macOS, o arquivo da engine precisa ter permissão de execução (`chmod +x arquivo`).

### "O arquivo não é uma engine ou não fala UCI nem XBoard"

O programa escolhido não é uma engine de xadrez, ou é uma engine que fala outro protocolo. Use **Detectar protocolo** para conferir.

### A engine abre e fecha logo em seguida

Veja o **Log** do painel de análise: ele mostra a mensagem de erro da engine. Causas comuns: faltam arquivos que a engine precisa (como a rede neural), o programa é para outra arquitetura ou o `Hash` pedido é maior que a memória livre. Em **Opções UCI...**, reduza o `Hash`.

### "N engines estão em execução; isso usa muita CPU"

Cada engine usa processador e memória. Pare as que não precisa em **Ferramentas > Engines...** (botão **Parar**).

### A engine joga fraco / forte demais

Em **Jogar contra a engine**, ligue **Limitar a força (UCI_Elo)** ou use o **Jogo tático**, que ajusta a força a um Elo. Nem toda engine aceita limitar a força.

### Os números da análise mudam toda hora

É normal: a engine refina o resultado conforme calcula mais fundo. Espere a profundidade subir antes de confiar nos números. Veja [Engines e análise](06-engines-e-analise.md).

## Jogar online

### "Partida online: assistência desativada"

Durante uma partida no Lichess ou no FICS, o MyChess bloqueia engines, árvore de aberturas, tablebases e buscas, para que ninguém jogue com ajuda. Isso termina quando a partida termina. Se o bloqueio ficar preso depois de uma partida acabada, use **Online > Remover o bloqueio de assistência...**.

### "O Lichess recusou o token"

O token expirou ou foi revogado. Crie outro (ou entre pelo navegador) em **Online > Conectar ao Lichess...**.

### "Falta uma permissão ao token"

Crie o token de novo com as permissões indicadas no link da janela de conexão.

### Quero jogar uma variante do Lichess (Chess960, Rei da Colina...)

O MyChess joga online só o xadrez padrão. Continue a partida no navegador.

### Fechei o programa no meio de uma partida online

O relógio continua correndo no servidor. Reabra o MyChess e conecte-se: ele retoma a partida em andamento ("Uma partida em andamento foi retomada") ou, se isso não for possível, continue no navegador.

## Aparência e outros

### O programa está em inglês

Escolha **Visualizar > Idioma > Português**, ou **Sistema** para seguir o idioma do computador.

### Não ouço os sons

Abra **Visualizar > Sons…** e confira se **Tocar sons** está marcado e o **Volume** não está no mínimo. Sem placa de som, o programa funciona, mas fica mudo.

### Mudei o tema e nada aconteceu

Na janela **Visualizar > Tema…**, as escolhas só valem depois que você clica em **Aplicar**.

### O MyChess avisa "Não foi possível ler o arquivo de preferências; os padrões estão em uso"

O arquivo de preferências estava danificado. O MyChess guardou uma cópia com a extensão `.corrupt` e voltou aos padrões. Você pode refazer as escolhas.

### Não consigo usar uma tecla de atalho

Veja se não há um diálogo aberto e se o foco não está numa caixa de texto. Alguns atalhos (como `Espaço`) só funcionam fora das caixas de texto. A lista completa está em [Atalhos de teclado](12-atalhos-de-teclado.md).

## O que o MyChess não faz

- **Xadrez por correspondência** (Xfcc, relés, e-mail): não será implementado.
- **Gráfico de pontuação (avaliação)** da partida: não existe. Use o [relatório da partida](06-engines-e-analise.md) e a barra de avaliação.
- **Semáforo MultiPV**: não existe.
- **CQL**, a linguagem de busca do Scid vs PC: não será implementada; no lugar há a busca de [táticas e padrões](04-buscas.md).
- **Variantes de xadrez** (Rei da Colina, Crazyhouse...): não são suportadas.

## Veja também

- [Primeiros passos](01-primeiros-passos.md)
- [Bancos de dados](03-bancos-de-dados.md)
- [Engines e análise](06-engines-e-analise.md)
- [Glossário](14-glossario.md)

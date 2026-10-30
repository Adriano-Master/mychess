# Manual do usuário do MyChess

O MyChess é um programa para guardar, estudar e analisar partidas de xadrez, para Linux, Windows e macOS. Ele é uma reimplementação moderna do Scid vs PC, com banco de dados de partidas, árvore de aberturas, engines, treinos e jogo online.

## Como usar este manual

Se você nunca usou um programa de xadrez com banco de dados, comece por [Primeiros passos](01-primeiros-passos.md) e siga na ordem dos capítulos. Se você já conhece o Scid vs PC, vá direto ao assunto que procura: cada capítulo é independente e começa com um resumo do que trata.

Alguns cuidados ao ler:

- **Nomes entre negrito** são os textos exatos da tela: menus, botões, campos e avisos. `Ctrl+Z` e outros textos em fonte de máquina são teclas ou textos que você digita. No macOS, `Ctrl` é a tecla `Cmd`.
- **Menu > Item** indica um caminho de menu. **Arquivo > Abrir...** quer dizer: abra o menu **Arquivo** e escolha **Abrir...**.
- Linhas que começam com *Estado:* avisam que um recurso **ainda não está completo**. Um recurso sem essa linha está pronto. O MyChess está em desenvolvimento, e este manual não promete o que ainda não existe.
- Termos técnicos (PGN, FEN, ECO, UCI, SI4...) são explicados no [Glossário](14-glossario.md).
- Os links levam a outro capítulo ou seção.

## Capítulos

1. [Primeiros passos](01-primeiros-passos.md): o que é o MyChess, a tela principal, a pasta `MyChess` e como abrir a primeira base.
2. [O tabuleiro e a partida](02-tabuleiro-e-partida.md): jogar lances, andar pela partida, variantes, comentários, símbolos de anotação, desfazer, copiar e colar, configurar posição e Chess960.
3. [Bancos de dados](03-bancos-de-dados.md): abrir e importar bases e PGN, a lista de partidas, o filtro, as colunas, marcadores, exportação e cópia entre bases.
4. [Buscas](04-buscas.md): buscar partidas por posição, material, sequência de lances, táticas e padrões, e salvar buscas.
5. [Aberturas](05-aberturas.md): a árvore de aberturas, os livros Polyglot, o ECO e as máscaras.
6. [Engines e análise](06-engines-e-analise.md): cadastrar engines, analisar, anotar partidas, o relatório da partida e as tablebases.
7. [Jogar](07-jogar.md): jogar contra o computador, torneios entre engines, Lichess, FICS, tabuleiros eletrônicos e sons.
8. [Treinar](08-treinar.md): treino de táticas, cálculo de variantes, treinador de aberturas e treino de finais.
9. [Jogadores e torneios](09-jogadores-e-torneios.md): localizadores, informações do jogador, correção de nomes, tabela de torneio, estatísticas e rastreador de peças.
10. [Manutenção e relatórios](10-manutencao-e-relatorios.md): compactar e ordenar bases, relatórios de abertura e de jogador, arquivos EPD, mesclar partidas e exportação.
11. [Aparência e preferências](11-aparencia-e-preferencias.md): tema, cores, peças, tabuleiro, menu em faixa, idioma e painéis.
12. [Atalhos de teclado](12-atalhos-de-teclado.md): a lista completa, por assunto.
13. [Perguntas frequentes e solução de problemas](13-perguntas-frequentes.md): dúvidas comuns e o que fazer diante de um erro.
14. [Glossário](14-glossario.md): o significado dos termos.

## Atalhos para as dúvidas mais comuns

- Abrir uma base ou um PGN: [Primeiros passos](01-primeiros-passos.md).
- Achar todas as partidas que passaram por uma posição: [Buscas](04-buscas.md).
- Ver o que se joga numa posição de abertura: [Aberturas](05-aberturas.md).
- Analisar com o Stockfish: [Engines e análise](06-engines-e-analise.md).
- Jogar contra o computador: [Jogar](07-jogar.md).
- Importar um PGN grande: [Bancos de dados](03-bancos-de-dados.md).
- Uma mensagem de erro apareceu: [Perguntas frequentes e solução de problemas](13-perguntas-frequentes.md).

## Sobre este manual

Este manual acompanha o programa. Ele pode ser lido no repositório do projeto, na pasta `assets/manual`, e deve poder ser aberto dentro do programa, no menu **Ajuda**.

*Estado: previsto. O visualizador do manual dentro do programa ainda não existe; por enquanto, leia os arquivos diretamente.*

O manual descreve a versão em desenvolvimento do MyChess; algumas partes mudam conforme o programa evolui.

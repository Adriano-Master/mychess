# ADR 0009: Busca de táticas e padrões própria, sem a linguagem CQL

- **Status:** aceita pelo usuário (2026-10-24)

## Contexto

O Scid vs PC 4.27 traz o CQL (*Chess Query Language*, `src/cql/`), uma
linguagem de texto para descrever posições e temas e pesquisar partidas. O
código do CQL é "todos os direitos reservados", com permissão de uso GPL dada
por e-mail dos autores (`src/cql/COPYING`; ver ADR 0004 e `roteiro.md`). Isso
é um risco de licença para o MyChess. Além disso, o CQL exige que o usuário
aprenda uma sintaxe própria, o que a maioria não faz.

O usuário decidiu **não usar a linguagem CQL** no MyChess, mas manter o
recurso: pesquisar posições e táticas de forma **fácil e simples**.

## Decisão

- O MyChess **não implementa nem lê CQL**: nem o código de `src/cql/`, nem a
  sintaxe, nem os arquivos `.cql`. O diretório `cql/` do Scid não é portado.
- O item do M6 passa a ser uma **busca de táticas e padrões própria**, com
  duas camadas na mesma janela de pesquisa:
  1. **Presets**: um clique escolhe uma tática comum (mate em 2, mate em 3,
     cravada, garfo, espeto, ataque descoberto, sacrifício...). Cada preset é
     um padrão pronto, com parâmetros simples quando fizer sentido (cor,
     peça, número de lances).
  2. **Busca avançada visual**: o usuário descreve o cenário escolhendo peças,
     casas e relações em campos e no tabuleiro (por exemplo, "uma torre branca
     cravando um cavalo preto contra o rei"), sem escrever código. Condições
     combinam com E, OU e NÃO.
- O núcleo é um **modelo de padrão estruturado** em Rust (uma árvore de
  condições serializável em TOML), não uma linguagem textual. Os presets são
  padrões predefinidos nesse modelo; a busca avançada edita o mesmo modelo. O
  usuário pode salvar e reutilizar seus padrões, no formato do MyChess.
- A implementação é **original**: detectores de motivos táticos escritos a
  partir dos conceitos do xadrez, sem copiar o CQL. A camada de regras
  (`mychess-core`) fornece o que for preciso (ataques, cravadas, buscas de
  mate em N); a varredura sobre o banco fica em `mychess-db`; a janela, em
  `mychess-app` e `mychess-gui`.
- Os itens não relacionados ao CQL do M6 continuam como estavam: busca por
  posição, por material e padrões do Scid, e por sequência de lances.

## Alternativas consideradas

- **Reimplementar o CQL a partir da especificação pública:** reduz o risco do
  código, mas mantém a barreira da sintaxe, que o usuário quer evitar.
- **Pedir autorização formal aos autores do CQL:** depende de terceiros e
  mantém a linguagem de texto.
- **Linguagem de texto simplificada própria:** continua exigindo que o
  usuário escreva código.

## Consequências

- Sem risco de licença vindo do CQL. Os arquivos `.cql` de terceiros não
  funcionam no MyChess.
- Há mais trabalho de projeto: o catálogo de táticas, a semântica dos
  padrões (em que posição da partida o tema vale, linha principal ou
  variações) e o custo da varredura (mate em N e motivos táticos pedem busca
  por posição, mais cara que a busca por casas).
- A interface precisa ser testada com usuários: o critério de sucesso é achar
  uma tática comum em poucos cliques.
- A especificação detalhada (catálogo, modelo de padrão, desempenho, interface)
  vira `docs/specs/m6-*.md`, com `/consultar-scid` para o que o Scid oferece
  hoje em "Material/Padrões", quando o M6 começar.

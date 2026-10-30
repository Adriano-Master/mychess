# Formato `.toml` de padrão tático (`mychess-pattern`)

Arquivo de texto (TOML) que guarda **um** padrão tático ou estrutural do MyChess:
uma árvore de condições avaliada nas posições da linha principal de cada partida
(janela "Táticas e padrões", M6-2). É um formato **próprio do MyChess**, sem
equivalente no Scid vs PC (o Scid só tem a busca por material e padrões simples
de peça em casa, `sc_search material`; a ADR 0009 recusa o CQL).

- **Código no MyChess:** `crates/mychess-db/src/pattern/model.rs` (tipos,
  validação, leitura e escrita), `pattern/store.rs` (diretório de padrões do
  usuário), `pattern/presets.rs` (padrões prontos, que geram este mesmo tipo).
- Especificação: [`../specs/m6-2-taticas-e-padroes.md`](../specs/m6-2-taticas-e-padroes.md),
  seção "Dados e formatos" e decisões D3276 e D3277.
- Exemplos completos: `crates/mychess-db/tests/m6_2a_fixtures/*.toml`.

## 1. Estrutura do arquivo

```toml
format = "mychess-pattern"   # obrigatório, exatamente este texto
version = 1                  # obrigatório; inteiro
name = "Garfo de cavalo"     # 1 a 80 caracteres, sem caracteres de controle
description = "..."          # opcional; até 500 caracteres
created = "2026-10-27"       # opcional; aaaa-mm-dd, dado por quem grava

[scope]                      # opcional; cada campo tem padrão
min_move = 1                 # primeiro lance (número do lance das brancas); 0 inclui a posição inicial
max_move = 999               # >= min_move
hold = 1                     # 1 a 40 posições consecutivas em que o padrão deve valer
end_only = false             # só a posição final (ignora faixa e hold)
ignore_colors = false        # tenta também com as cores trocadas e o tabuleiro espelhado

[root]                       # a árvore de condições
kind = "..."
```

Escalares vêm antes das tabelas (exigência do serializador). **Campo
desconhecido é erro** (`deny_unknown_fields`), para proteger de erro de
digitação; por isso qualquer mudança de formato, inclusive um campo opcional
novo, aumenta `version`, e o aplicativo continua lendo as versões antigas.

## 2. Árvore de condições (`kind`)

`kind` é a etiqueta de cada nó. Grupos têm 1 a 24 itens.

| `kind` | Campos (padrão entre parênteses) |
|---|---|
| `all` | `items`: lista de nós (E) |
| `any` | `items`: lista de nós (OU) |
| `not` | `item`: um nó |
| `piece` | `side`, `roles` (vazio = qualquer peça), `region` (`"anywhere"`), `min` (1), `max` (sem limite): quantidade de peças |
| `material` | `white`, `black` (`Counts`: `queens`, `rooks`, `bishops`, `knights`, `minors`, `pawns`, cada um `{ min, max }`), `diff` (`{ min, max }`, brancas menos pretas, D9 T5 B3 C3 P1), `bishops` (`either`/`same`/`opposite`) |
| `side_to_move` | `side`: `white` ou `black` |
| `phase` | `phase`: `opening`, `middlegame`, `endgame` |
| `status` | `status`: `check`, `checkmate`, `stalemate` (do lado que joga) |
| `move` | `which` (`previous`/`next`), `by`, `roles`, `from`, `to`, `capture`/`check` (`yes`/`no`/`both`), `promotion` (lista), `castling` (`short`/`long`/`any`) |
| `result` | `results`: lista de `white_wins`, `draw`, `black_wins`, `unknown` (não vazia) |
| `feature` | `feature` (`passed_pawn`, `isolated_pawn`, `doubled_pawns`, `exposed_king`, `bishop_pair`), `side`, `region`, `min` (1) |
| `relation` | `relation` (`attacks`, `defends`, `pins`, `skewers`, `forks`), `actor` (`side`, `roles`, `region`), `target` (`roles`, `region`), `behind` (só `pins`/`skewers`; `roles = ["king"]` = cravada absoluta), `gain` (`any`/`undefended`/`wins_material`, só `attacks`), `min_targets` (2; 2 a 8, só `forks`), `when` (`any`/`actor_just_moved`/`actor_to_move`), `new_this_move` (false) |
| `discovered` | `discovery` (`attack`/`check`/`double_check`), `by`, `roles` |
| `double_attack` | `by` |
| `sacrifice` | `by`, `roles` (vazio = qualquer exceto o rei), `min_net` (2; 1 a 20), `outcome` (`recovered`/`mate`/`either`/`any`; `either`), `within` (3; 1 a 8) |
| `mate` | `by` (`opponent` é inválido), `moves_min`, `moves_max` (1 a 8), `mode` (`played`/`available`; `available`), `forcing_only` (true), `max_nodes` (20000; 1000 a 2000000) |

`side`/`by`: `white`, `black`, `mover` (quem joga na posição avaliada),
`opponent`, `any` (vale para as brancas **ou** para as pretas, condição por
condição). `mover` é recusado em `discovered`, `double_attack` e `move` com
`previous`, porque a posição avaliada é a depois do lance. Papéis (`roles`):
`king`, `queen`, `rook`, `bishop`, `knight`, `pawn`.

Regiões (`region`, `from`, `to`): `"anywhere"`; `{ squares = ["e4", "d4"] }` (1 a
64 casas); `{ file = "d" }`; `{ rank = 7 }` (1 = linha das brancas);
`{ own_rank = 7 }` (do ponto de vista do dono da peça); `{ rect = { from =
"c3", to = "f6" } }`; `{ named = "center" }` com `center`, `extended_center`,
`queen_side`, `king_side`, `white_half`, `black_half`, `light_squares`,
`dark_squares`, `edge`, `long_diagonals`.

## 3. Limites e validação

- Profundidade da árvore até **6** (folha sozinha = 1) e até **48** nós.
- Arquivo até **256 KiB**, checado antes de ler; texto com byte NUL é recusado;
  codificação UTF-8 (com ou sem BOM) ou, se inválida, Windows-1252.
- Aninhamento de tabelas além de 40 níveis é recusado antes de chegar ao `serde`.
- `min <= max` em toda faixa; `hold` de 1 a 40; `max_move >= min_move`;
  `Region` com casas, coluna e linha dentro dos limites; `created` no formato
  `aaaa-mm-dd`.
- Erros: `WrongFormat` (`format` diferente), `UnsupportedVersion` (`version`
  maior ou ausente; nunca se interpreta parcialmente), `TooLarge`, `NotText`,
  `Toml`, `InvalidValue { path, reason }` (com o caminho do nó, por exemplo
  `root.items[2].actor.region`), `EmptyGroup`, `TooDeep`, `TooManyNodes`.

## 4. Padrões do usuário no disco

- Diretório `patterns/` dentro de `ProjectDirs::from("org", "mychess",
  "MyChess").data_dir()`; **quem escolhe o diretório é o aplicativo**, a
  biblioteca recebe `dir: &Path`.
- **Um arquivo por padrão**, `<nome-normalizado>.toml`. O nome verdadeiro fica
  dentro do arquivo; o nome do arquivo é só a forma segura dele
  (`file_stem_for`): caracteres `< > : " / \ | ? *` e de controle viram `_`,
  minúsculas, sem ponto ou espaço nas pontas, nomes reservados do Windows
  (`CON`, `NUL`, `COM1`...) com prefixo `_`, no máximo 80 caracteres. Nomes que
  diferem só na caixa ou que dão o mesmo texto recebem `-2`, `-3`... (nunca
  sobrescrevem outro padrão); salvar com o mesmo nome **substitui** o arquivo.
- Escrita atômica: grava num temporário oculto (`.<nome>.tmp`) **no mesmo
  diretório**, fecha o handle e faz `rename`; em erro o arquivo anterior fica
  intacto e o temporário é removido.
- No máximo **1000** padrões por diretório: a listagem coleta **só** os
  arquivos `.toml` regulares e não ocultos (outros arquivos nunca ocupam
  vaga), ordena pelo nome do arquivo sem diferenciar caixa e trunca em 1000;
  salvar um novo além disso é erro. Salvar e importar listam o diretório uma
  única vez. Arquivos
  que não são regulares (diretórios, FIFOs) são ignorados na lista e recusados
  na leitura, sem bloquear.
- A lista (`list_patterns`) ordena por nome sem diferenciar caixa e inclui os
  arquivos inválidos como `broken` com o motivo, para a interface oferecer
  apagá-los; um arquivo ruim não derruba a lista.
- Importar (`import_pattern`) lê e valida o arquivo, e o copia para o
  diretório **sem sobrescrever** (nome ocupado vira `Nome (2)`, `Nome (3)`...);
  exportar (`export_pattern`) grava em qualquer caminho, de forma atômica.

## 5. Valor no filtro da base

A varredura (`search_pattern`) grava no filtro de cada partida que casa o
**meio-lance da primeira posição que casa mais um** (`ply + 1`), ou **1** se o
meio-lance é 254 ou mais (o estouro do Scid, que guarda o valor em um byte). O
meio-lance exato volta de `hits_in_game`, que recalcula as ocorrências da
partida carregada.

# Máscaras de árvore (`.stm`)

Arquivo de texto com as máscaras da janela da árvore do Scid vs PC (cores, NAGs,
marcadores e comentários por lance e por posição). Independe de qualquer base.
O MyChess lê e grava **este mesmo formato** (nada de formato próprio, D1800 de
`docs/specs/m5-4-mascaras-de-arvore.md`).

- **Origem no Scid** (relativa a `reference/scid_vs_pc-4.27/`, somente leitura):
  `tcl/windows/tree.tcl`: gravação em `::tree::mask::save` (1957-1965), leitura
  em `::tree::mask::open` (1822-1857, `source $filename` + `array set mask
  $maskSerialized`), estrutura em 1779-1818 e nos acessores 2078-2341, chave em
  `toShortFen` (2544-2547), valores dos marcadores em `marker2image` (1804-1816);
  ajuda em `tcl/help/help.tcl` 2242-2373. A serialização é a de uma **lista do
  Tcl** (regras em `generic/tclUtil.c` do Tcl 8.6, `TclScanElement` e
  `TclConvertElement`), que não faz parte da árvore do Scid. O Tcl é distribuído
  sob a licença do Tcl/Tk (tipo BSD); o MyChess estudou só as regras de citação
  de lista e não copiou código do Tcl (D1857).
- **Código no MyChess:** implementado na M5-4a: `crates/mychess-db/src/tree/mask/`
  (`tcl.rs` para a lista do Tcl, `stm.rs` para o arquivo; decisões de
  implementação D1840-D1849). Decisões em D1800 a
  D1816 de [`../specs/m5-4-mascaras-de-arvore.md`](../specs/m5-4-mascaras-de-arvore.md).
- **Situação das evidências:** não havia `tclsh` nem um `.stm` real do Scid na
  máquina de pesquisa. A gramática abaixo vem da leitura do código do Scid e do
  conhecimento das regras de lista do Tcl; **o exemplo da seção 6 foi montado à
  mão** seguindo essas regras (não foi gerado pelo Scid). Os pontos que dependem
  de comportamento exato do Tcl estão marcados com **[a verificar]**; um `.stm`
  real (ou `tclsh`) fecha todos (pergunta 1 da spec).

## 1. O arquivo é um script Tcl de um comando

O Scid grava exatamente isto (`tree.tcl` 1959-1961):

```tcl
puts $f "set ::tree::mask::maskSerialized [list [array get ::tree::mask::mask]]"
```

Ou seja, **uma linha** (mais o `\n` do `puts`; comentários com quebra de linha
dentro de chaves a deixam com várias linhas físicas):

```
set ::tree::mask::maskSerialized <palavra>\n
```

e para ler ele **executa o arquivo como Tcl** (`source`). O MyChess **não
executa Tcl** (D1801): lê o arquivo como texto e aceita só esta forma:

| Parte | Regra de leitura |
|---|---|
| BOM, espaços, quebras de linha e linhas de comentário (`#` no início de um comando) antes do comando | ignorados |
| `set` | obrigatório |
| `::tree::mask::maskSerialized` | obrigatório (aceita também sem o `::` inicial, que o Tcl resolveria igual no espaço global) |
| `<palavra>` | uma palavra Tcl (seção 3): normalmente `{...}` (chaves), às vezes sem chaves com barras invertidas (seção 4.2); aceita também `"..."` |
| espaços, quebras de linha e comentários depois | ignorados |
| qualquer outra coisa depois (outro comando, texto solto) | `Err`: o arquivo **não** é lido (um script Tcl qualquer nunca é interpretado) |

Arquivo vazio, só com espaços ou sem o comando `set`: `Err(NotAMask)`.

## 2. Estrutura lógica (o valor da palavra)

O valor `V` da palavra é uma **lista Tcl** que é a achatada de um `array`
(`array get`): `chave1 valor1 chave2 valor2 ...`. Tem quatro níveis de lista:

```
V        = lista de pares  [ chave valor ]*              (nº par de elementos)
chave    = FEN curta: 4 campos separados por espaço      (string)
valor    = lista de 2 elementos: [ lances comentario ]   (posição)
lances   = lista de 0..n elementos, cada um uma lista de 6 elementos (lance)
lance    = [ san nag cor comentario marcador1 marcador2 ]
```

Ordem dos pares: a do `array get` do Tcl, que é a da tabela *hash* e **não é
especificada** (muda entre versões do Tcl). Quem lê não pode depender dela, e o
MyChess a preserva (D1812).

### 2.1 Chave (a posição)

`toShortFen`: `sc_pos fen` (`FEN_ALL_FIELDS`, `src/position.cpp`
`PrintFEN` 3013-3069) sem os dois últimos campos (`lreplace $fen end-1 end`),
portanto **quatro campos**: `<tabuleiro> <lado> <roque> <en passant>`, p.ex.
`rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -`.

- Lado: `w` ou `b`. Roque: `-` ou subconjunto ordenado de `KQkq`.
- En passant: `-`, **ou a casa só quando um peão adversário está ao lado do
  peão que avançou duas casas** (`position.cpp` 1919-1936, igual ao
  `EnPassantFen::Adjacent` do `mychess-core`), mesmo que a captura seja ilegal.
- Os contadores (lance e meio-lance) **não** fazem parte da chave: a máscara
  vale para qualquer sequência que leve à mesma posição (transposições).
- Xadrez padrão. O Scid não escreve máscaras de Chess960 de forma que valha a
  pena preservar; chave que não seja uma FEN curta válida é mantida como
  texto opaco (seção 5).

### 2.2 Valor da posição

Lista de **dois** elementos (`tree.tcl` 2086, 2092, 2303):

| Índice | Conteúdo |
|---|---|
| 0 | `lances`: lista de lances da posição (pode ser vazia, `{}`) |
| 1 | comentário da posição (texto livre, pode ser vazio, `{}`; o Scid faz `string trim` ao gravar) |

Uma posição **sem lances e sem comentário** é apagada pelo Scid em
`removeFromMask` (2117-2119); uma posição só com comentário é válida.

### 2.3 Lance (seis elementos, nesta ordem)

Criado vazio por `addToMask` como `[list $move {} $defaultColor {} {} {}]`
(`tree.tcl` 2091, `defaultColor = white`).

| # | Campo | Conteúdo | Observações |
|---|---|---|---|
| 0 | `san` | lance em **SAN inglesa** (`KQRBN`, mesmo com a tradução de peças ligada, 1776), com `x`, `=` e, se vier de partida, sufixo `+` ou `#` | ver 2.4 |
| 1 | `nag` | `""`, ou um de `!!`, `!`, `!?`, `?!`, `?`, `??`, `~`, com ou sem um **espaço antes** quando tem um só caractere (`" !"`, `" ?"`, `" ~"`) | ver 2.5 |
| 2 | `cor` | nome de cor do Tk; padrão `white` | ver 2.6 |
| 3 | `comentario` | texto livre, pode ter várias linhas; vazio = `{}` | `string trim` ao gravar (2261) |
| 4 | `marcador1` | nome de imagem do Tk, ou `""` | ver 2.7 |
| 5 | `marcador2` | idem | |

O Scid lê com `lindex`, então elementos que faltam valem `""` (lance com menos
de 6 elementos é lido como se completado com vazios) e elementos além do sexto
são ignorados.

### 2.4 O campo `san`

- **O sentinela `null`**: ao preencher a máscara com uma partida ou com a base
  (`feedMask`, 2489-2539) o Scid cria, na posição inicial da partida, um lance
  chamado `null` (`previousMoveNT` vazio vira `null`, 2498-2500), para ter onde
  guardar o comentário da posição. Aparece nos arquivos reais; a janela da árvore
  e a exibição da máscara o escondem (`tree.tcl` 851, 2688).
- **Sufixos.** A origem do texto muda o sufixo:
  - lances vindos de partida (`sc_game info previousMoveNT`) usam
    `SAN_MATETEST`: `+` para xeque, **`#` para mate** (`position.cpp`
    2229-2234);
  - lances escolhidos na linha da árvore usam o `san` do nó, com `SAN_CHECKTEST`
    (só `+`, também no mate; `tkscid.cpp` 15360);
  - lances escolhidos no submenu "adicionar lance" vêm de `sc_pos matchMoves`,
    que **remove `x`, `=`, `+` e `#`** (`tkscid.cpp` 11638-11720): entram como
    `Nf7` em vez de `Nxf7`, `e8Q` em vez de `e8=Q`, e o roque como `OK`
    (curto, convertido para `O-O` no menu) ou `OO` (um defeito: o `OO`
    do roque longo não é convertido).
  - A busca de um lance no Scid é `lsearch -regexp $moves "^$move\\+* "`: o
    lance a procurar é usado como **expressão regular** (o `+` de `Qh5+`
    vira quantificador, o `O-O` é literal) e o campo aceita zero ou mais `+`
    depois do texto. Na prática `Qh5` e `Qh5+` se encontram, mas `Qxf7#` guardado
    **não** é achado pela árvore, que procura `Qxf7+`. O MyChess não copia essa
    busca (D1805).
- Sempre inglês, sem número de lance e sem anotação (`!`, `?`) no texto.

### 2.5 O campo `nag`

Texto de zero a dois caracteres. O Scid escreve:
- pelo menu de contexto: `!!`, ` !`, `!?`, `?!`, ` ?`, `??`, ` ~` (os de um
  caractere **com um espaço antes**, `tree.tcl` 2058) ou `""` ("nenhum");
- pelo preenchimento (`feedMask` 2513-2529): o texto de `sc_pos getNags` sem
  espaço (`!`, `!!`, `!?`, `?!`, `??`, `~`; **o `?` nunca é gravado**, porque
  ficou de fora da lista `stdNags`, 2487: defeito do Scid, D1824).
Na leitura o Scid completa os de um caractere com o espaço (`getNag`, 2208).
O espaço, portanto, é só enfeite de colunas: o MyChess lê com ou sem ele e
grava sem (D1807).

### 2.6 O campo `cor`

Nome de cor do Tk que vira `-background` de um quadradinho na linha
(`tree.tcl` 735). Valores que o Scid escreve: `white` (o padrão, em minúsculas,
`defaultColor`) e, pelo menu, `White`, `Green`, `Yellow`, `Blue`, `Red`
(`tree.tcl` 2051-2053; maiúscula inicial). O Scid trata `white` e `White` como
"sem cor" (734). O Tk aceitaria qualquer nome de cor ou `#rrggbb`; o MyChess
preserva valores desconhecidos (D1808).

### 2.7 Os campos `marcador1` e `marcador2`

Guardam o **nome da imagem do Tk** (`marker2image`, `tree.tcl` 1804-1816), não
um código:

| Marcador (menu) | Valor gravado no arquivo | Observação |
|---|---|---|
| Include (incluir) | `::rep::_tb_include` | imagem de `tcl/tools/reper.tcl` |
| Exclude (excluir) | `::rep::_tb_exclude` | idem; o treino do Scid o usa |
| MainLine (linha principal) | `::tree::mask::imageMainLine` | |
| Bookmark | `tb_bkm` | |
| White | `::tree::mask::imageWhite` | |
| Black | `::tree::mask::imageBlack` | |
| NewLine | `tb_new` | |
| ToBeVerified | `tb_rfilter` | |
| ToTrain | `tb_msearch` | |
| Dubious | `tb_help_small` | máscaras antigas têm `tb_help` (1803) |
| ToRemove | `tb_cut` | |
| nenhum | `""` | |

Um nome que o Scid não conhece faz o `image create` falhar em silêncio e
mostra a imagem vazia (`tree.tcl` 726-730); na exibição da máscara (2726) um
nome desconhecido gera uma caixa de erro "possivelmente uma máscara antiga".

## 3. Regras de lista do Tcl (leitura)

Vale para todos os níveis: a palavra do `set` e cada lista aninhada são
analisadas com as mesmas regras (`Tcl_SplitList`, `TclFindElement`).

1. **Espaço em branco** entre elementos: espaço, `\t`, `\n`, `\v`, `\f`, `\r`
   (qualquer quantidade, inclusive no início e no fim).
2. Elemento que começa com **`{`**: vai até a `}` correspondente, contando as
   chaves aninhadas; **uma `\` pula o caractere seguinte** na contagem (então
   `\{` e `\}` não contam). O conteúdo é copiado **sem alteração** (nenhuma
   substituição por barra invertida), exceto `\` + quebra de linha, que o Tcl
   troca por um espaço (só aparece em arquivo editado à mão; a gravação do Scid
   nunca produz isso dentro de chaves). Depois do `}` tem de vir espaço em
   branco ou o fim da lista; senão é erro ("elemento entre chaves seguido de
   lixo").
3. Elemento que começa com **`"`**: vai até o próximo `"` não escapado; com as
   substituições da regra 4. O Scid não grava assim, mas o Tcl aceitaria.
4. Qualquer outro elemento ("palavra nua"): vai até o próximo espaço em branco e
   passa pelas **substituições com `\`**: `\a` (7), `\b` (8), `\f`, `\n`, `\r`,
   `\t`, `\v`, `\\`, `\<LF><espaços e tabs>` (vira um espaço), `\ooo` (até 3
   dígitos octais), `\xHH...` (hexadecimal, vale o byte baixo), `\uHHHH` (até 4)
   e `\UHHHHHHHH` (até 8) como ponto de código Unicode, e `\` seguida de
   qualquer outro caractere vira esse caractere (assim `\{`, `\}`, `\ `, `\[`,
   `\$`, `\;`, `\"` viram o caractere sem a barra). Uma `\` no fim da palavra
   fica como `\`. **Não há** substituição de `$` nem de `[`: na palavra do `set`
   o Tcl as faria, mas o Scid as protege com barra invertida (seção 4.2), e o
   MyChess não as executa.
5. Chaves `{}` sem nada dentro são o elemento **vazio** (`""`).

Cada nível é lido de novo a partir do **texto** do nível acima (é assim que
`lindex` e `foreach` trabalham), portanto as barras invertidas são decodificadas
**uma vez por nível**, e um texto com `\\` num nível interno aparece como `\\\\`
no externo quando a forma sem chaves é usada.

## 4. Gravação (regras de lista do Tcl)

O Scid chama `list` sobre `[array get ...]`, e o Tcl monta cada lista aninhada
do mesmo jeito: cada elemento passa por `TclScanElement` (decide a forma) e
`TclConvertElement` (escreve), e os elementos são juntados por **um espaço**.
O MyChess implementa o mesmo algoritmo, nível a nível, de dentro para fora:
`lance` (6 elementos) -> `lances` -> `valor` (2) -> `V` (pares) -> palavra do `set`.

### 4.1 Escolha da forma de um elemento `s`

1. `s` vazio: `{}`.
2. Percorra `s` caractere a caractere, com `nivel = 0`, `precisa_aspas = falso`,
   `precisa_barra = falso`:
   - `{`: `nivel += 1`; `precisa_aspas = verdadeiro`.
   - `}`: `nivel -= 1`; se `nivel < 0`, `precisa_barra = verdadeiro`;
     `precisa_aspas = verdadeiro`.
   - `\`: se é o **último** caractere, ou o próximo é uma **quebra de linha**,
     `precisa_barra = verdadeiro`; senão **pule o próximo caractere** (ele não
     conta nas chaves) e `precisa_aspas = verdadeiro`.
   - espaço, `\t`, `\n`, `\v`, `\f`, `\r`, `[`, `]`, `$`, `;`, `"`:
     `precisa_aspas = verdadeiro` (**[a verificar]** se `]` e `"` sozinhos, sem
     os outros, já obrigam; por segurança o MyChess os trata como obrigatórios
     e o resultado é igual para o leitor do Scid).
   - Ao final, `nivel != 0` também força `precisa_barra`.
3. Se `s` começa com `#` e é o **primeiro elemento** da lista, `precisa_aspas =
   verdadeiro` (o Tcl 8.5 e posteriores protegem comentário; ex.: lance `#`
   nunca ocorre, mas um comentário no início de lista sim).
4. Escolha:
   - nem `precisa_aspas` nem `precisa_barra`: o texto **cru** (palavra nua);
   - `precisa_aspas` e não `precisa_barra`: **`{` + s + `}`**;
   - `precisa_barra`: **forma com barras** (4.2).

### 4.2 Forma com barras (só quando as chaves não servem)

Cada caractere de `s` vira: `\` + o caractere para `{ } [ ] $ ; " \` e espaço
(e para `#` no início do primeiro elemento); `\n` para quebra de linha, `\t`
para tabulação, `\r`, `\f` e `\v` para os respectivos; qualquer outro, ele
mesmo. Isso acontece, por exemplo, para um comentário que **termina em `\`**
ou tem **`\` seguida de quebra de linha**. Chaves **não** pareadas num
comentário **não** forçam esta forma, porque o `\{` e o `\}` que o nível do
comentário já escreveu são pulados na contagem dos níveis de cima (exemplo B
da seção 6).

### 4.3 A palavra do `set`

`V` passa pela mesma escolha (4.1). Como `V` quase sempre tem espaços e chaves
pareadas, vira `{` + V + `}`. O comando final é

```
set ::tree::mask::maskSerialized <palavra>\n
```

sem espaço no fim e com **um** `\n` (o do `puts`).

### 4.4 Fidelidade da gravação

- **Ordem dos pares**: a do arquivo lido; posições novas no fim, na ordem de
  inserção (D1812). O Scid grava na ordem da tabela *hash*, então dois arquivos
  com o mesmo conteúdo podem diferir em bytes; o leitor do Scid não liga para a
  ordem.
- **Ida e volta byte a byte** vale para arquivo em forma canônica (o que o
  próprio Scid grava): ler e gravar sem editar devolve os mesmos bytes (com a
  ressalva dos itens **[a verificar]** de 4.1 e da codificação, seção 5).
  Arquivos de outras origens (espaços sobrando, `"..."`, `\x`) são lidos e
  gravados na forma canônica.

## 5. Codificação e finais de linha

- O Scid grava (`open w` + `puts`) e lê (`source`) na **codificação do
  sistema** do Tcl: UTF-8 no Linux e no macOS; no Windows, a página de código
  do sistema (Windows-1252 no Tcl 8.6 antes do modo UTF-8 do Windows 10). Sem
  conversão explícita (`grep encoding tcl/` não acha nada além de ajuda). Só
  importa para comentários com acentos.
- **Leitura no MyChess** (D1813): remove o BOM UTF-8; se o arquivo é UTF-8
  válido, usa; senão detecta (`chardetng`) e converte com `encoding_rs`
  (Windows-1252/Latin-1 são os esperados). `\r\n` e `\r` viram `\n` nos
  comentários (o `source` do Tcl faz o mesmo, tradução automática de linha).
- **Escrita**: UTF-8 **sem BOM**, final de linha `\n`.
- Os nomes de campo (SAN, `white`, nomes de imagem, NAGs) são ASCII.

## 6. Exemplos

### 6.A Arquivo com três posições (montado à mão, forma canônica)

Conteúdo lógico:

| Chave (FEN curta) | Lances `[san nag cor comentário m1 m2]` | Comentário da posição |
|---|---|---|
| `rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -` | `[null "" white "" "" ""]`, `[e4 ! Green "King's pawn" ::rep::_tb_include ""]`, `[d4 "" white "" "" ""]` | `My repertoire` |
| `rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -` | `[e5 "" white "" "" ""]`, `[c5 ?! Red "Sicilian: see line 2" ::rep::_tb_exclude ::tree::mask::imageMainLine]` | (vazio) |
| `rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq -` | `[Nf3 !? Yellow "Line one<LF>second line" tb_bkm ""]` | (vazio) |

Arquivo (o comentário `Line one` / `second line` tem uma quebra de linha
real, por isso o arquivo tem duas linhas físicas; termina com `\n`):

```
set ::tree::mask::maskSerialized {{rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -} {{{null {} white {} {} {}} {e4 ! Green {King's pawn} ::rep::_tb_include {}} {d4 {} white {} {} {}}} {My repertoire}} {rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq -} {{{e5 {} white {} {} {}} {c5 ?! Red {Sicilian: see line 2} ::rep::_tb_exclude ::tree::mask::imageMainLine}} {}} {rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq -} {{{Nf3 !? Yellow {Line one
second line} tb_bkm {}}} {}}}
```

Derivação, de dentro para fora (a primeira posição):
1. lance `e4`: `e4 ! Green {King's pawn} ::rep::_tb_include {}` (`!`, `Green` e
   `::rep::_tb_include` não têm caractere especial: nus; `King's pawn` tem
   espaço: chaves; vazio: `{}`);
2. `lances` = os três lances, cada um entre chaves (têm espaço) e separados por
   um espaço;
3. `valor` = `lances` (entre chaves, porque tem espaços e chaves) e
   `{My repertoire}`;
4. o par da lista `V` é `{chave}` (espaços: chaves) e `{valor}` (de novo entre
   chaves); por isso aparecem **três chaves de abertura** antes de `null`
   (valor, lances, lance);
5. `V` inteiro vai entre chaves (palavra do `set`), e `\n`.

### 6.B Comentário com chave sem par e com barra no fim

Lance `e4` com o comentário `a { b` (chave de abertura sem par):

```
{e4 {} white a\ \{\ b {} {}}
```

O comentário, sozinho, exige a forma com barras (nível não zero no fim e
espaços): `a\ \{\ b`. Os níveis de cima veem `\{` (pulado) e mantêm as chaves.
Para o comentário `x\` (termina em barra) a forma é `x\\`, que no nível de cima
continua cabendo entre chaves: `{e4 {} white x\\ {} {}}`.

Quando um nível tem **barra seguida de quebra de linha** (comentário `a\`
+ LF + `b`), o comentário vira `a\\\nb` (barra, barra, `\n` escrito com dois
caracteres) e os níveis de cima, que já não têm quebra de linha real, voltam a
usar chaves.

### 6.C Vetores da escolha de forma (entrada -> saída)

| Elemento (`s`) | Saída |
|---|---|
| (vazio) | `{}` |
| `e4`, `O-O`, `Qxf7#`, `e8=Q+`, `!?`, `::rep::_tb_include` | iguais (nus) |
| `King's pawn` | `{King's pawn}` |
| `a { b` | `a\ \{\ b` |
| `a } b` | `a\ \}\ b` |
| `x\` | `x\\` |
| `a$b` | `{a$b}` |
| `{a}` | `{{a}}` |
| `a<TAB>b` | `{a<TAB>b}` |
| `a<LF>b` | `{a<LF>b}` (quebra real dentro das chaves) |
| `#x` como primeiro elemento da lista | `{#x}` |
| `a"b` | **[a verificar]** `{a"b}` (ou `a\"b`) |

## 7. Casos extremos que o leitor trata (D1814)

| Situação | Tratamento |
|---|---|
| Erro de sintaxe de lista em qualquer nível acima da posição (chaves sem par, lixo depois de `}`, `"` sem fechar) | `Err(Syntax { offset })`; nada é carregado |
| `V` com número ímpar de elementos | o último elemento é ignorado, aviso |
| Chave repetida | vale a **última** (como `array set`), aviso |
| Chave que não é FEN curta de 4 campos reconhecível | mantida como texto opaco, nunca casa com posição; aviso |
| Valor com 1 elemento | comentário vazio; com mais de 2, os extras são ignorados |
| Lance com menos de 6 elementos | completado com vazios; com mais, extras ignorados; aviso |
| `san` vazio | lance descartado, aviso |
| `san` repetido na mesma posição (comparação normalizada, D1805) | o segundo é descartado, aviso |
| NAG, cor ou marcador desconhecidos | preservados como texto; a interface os mostra como "desconhecido" |
| Arquivo maior que 256 MiB | `Err(TooLarge)` antes de ler tudo |
| Nenhum pânico, nenhuma recursão sem limite (a estrutura tem 4 níveis fixos), nenhuma alocação maior que o arquivo | exigência da regra de ouro 2 |

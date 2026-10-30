# Formato `.spf` (fotos de jogadores) e fotos soltas

O `.spf` do Scid vs PC é um **script Tcl** executado com `source`; o único comando
que importa é `photo <nome> <dados>` (`tcl/main.tcl` 1049-1057). O MyChess **nunca
executa Tcl**: lê só os comandos `photo`. Não há amostra real na árvore de
referência; a gramática vem do código que consome o arquivo.

- **Código:** `crates/mychess-db/src/spelling/photo.rs`.
- Especificação: `docs/specs/m6-3-jogadores-torneios-spelling.md`, seção J.

## 1. Comandos

```
# comentário
photo "Almeida, Beatriz Costa" {R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7}
photo {Braga, Carlos Eduardo} "R0lGODlh..."; photo Solo R0lGODlh...
```

- Comandos terminam em fim de linha ou `;`. Uma barra invertida antes do fim de
  linha continua o comando. `#` no começo de um comando é comentário.
- Palavras: entre aspas duplas (com `\"`), entre chaves (aninhadas) ou soltas.
  Nada é interpretado (`$x` e `[cmd]` são texto).
- `photo` com três palavras: nome e dados em base64 padrão (alfabeto
  `A-Za-z0-9+/`, preenchimento `=`; espaços e quebras são ignorados). Outro
  número de palavras, base64 inválido, dado que não é imagem ou maior que 4 MiB:
  ignorado e contado (`SpfReport::invalid`). Qualquer outro comando é ignorado e
  contado (`ignored_commands`), nunca executado.
- A imagem é validada pela assinatura: GIF (`GIF87a`/`GIF89a`), PNG ou JPEG.
- Limites: arquivo até 256 MiB; foto até 4 MiB decodificada; até 200 000 fotos.

## 2. Chave da foto

`photo_key(nome, spelling)`: nome corrigido (`correct`) se houver verificador,
minúsculas (Unicode simples), sem o prefixo `deep `, sem espaços. A mesma função
vale na carga e na consulta. Consulta sem vírgula e com mais de 2 caracteres: se a
chave inteira não existe, o maior prefixo (de pelo menos 3 caracteres) que seja
chave.

## 3. Fotos soltas e prioridade

`<pasta>/<Nome do jogador>.gif|png|jpg|jpeg` (extensão sem diferenciar a caixa; o
nome do arquivo sem extensão é o nome do jogador). `PhotoLibrary::load_dir` lê
primeiro os `.spf` em ordem alfabética (o último vence) e depois as imagens soltas,
que substituem as dos `.spf`. Arquivo ilegível ou que não é imagem é contado em
`errors` e ignorado. A normalização Unicode das chaves (macOS grava nomes em NFD)
não é feita (sem dependência nova): é uma limitação conhecida.

# Formato `.ssp` (arquivo de *spelling*)

Arquivo de texto orientado a linhas com os nomes corretos de jogadores,
eventos, locais e rodadas, apelidos (grafias erradas), comentário do jogador
(título, país, pico, nascimento e morte), biografia, Elo por período e regras
gerais de substituição. Serve à correção ortográfica de nomes, ao título e país
dos jogadores e ao "Adicionar Elo". O `spelling.ssp` do Scid vs PC **não** faz
parte do repositório.

- **Origem no Scid** (relativa a `reference/scid_vs_pc-4.27/`): `src/spellchk.cpp`
  (`ReadSpellCheckFile` 442-567, `getNameAndComment` 418-440, `AddPrefixSuffix`
  569-612, `AddBioData` 614-629, `AddEloData` 727-791, `GetElo` 810-886,
  `GetTitle`/`GetLastCountry`/`GetPeakRating`/`GetBirthdate`/`GetDeathdate`
  965-1071), `src/namebase.cpp` (`NameTypeFromString` 462-480), `src/mfile.cpp`
  (`ReadLine`, `.gz`).
- **Código no MyChess:** `crates/mychess-db/src/spelling/` (`parse.rs` a gramática,
  `comment.rs` o comentário do jogador, `elo.rs` o `%Elo`, `match.rs` as
  consultas). Especificação: `docs/specs/m6-3-jogadores-torneios-spelling.md`.
- Dados de teste: `crates/mychess-db/tests/fixtures/m6_3a/ssp-exemplo.ssp`
  (sintético, nomes fictícios).

## 1. Texto e linhas

1. Bytes: BOM UTF-8 removido; UTF-8 se válido; senão `chardetng`/`encoding_rs`
   (Windows-1252 quando há pouca evidência, menos de 32 bytes fora do ASCII).
   Um fluxo gzip (`1f 8b`, `.ssp.gz`) é descompactado antes; o limite é
   1 GiB depois de descompactar (`MAX_FILE_BYTES`).
2. Fins de linha `\n`, `\r\n` e `\r`; a última linha não precisa de `\n`.
3. Linha com mais de 64 KiB (`MAX_LINE_BYTES`) é ignorada e contada
   (`LoadReport::skipped_lines`). O Scid lê blocos de 1023 bytes e trata os
   pedaços como linhas (defeito).
4. Cada linha se divide no primeiro `#`: `parte_nome` (aparada nas duas pontas
   de espaço, tab, CR, LF) e `comentário` (aparado só à direita; o espaço
   depois do `#` fica).

## 2. Classe da linha (primeiro caractere da `parte_nome`)

| Início | Significado |
|---|---|
| (vazia) | ignorada |
| `>` | biografia antiga, ignorada |
| `@` | seção (abaixo) |
| `%` | diretiva (abaixo) |
| `=` | apelido do último nome correto lido do tipo da seção |
| outro | nome correto do tipo da seção |

**Seção** `@TIPO ["excluídos"]`: o texto depois do `@` escolhe o tipo
(`player`, `event`, `site` ou `round`; qualquer caixa, prefixos aceitos como
`@P`, `@pl "x"`; espaços iniciais ignorados). Tipo desconhecido: a seção não
tem dono e suas linhas são ignoradas, sem erro. `"excluídos"` é o conjunto de
caracteres removidos antes de comparar nomes e consultas (`"., -_*"` para
jogadores). Aspas de abertura sem a de fechamento num tipo válido é o único
erro de sintaxe (`SpellingError::UnclosedQuote`). Um tipo pode ter várias
seções; nomes e regras se acumulam.

**Nome correto**: `nome` é a `parte_nome` inteira, `chave` é o nome sem os
excluídos, `comentário` o do jogador. **Apelido**: `chave` do texto depois do
`=` (espaços iniciais removidos); herda nome e comentário do último nome
correto; sem nome correto antes é ignorado.

**Diretivas** (caixa exata e **um espaço** depois do nome; `%Bio` sozinho é
ignorado):

| Diretiva | Efeito |
|---|---|
| `%Bio texto` | uma nota; até 1000 por nome (`MAX_BIO_NOTES`), as demais são descartadas e contadas |
| `%Elo ano:v1,v2,... ...` | Elo por período (seção 4); até 64 linhas por nome |
| `%Render texto` | guardada, sem uso (como no Scid) |
| `%Prefix "a" "b"`, `%Suffix ...`, `%Infix ...` | regra de substituição do tipo da seção atual; precisa das quatro aspas; `a` não pode ser vazio (o Scid tem comportamento indefinido, a regra é ignorada); a regra definida por último é testada primeiro |

`%Bio`, `%Elo` e `%Render` valem para o último nome correto lido **de cada tipo**,
em qualquer seção (o Scid lê o arquivo uma vez por tipo e faz o mesmo; o MyChess
lê uma vez só e dá o mesmo resultado).

## 3. Comentário do jogador

```
TÍTULOS PAÍSES [PICO] NASCIMENTO[--MORTE] [SINALIZADORES]
GM RUS [2812] 1963.04.13 i
- ENG/NZL [....] 1904.??.??--1979.08.15
cim URS/RUS [2520*] 1911.06.14--2004.02.29
```

- `TÍTULOS`: `-` ou códigos separados por `+` (`gm im fm cm wgm wim wfm wcm cgm
  cim csm clg clm hgm comp`, e `w` sozinho), qualquer caixa. Para a busca vale o
  primeiro código (`Title::from_scid_str`; `hgm` = GM).
- `PAÍSES`: códigos de 3 letras separados por `/`, o mais antigo primeiro; o país
  atual são os 3 últimos caracteres do campo (vazio se tiver menos de 3).
- `PICO`: inteiro depois do primeiro `[` (`2520*`: estimativa; `....` = 0).
- `NASCIMENTO` e `MORTE`: `AAAA[.MM[.DD]]`, partes `?` ou ausentes = desconhecidas;
  a morte vem depois do primeiro `-`. O quinto campo é ignorado.
- Comentário malformado: cada campo que falta fica vazio; nada falha.

## 4. `%Elo`

`%Elo 2013:2300,2301,?,2310 2001:2200,2210,2220,2230`

- Grupos `ANO:valores` separados por espaço; valor = 1 a 4 dígitos ou `?` (0).
  Anos fora de `[1970, ano corrente]` e valores além do 12º são descartados;
  caractere inesperado interrompe a leitura **da linha**, conservando o que veio
  antes. Uma segunda linha só sobrescreve as posições que define.
- A posição do valor é a do **período**, não o mês:

| Anos | Posições | Mês → posição (jan..dez; mês desconhecido = 0) |
|---|---|---|
| ≥ 2013 | 12 | 0 1 2 3 4 5 6 7 8 9 10 11 |
| 2012 | 9 | 0 0 1 1 2 2 3 4 5 6 7 8 |
| 2010-2011 | 6 | 0 0 1 1 2 2 3 3 4 4 5 5 |
| 2009 | 5 | 0 0 0 1 1 1 2 2 3 3 4 4 |
| 2001-2008 | 4 | 0 0 0 1 1 1 2 2 2 3 3 3 |
| 1970-2000 | 2 | 0 0 0 0 0 0 1 1 1 1 1 1 |

- `elo_at(nome, data, exato)`: só o nome correto exato (apelido não tem Elo); lê a
  posição do período; se for 0, a primeira posição do ano; se ainda for 0, mês
  desconhecido e `exato` falso, a primeira não nula do ano.
- `elo_history`: pares `(ano + posição*100/n/100, Elo)`, `n` = posições do ano.

## 5. Consultas

Chave da consulta = nome sem os excluídos; vazia = nenhum casamento. Uma entrada
casa quando a chave da consulta é **prefixo** da chave dela (caixa exata). Ordem
do Scid: do último para o primeiro do arquivo. `correct`: a exata de maior índice
ou, sem exata, a de **menor** índice. `corrections`: nomes corretos sem repetição,
ou só o da exata. `comment_exact` e o Elo só olham nomes corretos. No MyChess as
entradas ficam num vetor ordenado por chave, então a consulta custa
`O(log n + k)`.

## 6. Diferenças em relação ao Scid

Títulos em maiúsculas reconhecidos; última linha sem `\n` lida; linhas até 64 KiB;
`@pl "x"` aceito; um passe em vez de quatro; `%Elo` com valores de 1 a 4 dígitos;
morte só com ano não bloqueia partidas do ano; `.sn4` regravado com as
frequências recontadas ao aplicar correções.

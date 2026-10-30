# ADR 0012: Tablebases: crate `mychess-tablebase`, Syzygy local com `shakmaty-syzygy` e consulta online do Lichess

- **Status:** aceita pelo orquestrador; revertível pelo dono. Especificação: `docs/specs/m8-8-tablebases.md` (D2550-D2599).

## Contexto

O Scid vs PC mostra, numa janela, o resultado de cada lance de uma posição de final (vitória, empate, derrota e distância ao mate) a partir de tablebases **Nalimov** locais (até 5 peças) ou de consultas online (Lichess, Shredder, K4it). O formato Nalimov é obsoleto, tem licença restrita e o código do Scid que o lê (`probe.cpp`, `egtb/`) não pode ser portado (`docs/referencia-scid.md`). O roteiro (M8) já previa o crate `mychess-tablebase` e a ADR 0003 já escolhera o `shakmaty-syzygy`. Esta ADR fecha os detalhes: licença, `unsafe`, camada, rede e custo.

## Decisão

1. **Criar o crate `mychess-tablebase`** (camada Model, abaixo do `mychess-app`), dependendo de `mychess-core`, `mychess-online`, `shakmaty` e `shakmaty-syzygy`. Contém: tipos de resultado próprios (`Category`, `Evaluation`, `MoveEntry`, `PositionReport`), a *trait* `PositionProbe` (síncrona; local e falsos de teste), a *trait* `OnlineProbe` (assíncrona), `LocalTablebase` (abre pastas de `.rtbw`/`.rtbz`), `OnlineTablebase` (Lichess, com cache) e um gerador de posições de treino. A GUI e o `app` nunca veem tipos do `shakmaty-syzygy`.
2. **Syzygy no lugar do Nalimov.** Dá WDL (com a regra dos 50 lances) e DTZ, até 7 peças. **Não dá DTM**; a interface mostra DTZ e só mostra DTM quando a fonte online o fornece (D2554). O MyChess não calcula DTM.
3. **Usar o cliente HTTP do `mychess-online`** em vez de criar outro: ganha-se o `RequestGate` (uma requisição por vez, bloqueio e *backoff* em 429), o limite de corpo, o `https` obrigatório sem redirecionamento, o provedor `ring` do `rustls` e o servidor falso para testes. Acrescenta-se ao `mychess-online` **um método genérico** `Client::get_public` (GET sem token, corpo limitado, com o `RequestGate`). `mychess-tablebase` passa a poder depender de `mychess-online` (`ALLOWED_DEPS`).
4. **Sem a feature `mmap`** do `shakmaty-syzygy` (ver "`unsafe`" abaixo).
5. Consulta online **desligada por padrão**, ativada pelo usuário; é ajuda externa, então fica na família de assistência bloqueada em partida online (ADR 0010, D2570).

## Licença (verificada no registry local)

- `~/.cargo/registry/src/*/shakmaty-syzygy-0.28.1/Cargo.toml`: `license = "GPL-3.0-or-later"`; o `README.md` do pacote diz "licensed under the GPL-3.0 (or any later version at your option)". O pacote publicado não traz arquivo `LICENSE` (o aviso está no manifesto e no README); o mesmo vale para `shakmaty` 0.30.2, já usado. Compatível com a licença do MyChess (GPL-3.0-or-later, ADR 0004), que já cita o crate.
- O código de sondagem é "estreitamente baseado" na implementação de Ronald de Man para o Stockfish (agradecimento no README); essa herança é responsabilidade do autor do crate, que a publica sob a GPL.
- **Dependências transitivas** (todas já em `Cargo.lock` desde o `shakmaty`/`tracing`): `arrayvec` 0.7.8 (MIT OR Apache-2.0), `bitflags` 2.13.2 (MIT OR Apache-2.0), `byteorder` 1.5.0 (Unlicense OR MIT), `once_cell` 1.21.4 (MIT OR Apache-2.0), `rustc-hash` 2.1.3 (Apache-2.0 OR MIT), `tracing` 0.1.44 (MIT), `libc` 0.2.191 (só em `cfg(target_os = "linux")`; MIT OR Apache-2.0), `shakmaty` 0.30.2 (GPL-3.0-or-later). Todas constam da lista `allow` de `deny.toml`. **Nenhuma entrada nova em `deny.toml` nem em `Cargo.lock`** além da do próprio `shakmaty-syzygy` e do crate interno.
- Licença dos dados: os arquivos `.rtbw`/`.rtbz` são do usuário e **não entram no repositório** (D2561). Os testes com tabelas reais leem uma pasta de `MYCHESS_TEST_SYZYGY`.

## `unsafe` e *mmap*

- O `shakmaty-syzygy` tem `unsafe` em dois lugares, ambos opcionais: `Tablebase::with_mmap_filesystem` (feature `mmap`, exige `unsafe` do **chamador**, porque uma alteração do arquivo ou um erro de E/S durante a leitura gera `SIGBUS`/`SIGSEGV`) e `OsFilesystem::with_advise_random` (`libc::posix_fadvise`, só no Linux). O MyChess **não ativa `mmap` e não chama `with_advise_random`**: usa `Tablebase::new()` (`OsFilesystem`, leitura por `pread`/`seek`), que é seguro. O código novo do MyChess não tem `unsafe` e o lint do workspace continua `deny`. **Nenhuma exceção à regra de `desempenho-e-unsafe.md` é pedida.**
- Custo: sem *mmap* cada sondagem faz algumas chamadas de sistema de leitura. Para o uso interativo (uma posição por vez, mais os lances dela) é aceitável; se um uso futuro em laço (adjudicação de torneio, análise) pedir mais, mede-se e reabre-se a decisão com a regra de `unsafe` (exigiria o arquivo aberto somente para leitura e o descarte antes de qualquer escrita, como no `.sg4`).
- O `shakmaty-syzygy` assume arquivos íntegros; a tabela corrompida devolve `CorruptedTable`, mas há caminhos com `expect`/aritmética que podem entrar em *panic*. Cada sondagem roda dentro de `catch_unwind` e o erro vira `TablebaseError::Internal` (regra de ouro 2: entrada externa nunca causa *panic*).

## Tamanho do binário e tempo de compilação

O `shakmaty-syzygy` tem ~3 700 linhas e só as dependências pequenas acima; o crate `mychess-online` (e com ele `reqwest`/`rustls`) já faz parte do `mychess-app`. Estimativa: dezenas de KB no binário e poucos segundos de compilação. Medir com `cargo build --timings` se o tamanho incomodar.

## Alternativas consideradas

- **Cliente HTTP próprio no novo crate (`reqwest` direto):** duplica o TLS, o *gate*, os limites e o servidor falso; recusado (o pedido era reaproveitar o comum).
- **`mychess-tablebase` só com o `core` (sem `mychess-online`) e o cliente online dentro do `mychess-app`:** mantém o crate sem rede, mas põe rede no controlador e dificulta o teste isolado (ADR 0010 recusou o mesmo para o Lichess).
- **Cliente online dentro do `mychess-online`:** possível, mas o `mychess-online` é o Board API (autenticado); a tablebase é outro serviço, outro host e outro uso (ajuda, não jogo). Mantém-se um crate por assunto.
- **`mmap`:** mais rápido, mas `unsafe`, com risco de `SIGBUS` com o arquivo em disco de rede ou removível; não há necessidade medida.
- **Manter Nalimov:** formato obsoleto, licença restrita, sem 6-7 peças; recusado.
- **Calcular DTM por busca sobre o WDL:** fora de escopo; deixado como gancho.

## Consequências

- Nova linha em `ALLOWED_DEPS` (`mychess-tablebase` → `mychess-core`, `mychess-online`) e `mychess-app` passa a depender do novo crate. A tabela de `docs/arquitetura.md` registra a mudança (a coluna "Pode depender de" do `mychess-tablebase` deixa de ser só `core`).
- O `mychess-online` ganha `get_public`, que não é específico do Board API; o contrato dele (https, sem redirecionamento, limite de corpo, *gate*) vale para qualquer host público.
- A tablebase é mais uma **família de assistência** (`Family::Tablebases`) no bloqueio de partidas online.
- Os arquivos de tabela têm de ser baixados pelo usuário (espelho do Lichess: `tablebase.lichess.ovh/tables`); a interface explica e nunca os baixa sozinha.
- Limitação conhecida: o crate só reconhece as extensões em minúsculas.

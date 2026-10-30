# ADR 0010: Cliente online (`mychess-online`): Lichess pelo Board API, reqwest + rustls

- **Status:** aceita pelo orquestrador; revertível pelo dono. As partes marcadas "a confirmar" dependem do `cargo deny` e do CI nas três plataformas (ver `docs/specs/m8-3-lichess-online.md`, R4 e P2).

## Contexto

O M8 prevê jogar online. O usuário escolheu o **Lichess** primeiro e o FICS depois. É o primeiro código do projeto que fala com a rede e o primeiro que carrega um token de acesso. O roteiro e `docs/arquitetura.md` já previam o crate `mychess-online` (camada Model, abaixo de `mychess-app`).

Fatos da API (lidos da especificação OpenAPI oficial em 2026-10-24):

- O **Board API** serve clientes de terceiros com contas comuns e diz, na própria descrição, que ajuda de engine ou qualquer ajuda externa é proibida. Só tempo real *rapid* e *classical* (e *blitz* em desafios diretos) e correspondência.
- O **Bot API** é outro conjunto de endpoints, só para contas Bot (conversão irreversível), onde a engine é permitida.
- As regras de *fair play* do Lichess proíbem engine, livros, tablebases e recomendação de lances durante a partida; os meios legais de jogar são as APIs oficiais.
- Política de uso: uma requisição por vez; em 429 esperar ao menos um minuto; um *event stream* por token; tokens pessoais ou OAuth com PKCE (`S256`).

## Decisão

- Criar o crate **`mychess-online`** (Model), dependendo só de `mychess-core`, com: modelos tolerantes da API, enquadramento de NDJSON, cliente HTTP com limite de taxa, streams com reconexão, armazenamento do token e um servidor falso para testes. `mychess-app` passa a depender dele; `mychess-gui` não.
- Usar só o **Board API** com conta humana. Contas Bot são recusadas. O app **bloqueia, no `update`, todo recurso de assistência** (engines, árvore, livros, tablebases, buscas por posição, ECO, edição da partida) durante uma partida online como jogador.
- Cliente HTTP: **`reqwest` 0.13** com `rustls` (sem OpenSSL do sistema) e leitura de streams por `Response::chunk()`. JSON com `serde_json`. Token no chaveiro do sistema com `keyring` 4.x, com *fallback* para arquivo 0600 sob consentimento.
- Login v1 por **token pessoal**; PKCE por *loopback* na fatia d (feita em 2026-10-27, spec seção 13): sem crate para abrir o navegador (comando do sistema com argumento separado), SHA-256 e aleatoriedade pelo `ring` (já o provedor do `rustls`, Apache-2.0 AND ISC) e `zeroize` (Apache-2.0 OR MIT) para apagar o token da memória. Observar partidas usa os endpoints públicos, sem token. Bloqueio de assistência inalterado.
- Testes só contra um servidor HTTP falso local; nada de rede real no CI.

## Alternativas consideradas

- **`ureq` 3 (síncrono):** menos dependências assíncronas, mas pede um thread por stream e cancelamento por *timeout*; mais código para o mesmo resultado, fora do padrão `tokio` do workspace.
- **`hyper` direto:** controle total, mas TLS, redirecionamento, compressão e *pool* ficam por nossa conta.
- **TLS nativo (`native-tls`):** usaria OpenSSL no Linux, proibido por `dependencias-e-licencas.md`.
- **Backend `rustls-rustcrypto` (Rust puro):** evita C e a licença OpenSSL do `aws-lc`, mas é mais jovem. Fica como plano B se o `cargo deny` ou o CI do Windows reclamar.
- **Bot API para jogar com engine:** exige conta Bot separada; fora desta decisão (pergunta P1 da spec).
- **Token em arquivo de preferências:** simples, mas expõe o token em cópias e *backups*; recusado.
- **Colocar o cliente dentro de `mychess-app`:** misturaria rede com estado e dificultaria o teste isolado e o reuso pelo FICS.

## Consequências

- Primeira pilha de rede e TLS do projeto (`reqwest`, `hyper`, `rustls`, backend criptográfico, `keyring`): tempo de compilação e tamanho do binário crescem. Vale medir com `cargo build --timings` na fase a.
- `cargo deny` precisa aceitar as licenças da pilha de TLS (possível entrada nova em `deny.toml`, a confirmar com a lista de licenças compatíveis com GPLv3 do FSF).
- O CI das três plataformas passa a compilar código com dependência de C (rustls com `aws-lc-rs`): registrar qualquer pacote de sistema no `README.md` e em `.github/workflows/ci.yml` no mesmo commit.
- O bloqueio de assistência vira regra transversal: toda `Message` nova de uma família assistiva precisa entrar na tabela da spec (seção 4.2) e no teste de varredura.
- O FICS poderá reaproveitar o bloqueio de assistência, o relógio autoritativo e a importação, mas terá cliente próprio (protocolo de texto, não HTTP) no mesmo crate ou em módulo separado.
- Aceitar o Board API significa aceitar também as restrições do Lichess (sem bullet, sem engine, uma partida em tempo real por vez no app).

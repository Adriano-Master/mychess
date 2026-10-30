# ADR 0017: Torneios Arena do Lichess: escopo `tournament:write`, só ritmos do Board API, acompanhamento por *polling* e painel `PaneKind::Arena`

- **Status:** proposta (2026-10-29), aguarda a decisão do dono nas perguntas P1 a P3 da spec `docs/specs/m8-13-lichess-torneios-arena.md`. Estende a ADR 0010 (cliente online) sem substituí-la.

## Contexto

O dono quer participar de uma arena do Lichess organizada por amigos (entrar pelo link, jogar e acompanhar o torneio inteiro) e criar arenas pelo MyChess. A ADR 0010 fixou o Board API com conta humana e escopos `board:play`, `challenge:read` e `challenge:write`. Pesquisa feita em 2026-10-29 na especificação OpenAPI e no código do servidor (`lichess-org/lila`):

- Entrar, pausar, criar, editar e encerrar exigem o escopo **`tournament:write`**, que o app não pede.
- Uma partida de arena só é aceita pelo Board API se o ritmo estimado (`limite + 40 × incremento`) for de **480 s ou mais** (rapid ou mais lento). Arenas blitz e bullet, a maioria das arenas de amigos, não podem ser jogadas pelo MyChess.
- Quem joga só pela API não é emparelhado sozinho: é preciso chamar `join` com `pairMeAsap=true` (vale 70 s e é consumido a cada emparelhamento), dentro de um limite de 30 chamadas por 10 minutos.
- Não existe *stream* de classificação na API pública: o acompanhamento é por consulta periódica de `GET /api/tournament/{id}`.
- Partidas de torneio são "obrigatórias" no servidor: sem aborto, sem *takeback*.

## Decisão

1. **Escopo OAuth opcional.** `tournament:write` entra como quarto escopo, **opcional** (caixa na janela "Conectar", PKCE e link do token pessoal com 4 escopos; botão "Autorizar torneios..." para tokens antigos). Sem ele, o app continua com tudo o que já fazia e só acompanha torneios.
2. **Só se entra em arenas que o Board API joga.** O app calcula `limite + 40 × incremento >= 480` (e variante `standard`/`chess960`, sem *team battle*) e recusa a inscrição nas demais, que ficam disponíveis para acompanhar. Evita ser emparelhado numa partida impossível e perder por `noStart`, prejudicando os adversários.
3. **Pareamento ativo.** Enquanto inscrito, ativo e sem partida, o app registra o usuário na lista de espera do servidor (`join` com `pairMeAsap=true`) 3 s depois de cada partida e a cada 50 s, com teto local de 20 chamadas por 10 minutos. Pausar, sair do app e desconectar enviam `withdraw`.
4. **Exceção documentada à D1308(3) ("nada de *polling*").** O acompanhamento do torneio consulta `GET /api/tournament/{id}` a cada 15 s (60 s durante a própria partida), só com o painel aberto ou o usuário inscrito, sempre pelo `RequestGate` (uma requisição por vez, bloqueio de 429). Partida e eventos continuam sem *polling*.
5. **Painel novo `PaneKind::Arena`** (`"lichess-arena"`), não modal, com três abas (Torneio, Classificação, Partidas). Diálogos modais só para abrir, senha, criar e confirmar. Motivo: o torneio dura horas e o usuário precisa da classificação durante e entre as partidas; um modal cobriria o tabuleiro. O layout salvo é tolerante a nomes desconhecidos (ADR 0008).
6. **Módulo `online/arena/`** no `mychess-app` (não `tournament`, que já é o torneio de engines), com `RequestId` e época por acompanhamento, temporizadores como efeitos com geração e cancelamento cooperativo. O `mychess-online` ganha os modelos, os endpoints e a validação do formulário de criação.
7. **Bloqueio de assistência inalterado** (D1330-D1332). Entre partidas o bloqueio solta; ao `gameStart` as engines param. Enquanto inscrito e ativo, o app recusa iniciar partida contra engine, torneio de engines, jogo tático e treinos que o `gameStart` encerraria (`AppState::arena_engaged()`).
8. **Correção geral:** o `gameStart` passa a respeitar `compat.board`; partidas que o Board API não aceita são ignoradas com aviso (corrige um defeito existente da M8-3).

## Alternativas consideradas

- **Pedir `tournament:write` sempre, sem opção.** Mais simples, mas alarga o token de quem só quer jogar partidas avulsas.
- **Deixar entrar em qualquer arena e jogar no navegador.** Esconde a limitação e leva o usuário a ser emparelhado e perder por não jogar. Recusada.
- **Bot API para jogar arenas blitz/bullet.** Exige conta Bot irreversível e proíbe humano; fora da ADR 0010.
- **Janela modal para o torneio.** Cobre o tabuleiro durante a partida. Recusada em favor do painel.
- **Janela destacada (outra janela do sistema).** Não existe no projeto para painéis de dados (roteiro M9 prevê "painéis destacáveis" depois); o painel acoplável cobre o caso.
- **WebSocket do Lichess para classificação ao vivo.** Não é API pública, muda sem aviso e viola o espírito dos termos de uso. Recusada.
- **Biblioteca de datas com fuso local** para o horário de início. Fica como pergunta ao dono (P2); a v1 usa UTC e tempo relativo.

## Consequências

- Mais um escopo no consentimento do usuário e no `oauth::SCOPES`; os tokens guardados continuam válidos.
- Uma parte das arenas de amigos (as blitz) será só acompanhada; o formulário de criação avisa e sugere 10+0 ou 8+0.
- O app passa a fazer chamadas periódicas ao Lichess durante um torneio (cerca de 4 por minuto no pior caso); o `RequestGate` e os intervalos mantêm o uso dentro da política ("uma requisição por vez", respeito a 429). Calibragem real pendente (spec, R2 e P8).
- `PaneKind` ganha uma variante; o layout antigo continua carregando, o novo layout com `"lichess-arena"` é descartado por versões antigas do app (ignora nomes desconhecidos).
- O comportamento do `gameStart` muda para toda partida não compatível com o Board API (teste de regressão).
- Suíço fica como extensão (spec futura `m8-14`): mesmo escopo, outro conjunto de endpoints, sem lista de espera.

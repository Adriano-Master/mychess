# ADR 0007: Fonte de banco compartilhada (`Arc<RwLock<_>>`) nos slots de banco

- **Status:** aceita pelo orquestrador (2026-10-22); revertível pelo dono do projeto

## Contexto

O `mychess-app` mantém até 9 bancos abertos (8 slots e a Clipbase, M3-4). A
leitura de uma partida de um banco SI4 ou PGN exige `&mut` (buffer interno do
arquivo de partidas), e a leitura de disco não pode acontecer no `update` nem no
`view` (regras de ouro 3 e MVU). Por isso um `Effect` (executado em
`spawn_blocking`) precisa **possuir** a fonte, que é `'static`, enquanto o
estado do app continua sendo o dono lógico do slot.

A regra de estilo (`.claude/rules/rust-estilo.md`) proíbe `Rc<RefCell<_>>` e
`Arc<Mutex<_>>` no domínio, porque estado mutável compartilhado costuma indicar
desenho errado no MVU. Esta ADR registra a exceção, restrita e justificada.

## Decisão

- Existe **um único** `Arc<Mutex<BaseSource>>` (`SharedSource`, em
  `mychess-app::bases`) por slot, e nada mais no domínio usa esse padrão.
- O `update` continua sendo o **único** que altera o conjunto de slots, o
  filtro, a partida atual e a Clipbase. A fonte compartilhada é usada só para
  **leitura** de registros pelos `Effect`s (e para a Clipbase, que está em
  memória, pelo `update`, em operações curtas).
- API mínima: o acesso mutável à Clipbase é `pub(crate)`. `SharedSource::lock`
  é `pub` (os testes de integração dos crates de cima o usam), mas **por
  convenção e documentação é só de leitura**: a GUI lê por funções do `Bases`
  (`list_page`) e não toma o bloqueio.
- Nenhum código segura dois bloqueios ao mesmo tempo (sem *deadlock*);
  envenenamento do `Mutex` é tolerado (`into_inner`), porque as escritas da
  Clipbase são "tudo ou nada".

## Alternativas consideradas

1. Tornar a leitura `&self` nas fontes só de memória (`Clipbase`, `PgnDatabase`)
   e manter o bloqueio apenas para SI4. Reduz o uso do `Mutex`, mas não o
   elimina e duplica caminhos de código.
2. Um leitor próprio por `Effect` (reabrir o `Si4Database` a cada leitura). Sem
   estado compartilhado, mas custo de abertura em cada página da lista.
3. `RwLock`, para a lista e as leituras não competirem. Avaliar se a medição na
   GUI (4b) mostrar contenção no `view`.

## Consequências

- O `view` pode esperar o fim de uma leitura em voo do mesmo slot (milissegundos
  em SI4). **Dívida:** a 4b chama `list_page` (que bloqueia a fonte) no `view`
  e a medição de contenção ainda não foi feita; medir na 4c/4d e, se preciso,
  adotar a alternativa 3.
- A regra "sem `Arc<Mutex>` no domínio" passa a ter esta única exceção. Qualquer
  outro uso exige uma nova ADR.

## Emenda de 2026-10-22 (M3-4c, D559 e D570)

A alternativa 3 foi adotada: `SharedSource` é `Arc<RwLock<BaseSource>>`, com
duas funções de acesso com papéis distintos.

- `read()` (leitura compartilhada): só índice e base de nomes, que `GameSource`
  expõe com `&self`. É o que usam a página da lista, a busca, a ordenação e a
  cópia (o lado da origem não, ver abaixo). Vários leitores ao mesmo tempo.
- `lock()` (escrita exclusiva): ler o registro de uma partida (`&mut`) e
  acrescentar à Clipbase. A Clipbase também é escrita pelo job de cópia, em
  `perform` (`push_many`, um bloqueio curto no fim), além das operações
  curtas do `update` (copiar, colar, limpar uma partida).
- Risco: o `RwLock` do Linux prefere escritores; um escritor na fila faz
  leitores novos esperarem. Mitigações:
  1. as buscas e ordenações seguram o bloqueio **por bloco** de 16 384
     partidas e o soltam entre blocos (`scan_chunks`, `sort_order`); nenhum
     bloqueio dura mais que um bloco;
  2. a `view` nunca espera: `Bases::list_page` usa `try_read()` e, sem o
     bloqueio, devolve a última página guardada no slot;
  3. o `update` recusa com `ListBusy` os comandos da Clipbase que tomam
     escrita enquanto há job sobre ela (busca na sua lista ou cópia para ela),
     e `LoadGame` enquanto há job no mesmo slot, em vez de enfileirar um
     escritor.
  O que resta é uma espera de no máximo um bloco, e só dos efeitos.
- O envenenamento continua tolerado (`into_inner`).
- Esta continua sendo a única exceção à regra "sem `Arc<Mutex>`/`RwLock` no
  domínio".

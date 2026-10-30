# ADR 0011: Funcionalidades retiradas ou fora do escopo (gráfico de pontuação, semáforo MultiPV, xadrez por correspondência)

- **Status:** decisão do dono do projeto, registrada em 2026-10-25. **É uma
  intenção: nenhum código foi removido ainda.** Quando a retirada for executada,
  esta ADR passa a "executada" e o resultado vai para a spec correspondente.

## Contexto

A meta original do roteiro era ter **todas** as funcionalidades do Scid vs PC
(`docs/roteiro.md`). O dono do projeto decidiu abrir três exceções, por
considerar que não valem o custo de manutenção nem se encaixam no rumo do
MyChess:

1. **Retirar o gráfico de pontuação** (o gráfico de avaliação da partida).
2. **Retirar o semáforo MultiPV** ("traffic lights" da análise).
3. **Não implementar a partida por correspondência** (Xfcc, relés, caixas de
   entrada e saída, envio por e-mail).

## Decisão

### 1. Gráfico de pontuação: retirar
Sai o painel de gráfico e seus estilos: M4-7 (`docs/specs/m4-7-graficos-
avaliacao-tempo.md`) e M4-9 (`docs/specs/m4-9-grafico-de-linha.md`).

**Pergunta em aberto (a decidir antes de executar):** o M4-7 também entrega o
gráfico de **tempo** (`%emt`, `%clk`) no mesmo painel. A decisão do dono falou
em "gráfico de pontuação"; se o gráfico de tempo sai junto ou fica, ainda não foi
dito. Enquanto não for decidido, o gráfico de tempo **fica**.

Impactos conhecidos (conferidos no código em 2026-10-25), para planejar a
retirada:
- **O relatório da partida (M4-10) reaproveita o código do gráfico:** a curva
  do painel Relatório usa `graphs::line_view_from_scores`, `report_graph_view`
  e o `canvas` do gráfico (`windows/report_panel.rs`, `graphs/line.rs`,
  `GraphColors::marker`). Retirar o gráfico exige antes **separar** essa curva
  (mover para um módulo do relatório) ou removê-la também.
- `PaneKind::Graph` está persistido nos layouts salvos
  (`prefs/layout.rs`, `prefs/layout/ops.rs`): o layout antigo de um usuário não
  pode quebrar; um painel `"graph"` desconhecido deve ser descartado sem erro.
- Preferências `[graph]` (`graphs::GraphPrefs`): chaves antigas devem ser
  ignoradas sem aviso de erro.
- `Action::ToggleGraphPane` e o atalho (`shortcuts.rs`, `state/actions.rs`), o
  item **Ferramentas > Gráfico** no menu e na faixa, o bloqueio de assistência
  online (`online/assist.rs`) e os textos Fluent do gráfico (atenção:
  `assets/i18n/*/graph.ftl` também guarda chaves do **relatório** e da
  anotação, que ficam).
- Testes: `m4_7_*`, `m4_9_*`, `graph/tests.rs` e os que citam `PaneKind::Graph`.
- **Continuam:** a leitura e a escrita de `[%eval]`, `[%emt]`, `[%clk]` nas
  partidas (usadas por anotação, relatório e importação/exportação PGN).

### 2. Semáforo MultiPV: retirar
Sai o semáforo da análise (parte do M4-5, `docs/specs/m4-5-barra-setas-
semaforo.md`). **Ficam** a barra de avaliação e as setas da engine.

Impactos conhecidos: `evalbar/lights.rs`, a preferência de semáforo em
`evalbar/prefs.rs` e `prefs/change.rs`, `state/evalbar_flow.rs`, mensagens em
`message.rs`, o item **Ver > Semáforo** e seu botão na faixa, as chaves Fluent
correspondentes e os testes `m4_5_*` que cobrem o semáforo. A preferência
antiga no arquivo do usuário deve ser ignorada sem erro.

### 3. Xadrez por correspondência: não implementar
O item "Xadrez por correspondência" do M8 (`tcl/tools/correspondence.tcl` do
Scid) **não será feito**. Nenhum código existe hoje, então não há remoção:
só o roteiro é atualizado. Isso inclui Xfcc (SOAP), relés, caixas e e-mail. O
jogo online continua pelo Lichess (ADR 0010).

## Como executar a retirada (quando o dono pedir)
1. Escrever uma spec curta com a lista de arquivos acima e a decisão sobre o
   gráfico de tempo.
2. Separar o que o relatório usa do gráfico **antes** de apagar o gráfico.
3. Remover código, menus, atalhos, chaves Fluent e testes; manter a leitura
   tolerante de layouts e preferências antigas (testes de migração).
4. `cargo xtask ci` verde; marcar o roteiro; mudar o status desta ADR.

## Alternativas consideradas
- **Manter tudo e só esconder pelos menus:** deixa código e testes a manter sem
  uso. Rejeitada pelo dono.
- **Fazer a partida por correspondência depois:** o dono descartou; o Lichess
  cobre o jogo online.

## Consequências
- O roteiro deixa de prometer paridade total com o Scid: as três exceções ficam
  registradas aqui e anotadas nos itens do `docs/roteiro.md`.
- Menos código e menos testes para manter; em troca, o painel do relatório
  precisa de uma curva própria antes de o gráfico sair.
- Quem usa o Scid e depende dessas funções não as terá no MyChess.

# ADR 0008: Layout dos painéis como árvore de grupos de abas

- **Status:** aceita pelo orquestrador (2026-10-24); revertível pelo dono do projeto

## Contexto

Até a M5 o layout é uma árvore binária de divisões cujas folhas são painéis
(`LayoutNode::Pane(PaneKind)`, M2-3 D135), desenhada pelo `pane_grid` do iced.
As abas do painel da notação (Lances, Informações, Comentário) são fixas e
vivem **dentro** de um único painel (M2-5 D212): `PaneKind` não as conhece.
Para fechar um painel é preciso ir ao menu Visualizar, e não há como reunir
painéis numa só área com abas.

O usuário pediu: (1) um botão para fechar cada painel; (2) painéis que se
fundem em abas por arrasto; (3) abas da notação dinâmicas, isto é, cada aba é
um painel que pode ser fechado, reaberto, movido e agrupado como os demais.

## Decisão

- A folha da árvore passa a ser um **grupo de abas**:
  `LayoutNode::Group { tabs: Vec<PaneKind>, active: usize }`. Um painel
  sozinho é um grupo de uma aba. As divisões continuam binárias.
- O painel da notação deixa de conter abas próprias. Lances (`Notation`),
  Informações (`Info`) e Comentário (`Comment`, novo) são painéis comuns,
  agrupáveis. O layout padrão é `[ Tabuleiro | Grupo(Lances, Informações,
  Comentário) ]`, com a mesma aparência de antes.
- O `Board` fica sempre sozinho em seu grupo e não pode ser fechado nem
  receber abas.
- O formato salvo (`[layout]` do `preferences.toml`) ganha `tabs`/`active` e um
  marcador de versão. Layouts antigos são lidos e migrados (um `Notation`
  isolado vira o grupo com as abas que ele mostrava).
- Quem manda é o layout: as marcas `show_*` do estado da GUI passam a ser
  consultas derivadas dele, não o contrário (hoje o layout é reconstruído a
  partir das marcas, o que perderia os grupos do usuário).
- A fusão é por arrasto no `pane_grid`: soltar na região central de outro
  painel funde; soltar nas bordas continua dividindo. Isso **substitui** a
  troca de lugar que o `pane_grid` fazia ao soltar no centro.

## Consequências

- A tabela, o formato e a migração ficam em `mychess-app` (dados puros,
  testáveis sem janela); o desenho e o arrasto ficam em `mychess-gui`.
- Os testes que tratam as abas da notação como parte de um painel
  (`NotationTab`, `SelectNotationTab`, menu "Aba da notação") precisam ser
  reescritos.
- Arrastar uma aba individual para fora do grupo não é possível com o
  `pane_grid` (o título é a pega de todo o grupo): a separação é por menu de
  contexto da aba. Um arrasto por aba exigiria um widget próprio; fica como
  evolução.
- Versões antigas do aplicativo ignoram o formato novo e usam o layout padrão.

## Alternativas consideradas

1. Manter as abas fixas e só acrescentar o botão de fechar: não atende o pedido
   de agrupar painéis nem de dispensar uma aba da notação.
2. Abandonar o `pane_grid` por um dock próprio: arrasto por aba e zonas de
   soltura mais ricas, mas é um widget grande e novo; não se justifica agora.

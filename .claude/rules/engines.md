---
paths:
  - "crates/mychess-engine/**"
---

# Engines (UCI e XBoard/WinBoard)

- Protocolos como **máquinas de estado explícitas** (por exemplo
  `Starting → Handshake → Ready → Searching → Stopping`). Cada comando só é
  enviado no estado em que é válido.
- **Parsing tolerante e defensivo**: linha desconhecida é registrada
  (`tracing::debug!`) e ignorada; número ou lance inválido não derruba a sessão.
  Lances vindos da engine são validados contra a posição antes de chegar ao app.
- **Timeouts** em toda espera: `uci`/`uciok`, `isready`/`readyok`,
  `stop`/`bestmove`. Engine travada é encerrada e o problema é reportado.
- **Ciclo de vida**: ao fechar a sessão, `quit`, espera curta e então `kill`.
  `kill_on_drop(true)`. Nenhum processo órfão.
- **Multiplataforma**: ver `multiplataforma.md`, seção Processos (sem console no
  Windows, sem shell, diretório de trabalho correto).
- **Sem bloqueio**: I/O assíncrono (`tokio::process`, leitura por linhas). A API
  pública entrega um *stream* de eventos (`EngineEvent`) e recebe comandos por
  canal.
- **Log do protocolo**: o tráfego enviado e recebido fica disponível para a
  janela de log da engine (como no Scid), com limite de memória.
- **Opções**: `option name ... type ...` mapeadas para tipos (`check`, `spin`,
  `combo`, `button`, `string`), com validação de limites. A configuração por
  engine é persistida pelo `mychess-app`.
- **Testes**: engine falsa e roteirizada, um binário de teste do próprio crate
  que fala UCI/XBoard por stdin/stdout. Nada de Stockfish obrigatório no CI.
  Testes com engine real: `#[ignore]` e a variável
  `MYCHESS_TEST_ENGINE=/caminho/da/engine`.
- Referência no Scid: `tcl/tools/analysis.tcl` (janelas de análise e
  anotação), `tcl/tools/uci.tcl` (UCI), `tcl/tools/wbdetect.tcl` (XBoard) e
  `tcl/tools/comp.tcl` (torneios). Use a skill `/consultar-scid`. O
  `src/engine.cpp` é a engine **interna** do Scid (usada em "melhor casa sob o
  mouse" e no Scidlet) e não será portado: o MyChess usa engines UCI.

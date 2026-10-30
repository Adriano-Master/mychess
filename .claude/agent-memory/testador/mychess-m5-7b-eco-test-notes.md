---
name: mychess-m5-7b-eco-test-notes
description: Armadilhas dos testes m5_7b (navegador ECO, classificar, novidade) no app e na GUI
metadata:
  type: project
---

- Abrir uma base (PGN/SI4) carrega a partida 1 no editor: nos testes de novidade/identificar chame `Command::NewGame` antes de jogar lances, senao os lances somam na partida carregada.
- `ListMessage::KeepSelected` e bloqueada com o navegador ECO aberto: filtre antes de abrir.
- `Message::EcoLoaded` e ignorada se `prefs.files.eco_file` nao for o mesmo caminho.
- `EcoBook::summary("")` afina muito (prefixo < 3 caracteres); para testar o corte de 500 use um no de 3 caracteres (`B90`) e posicoes distintas (posicoes repetidas se fundem).
- Cancelar antes do inicio devolve `Err(Cancelled)`: o dialogo fica sem resultado e sem aviso (so uma partida/lote em andamento gera "interrompida").
- GUI: radios e checkbox do iced nao expoem o texto ao `find`; clique por `point_at` + `simulate(click())` (clicar um `Point` pelo `click` acerta o centro da janela). Texto em Enter com foco no campo e do `text_input`, nao do handler de teclas.
- GUI: no navegador ECO as legendas/contagens das barras saem com caixa 0x0 (`stack!` limita a altura): achado de produto em `eco_browser.rs::chart`.

# ADR 0002: Interface gráfica com iced 0.14

- **Status:** aceita (2026-10-21)

## Contexto

O MyChess precisa de uma interface moderna nas três plataformas, escrita em
Rust, com: tabuleiro com arrastar e soltar e desenho de setas, lista de partidas
virtualizada (milhões de linhas), notação com lances clicáveis, gráficos,
painéis encaixáveis, multijanela, temas claro/escuro e HiDPI. Também precisa de
comunicação assíncrona contínua com engines.

## Decisão

Usar **iced 0.14** (MIT), com renderização wgpu e fallback por software
(tiny-skia), nas features `canvas`, `svg`, `tokio` e `advanced`. Componentes
extras do `iced_aw` (menus, abas) só quando necessários. Testes de interface
headless com `iced_test` 0.14.

Motivos:

- **Rust puro e nativo** nas três plataformas, sem WebView.
- **Elm Architecture nativa**: casa com a ADR 0001.
- O 0.14 já traz o que o projeto precisa: `pane_grid` (painéis), `table`,
  `canvas`, `rich_text`, `markdown`, multijanela (`daemon`), `Subscription`
  para fluxos de engine e `iced_test` (simulador headless com snapshots).
- Usado em produção (por exemplo, o desktop COSMIC da System76).

## Alternativas consideradas

| Alternativa | Por que não |
|---|---|
| **Slint** | Visual moderno pronto e boa acessibilidade, mas a interface fica numa DSL própria (`.slint`), o desenho customizado (tabuleiro, setas) é mais limitado que um canvas, e o licenciamento é triplo (GPLv3/royalty-free/comercial) |
| **egui** | Muito produtivo, mas o modo imediato (redesenho a cada frame) dá cara de "ferramenta de desenvolvedor" e não segue MVC/MVU |
| **Tauri + front-end web** | Visual web e o tabuleiro `chessground` (usado pelo En Croissant), mas a interface sairia em TypeScript e cairia na WebKitGTK no Linux, que é instável |
| **Dioxus** | Rust com estilo React, mas no desktop roda sobre WebView, com o mesmo problema da WebKitGTK |
| **GPUI (Zed)** | Muito rápido, mas a documentação é escassa e a API instável para um projeto deste porte |

## Consequências

- O iced é pré-1.0: haverá mudanças de API entre versões. A versão fica
  **fixada** no `Cargo.toml` do workspace; atualizar é uma tarefa planejada,
  nunca incidental.
- Componentes que o iced não tem (lista virtualizada para milhões de linhas,
  widget de tabuleiro) são escritos no `mychess-gui` como widgets próprios.
- Acessibilidade (leitores de tela) ainda é limitada no iced; reavaliar no M9.
- A feature `image` do iced puxa todos os codecs (inclusive AVIF, de compilação
  pesada). Prefira SVG; se precisar de bitmaps, use `image-without-codecs` mais
  o crate `image` só com `png`.

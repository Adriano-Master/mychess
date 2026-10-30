---
paths:
  - "crates/mychess-gui/**"
---

# GUI com iced 0.14

Arquitetura em `docs/arquitetura.md` (seções 2 e 9) e na ADR 0002.

## Confira a API da versão fixada
O iced muda entre versões menores. Exemplos da internet para 0.12/0.13
(`Application`/`Sandbox` como trait, `Command`) **estão obsoletos**. Antes de
usar uma API, confira a fonte da versão do `Cargo.lock`:
`~/.cargo/registry/src/*/iced_widget-0.14.*/src/` (widgets),
`~/.cargo/registry/src/*/iced-0.14.*/src/application.rs`, ou
https://docs.rs/iced/0.14.0.

Fatos verificados no 0.14:
- `iced::application(boot, update, view)`:
  - `boot`: `Fn() -> State` ou `Fn() -> (State, Task<Message>)`;
  - `update`: `Fn(&mut State, Message) -> impl Into<Task<Message>>`;
  - `view`: `Fn(&State) -> impl Into<Element<Message>>`.
- Métodos do builder: `.title(..)`, `.theme(..)` (função que retorna
  `impl Into<Option<Theme>>`), `.subscription(..)`, `.window_size(..)`,
  `.font(..)`, `.run()`.
- Multijanela: `iced::daemon(boot, update, view)` com `view(&State, window::Id)`.
- Assincronia: `Task::perform(future, Message::X)`, `Task::run(stream, f)`,
  `Task::batch(..)`, `Task::none()`. Fluxos contínuos usam `Subscription::run(fn)`
  ou `Subscription::run_with(dado: Hash, fn(&dado) -> Stream)`. O *builder* é
  ponteiro de função, sem captura; dados entram pelo `run_with`.
- Widgets disponíveis: `pane_grid`, `table`, `canvas`, `svg`,
  `rich_text`/`span`, `text_editor`, `markdown`, `scrollable`, `tooltip`,
  `combo_box`, `pick_list`, `mouse_area`, `responsive`, `stack`, `float`, `pin`,
  `sensor`, `grid`, `keyed`.
- Testes headless: `iced_test::simulator(element)`, com `.click("texto")`,
  `.find(..)`, `.typewrite(..)`, `.tap_key(..)`, `.into_messages()` e snapshots.

## Estrutura do crate
```
src/main.rs     # tracing, preferências, iced::daemon(..).run()
src/shell.rs    # Shell { app: AppState, ui: UiState }, Message, update/view/subscription
src/windows/    # uma janela ou painel por módulo (board, game_list, tree, analysis, ...)
src/widgets/    # widgets reutilizáveis: board, virtual_list, notation, graph
src/theme/      # paletas, estilos, temas de tabuleiro
src/i18n.rs     # carregamento Fluent e consulta de chaves
```
Assets (SVG, fontes, sons, `.ftl`) ficam em `assets/` na raiz do workspace e
entram no binário com `include_bytes!`/`include_str!`.

## Regras MVU
1. `update` e `view` **nunca bloqueiam**: nada de I/O, `sleep` ou cálculo longo.
   Isso vira um `Effect` do `mychess-app`, executado como `Task`.
2. O estado de domínio mora em `mychess_app::AppState`. A GUI guarda só estado
   visual (hover, arraste, rolagem, menus abertos, ids de janela). Critério: se
   precisa ser salvo ou testado como regra, pertence ao `mychess-app`.
3. Mensagens: `enum Message { App(mychess_app::Message), Ui(UiMessage) }`;
   widgets filhos são mapeados com `.map(..)`.
4. `view` compõe funções pequenas que retornam `Element<'_, Message>`,
   emprestando do estado (sem clonar dados grandes). Nenhuma regra de negócio
   em `view`.
5. Cores e estilos só via tema (`theme.extended_palette()` e módulo `theme`);
   nada de cor literal espalhada.
6. Nenhum texto visível no código: chaves Fluent em
   `assets/i18n/<idioma>/*.ftl`, sempre com en-US e pt-BR.
7. Atalhos de teclado centralizados num único mapa, com
   `Modifiers::command()` (Cmd no macOS).
8. Listas com milhares de itens são **virtualizadas**: só as linhas visíveis
   viram widgets. `table`/`column` simples só para listas pequenas.
9. Tabuleiro: `canvas` (ou `advanced::Widget`) com `canvas::Cache` separando as
   camadas estáticas (casas, coordenadas) das dinâmicas (peças, setas, peça
   arrastada). Limpe o cache só quando a camada muda.
10. Imagens: prefira `svg`. Para bitmaps, use `image-without-codecs` mais o
    crate `image` só com `png`. A feature `image` puxa todos os codecs,
    inclusive AVIF, com compilação pesada.

## Esqueleto de referência (0.14)
```rust
fn main() -> iced::Result {
    iced::application(Shell::new, Shell::update, Shell::view)
        .title(Shell::title)
        .theme(Shell::theme)
        .subscription(Shell::subscription)
        .run()
}

impl Shell {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::App(msg) => {
                let effects = mychess_app::update(&mut self.app, msg);
                Task::batch(effects.into_iter().map(|effect| {
                    Task::perform(mychess_app::perform(effect), Message::App)
                }))
            }
            Message::Ui(msg) => self.ui.update(msg),
        }
    }
}
```
Os nomes do `mychess-app` são ilustrativos: siga a API real do crate.

## Testes
- Regras de interação são testadas no `mychess-app`, como funções puras.
- Widgets e telas: `iced_test` (headless). Snapshots com o renderizador por
  software, para serem determinísticos. O `.cargo/config.toml` já define
  `ICED_TEST_BACKEND=tiny-skia`; sem isso o simulador abre um dispositivo
  Vulkan por teste e, em paralelo, trava de forma intermitente.
- **Não rode `cargo run -p mychess-gui` em primeiro plano** numa sessão
  automatizada: a janela bloqueia o comando até ser fechada. Para validar,
  prefira os testes headless. Se precisar abrir o app, rode em segundo plano e
  encerre o processo depois.

## Menus do `iced_aw`
- Todo `MenuBar` passa por `widgets::menu_guard::guard` (menu da janela e
  menu "mais" da barra de navegação). O `iced_aw` 0.14.1 entra em pânico em
  `Overlay::operate` quando uma operação (`focus`, `scroll_to`, `snap_to`...)
  roda com um menu aberto que tem submenu ainda não aberto (D992). Remover o
  wrapper quando o upstream corrigir; o teste `m5_2_menu_guard` avisa (o caso
  "sem proteção" deixa de entrar em pânico).

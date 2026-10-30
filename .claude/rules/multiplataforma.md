# Multiplataforma (Linux, Windows, macOS)

Todo código compila e funciona nas três plataformas. O CI roda clippy e testes
em `ubuntu-latest`, `windows-latest` e `macos-latest`.

## Caminhos e arquivos
- `Path`/`PathBuf` e `join`. Nunca concatene `"/"` ou `"\\"`; exiba com
  `.display()`.
- Diretórios do usuário via `directories::ProjectDirs`. Nada de `~`, `/tmp` ou
  `C:\` fixos; temporários com `tempfile`.
- Os sistemas de arquivos do Windows e do macOS não diferenciam maiúsculas por
  padrão. Não use a caixa para distinguir arquivos (`Base.si4` × `base.si4`) e
  compare extensões sem diferenciar caixa.
- Nomes gerados (exportação): remova `< > : " / \ | ? *` e evite os nomes
  reservados do Windows (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`,
  `LPT1`–`LPT9`).
- Escrita segura: grave num temporário **no mesmo diretório** e use
  `std::fs::rename`, que substitui o destino nas três plataformas.
- **O Windows bloqueia arquivos abertos**: feche handles e *mmaps* antes de
  renomear, apagar ou compactar um banco.

## Texto e bytes
- Texto pode chegar em UTF-8 (com ou sem BOM), Latin-1 ou Windows-1252 (PGNs
  antigos). Detecte com `chardetng` e converta com `encoding_rs` na entrada;
  internamente tudo é `String` UTF-8.
- Aceite `\n`, `\r\n` e `\r` na leitura; grave `\n`.
- Formatos binários: ordem de bytes explícita (`u32::from_be_bytes`,
  `to_be_bytes`). Nada de `transmute`, de `#[repr(C)]` sobre bytes de arquivo ou
  de depender do tamanho de `usize`. Offsets de arquivo são `u64`.

## Processos (engines e programas externos)
- `Command` com programa e argumentos separados. **Nunca** `sh -c`/`cmd /C`
  com string montada.
- Windows: `CREATE_NO_WINDOW` (`0x0800_0000`) via
  `std::os::windows::process::CommandExt::creation_flags`, para não abrir um
  console ao iniciar a engine.
- `kill_on_drop(true)` (tokio) ou encerramento explícito. Diretório de trabalho
  = diretório da engine. Não presuma extensão de executável.

## Código específico de plataforma
- `#[cfg(windows)]`, `#[cfg(unix)]` e `#[cfg(target_os = "macos")]` ficam
  concentrados em módulos `platform` pequenos, com implementação (ou alternativa
  documentada) para as três plataformas.
- Prefira crates que abstraem a plataforma (`directories`, `rfd`, `tempfile`) a
  escrever `cfg` próprio.

## Interface
- Atalhos: `Ctrl` no Linux/Windows, `Cmd` no macOS. Use
  `keyboard::Modifiers::command()` do iced, nunca `control()` fixo.
- Escala HiDPI fracionária (Windows, Wayland): nada de tamanhos em pixels
  físicos; peças em SVG.
- Fontes embutidas no binário; não dependa de fontes do sistema.
- Diálogos de arquivo nativos via `rfd` (no Linux, portal XDG).

## Dependências de sistema
- Prefira Rust puro. Se uma dependência exigir biblioteca de sistema para
  compilar (por exemplo, `rodio` pede ALSA no Linux: `libasound2-dev`),
  documente no `README.md` e adicione a instalação ao
  `.github/workflows/ci.yml` **no mesmo commit**.

## Testes
- Nada que dependa da máquina de quem roda: separador de caminho, final de
  linha, fuso horário, ordem de listagem de diretório. Teste específico de
  plataforma usa `#[cfg]` e, quando fizer sentido, tem equivalente nas demais.

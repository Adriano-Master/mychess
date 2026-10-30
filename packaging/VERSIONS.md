# Versões usadas nos pacotes atuais (0.1.0-alpha)

Máquina de compilação: Linux (Freedesktop SDK 25.08, glibc 2.42), x86-64.

| Ferramenta | Versão |
|---|---|
| rustc / cargo | 1.99.0 (fixado em `rust-toolchain.toml`) |
| Alvo Windows | `x86_64-pc-windows-gnu` (MinGW, ligado estaticamente; sem DLLs do MinGW) |
| cargo-zigbuild | 0.24.0 |
| Zig (pacote pip `ziglang`) | 0.17.0 |
| Perfil | `dist` (LTO completo, `codegen-units = 1`, `strip = "symbols"`) |

O binário Linux exige glibc >= 2.39 (maior símbolo `GLIBC_2.39`, conferido com `objdump -T`).

## Stockfish

- Linux: Stockfish 10, build `stockfish_10_x64_modern_official` ("64 POPCNT"),
  SHA-256 `1bea09ef86b775862f1435d5733b726d97898b4c642dcf8bb7adb64ea21c2e6f`;
  `stockfish-10-src.zip` SHA-256
  `29bd01e7407098aa9e851b82f6ea4bf2b46d26e9075a48a269cb1e40c582a073`. Fornecidos
  pelo dono do projeto; copiados sem alteração.
- Windows: **sem engine**. Tentativa de baixar o Stockfish 10 para Windows do site
  oficial: `https://stockfishchess.org/files/stockfish-10-win.zip` devolve uma página
  HTML (o site foi refeito; não é um zip), a página `stockfishchess.org/download/`
  só oferece a versão atual (Stockfish 19) e as *releases* do repositório oficial
  `official-stockfish/Stockfish` anteriores à sf_16 não têm binários anexados. Nada
  foi baixado nem incluído.

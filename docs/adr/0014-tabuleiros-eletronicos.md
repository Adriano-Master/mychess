# ADR 0014: Tabuleiros eletrônicos: crate `mychess-eboard`, `serialport` sem udev e driver como processo

- **Status:** aceita pelo orquestrador; revertível pelo dono. Especificação: `docs/specs/m8-10-tabuleiros-eletronicos.md` (D2700-D2759).

## Contexto

O Scid vs PC fala com a Novag Citrine diretamente pela porta serial (`novag.tcl`) e com tabuleiros DGT por um processo externo, o *input engine* (`inputengine.tcl`, protocolo do projeto dgtdrv). O MyChess precisa das duas coisas em Linux, Windows e macOS, sem hardware no CI.

## Decisão

1. **Crate novo `mychess-eboard`** (Model), dependendo só de `mychess-core`. Contém os dois protocolos como funções puras, a conversão do lance do tabuleiro em lance legal, a abertura da porta, o processo do driver, as sessões `async` e os dublês de teste (`testing`). Não vive no `mychess-engine`: o protocolo é outro e o reaproveitável (montar `Command`, cortar linhas) são ~40 linhas.
2. **Dependência nova: `serialport` 4.10.1 com `default-features = false`.** Sem a feature `libudev` (padrão), o crate **não precisa de nenhuma biblioteca de sistema** no Linux (a enumeração lê `/sys/class/tty`); Windows usa `windows-sys` e macOS `IOKit`/`core-foundation`. Alternativa recusada: abrir o dispositivo com `std` e configurar o termios/`DCB` na mão exigiria `unsafe` e `cfg` por plataforma (regra de ouro 4); `tokio-serial`/`mio-serial` puxariam mais dependências para um fluxo de 57600 baud que cabe em duas *threads*.
3. **E/S serial bloqueante em *threads* de sistema** (uma de leitura com *timeout* curto, uma de escrita), ligadas à sessão por canais; nada bloqueia o executor.
4. **O driver DGT é um processo** iniciado como as engines: programa e argumentos separados, sem *shell*, `CREATE_NO_WINDOW` no Windows, `kill_on_drop`. O MyChess não traz driver.

## Licença (verificada no registry local)

- `serialport` 4.10.1: **MPL-2.0** (já em `deny.toml`). A MPL-2.0 §3.3 permite combinar com GPL (a GPL é "Secondary License"), sem a restrição da cláusula "Incompatible With Secondary Licenses" (o crate não a usa). Código do `serialport` continua em arquivos MPL; o MyChess não o modifica.
- Transitivas (sem a feature `libudev`): `cfg-if` (MIT/Apache-2.0), `scopeguard` (MIT/Apache-2.0), `bitflags` 2 (MIT/Apache-2.0) e `nix` 0.26 (MIT; Unix), `unescaper` (MIT; Linux), `windows-sys` 0.52 (MIT/Apache-2.0; Windows), `core-foundation`/`core-foundation-sys`/`io-kit-sys`/`mach2` (MIT/Apache-2.0; macOS). Verificar com `cargo deny check licenses` (a cargo-deny roda no CI).
- `unsafe`: o `serialport` o usa por dentro; o código do MyChess não tem `unsafe` e o lint do workspace continua `deny`.

## Riscos conhecidos

- `serialport::available_ports` (Linux sem udev) tem um `expect` quando `/sys/class/tty` não existe: a chamada é protegida por `catch_unwind` (D2702).
- Sem placa nem driver DGT disponíveis para quem implementou: o comportamento real fica registrado como dívida (D2750-D2752).

## Consequências

- `ALLOWED_DEPS`: `mychess-eboard` → `mychess-core`; `mychess-app` → `mychess-eboard`. `docs/arquitetura.md` ganha a linha.
- Nada muda em `.github/workflows/ci.yml`: não há biblioteca de sistema nova. O README ganha uma frase: o `serialport` é usado **sem** `libudev`; quem quiser a enumeração detalhada de USB no Linux pode compilar com a feature (não é padrão).

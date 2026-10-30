#!/bin/sh
# PostToolUse (Edit|Write): formata com rustfmt o arquivo .rs recém-editado.
# Se o rustfmt falhar (em geral, erro de sintaxe), a mensagem volta ao Claude:
# exit 2 em PostToolUse não desfaz a edição, só exibe o stderr.
# A verificação definitiva continua sendo `cargo xtask ci`.
. "$(dirname "$0")/lib.sh"
hook_read_input
file=$(hook_file_path)

case "$file" in
    *.rs) ;;
    *) exit 0 ;;
esac
[ -f "$file" ] || exit 0

# Só arquivos dentro do projeto.
root=${CLAUDE_PROJECT_DIR:-$PWD}
case "$(hook_norm_path "$file" "$root")" in
    "$(hook_norm_path "$root" /)"/*) ;;
    *) exit 0 ;;
esac
command -v rustfmt >/dev/null 2>&1 || exit 0

# A partir do diretório do arquivo, o rustup encontra o rust-toolchain.toml e o
# rustfmt encontra o rustfmt.toml do projeto.
cd "$(dirname "$file")" || exit 0
if ! output=$(rustfmt --edition 2024 "$file" 2>&1); then
    printf 'rustfmt não conseguiu formatar %s (provável erro de sintaxe):\n' "$file" >&2
    printf '%s\n' "$output" | head -n 20 >&2
    exit 2
fi
exit 0

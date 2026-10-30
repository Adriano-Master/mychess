#!/bin/sh
# PreToolUse (Edit|Write|NotebookEdit): bloqueia escrita dentro de reference/,
# onde fica o código do Scid vs PC, mantido só para consulta. Complementa a
# regra "deny" `Edit(/reference/**)` do settings.json. A raiz é conferida duas
# vezes, por CLAUDE_PROJECT_DIR e pela raiz do repositório git, para continuar
# valendo se a sessão começar num subdiretório do projeto.
. "$(dirname "$0")/lib.sh"
hook_read_input
file=$(hook_file_path)
[ -n "$file" ] || exit 0

check_root() {
    [ -n "$1" ] || return 0
    target=$(hook_norm_path "$file" "$1")
    reference=$(hook_norm_path reference "$1")
    case "$target" in
        "$reference" | "$reference"/*)
            printf 'Bloqueado: %s fica em reference/, o código do Scid vs PC, que é somente leitura. Escreva no workspace (crates/, docs/, assets/).\n' "$file" >&2
            exit 2
            ;;
    esac
}

check_root "${CLAUDE_PROJECT_DIR:-}"
check_root "$(git rev-parse --show-toplevel 2>/dev/null)"
exit 0

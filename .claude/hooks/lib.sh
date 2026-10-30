# shellcheck shell=sh
# Funções comuns aos hooks do Claude Code deste projeto.
# POSIX sh: roda no Linux, no macOS e no Git Bash do Windows.
# O evento chega como JSON no stdin; os campos usados estão em `tool_input`.

# Lê o JSON do evento (stdin) uma única vez.
hook_read_input() {
    HOOK_INPUT=$(cat)
}

# Imprime o nome de um Python que realmente executa. No Windows,
# `command -v python3` pode encontrar o atalho da Microsoft Store, que não roda.
hook_find_python() {
    for hook_py in python3 python; do
        if command -v "$hook_py" >/dev/null 2>&1 &&
            "$hook_py" -c 'import sys' >/dev/null 2>&1; then
            printf '%s' "$hook_py"
            return 0
        fi
    done
    return 1
}

# Imprime `tool_input.<campo>` (string), ou nada se o campo não existir.
# Usa jq ou Python (lendo e escrevendo UTF-8 mesmo no Windows). Sem nenhum dos
# dois, recorre a um extrator simples que basta para caminhos de arquivo, mas
# não para comandos com aspas. O `\r` que jq.exe/python.exe emitem no Windows é
# removido.
hook_field() {
    if command -v jq >/dev/null 2>&1; then
        printf '%s' "$HOOK_INPUT" | jq -r --arg f "$1" '.tool_input[$f] // empty'
    elif hook_python=$(hook_find_python); then
        printf '%s' "$HOOK_INPUT" | "$hook_python" -I -c '
import json, sys
data = json.loads(sys.stdin.buffer.read().decode("utf-8"))
value = data.get("tool_input", {}).get(sys.argv[1])
if isinstance(value, str):
    sys.stdout.buffer.write(value.encode("utf-8"))
' "$1"
    else
        printf '%s' "$HOOK_INPUT" |
            sed -E -n "s/.*\"$1\"[[:space:]]*:[[:space:]]*\"([^\"]*)\".*/\\1/p" |
            head -n 1 | sed 's/\\\\/\\/g'
    fi | tr -d '\r'
}

# Caminho do arquivo alvo: Edit e Write usam `file_path`; NotebookEdit, `notebook_path`.
hook_file_path() {
    hook_path=$(hook_field file_path)
    [ -n "$hook_path" ] || hook_path=$(hook_field notebook_path)
    printf '%s' "$hook_path"
}

# Normaliza um caminho **para comparação** (não para uso):
# - separadores `/`, unidade do Windows no formato do Git Bash (`C:\x` → `/c/x`);
# - sem `.`, `..`, `//` nem `/` final;
# - minúsculas, porque Windows e macOS não diferenciam caixa por padrão.
# Um caminho relativo é resolvido a partir de $2.
hook_norm_path() {
    printf '%s\n' "$1" | tr '\\' '/' | awk -v base="$(printf '%s' "$2" | tr '\\' '/')" '
        function drive(p) {
            if (p ~ /^[A-Za-z]:(\/|$)/) return "/" tolower(substr(p, 1, 1)) substr(p, 3)
            return p
        }
        {
            p = drive($0)
            if (substr(p, 1, 1) != "/") p = drive(base) "/" p
            n = split(p, parts, "/")
            depth = 0
            for (i = 1; i <= n; i++) {
                if (parts[i] == "" || parts[i] == ".") continue
                if (parts[i] == "..") { if (depth > 0) depth--; continue }
                stack[++depth] = parts[i]
            }
            out = ""
            for (i = 1; i <= depth; i++) out = out "/" stack[i]
            print tolower(out == "" ? "/" : out)
        }'
}

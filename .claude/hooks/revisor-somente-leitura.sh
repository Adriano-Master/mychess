#!/bin/sh
# PreToolUse (Bash) do agente revisor: bloqueia comandos que alteram arquivos,
# o repositório git ou as dependências. É uma rede de segurança heurística
# contra acidentes plausíveis, não contra um agente adversário. O revisor
# também é instruído a apenas ler e não tem as ferramentas Edit/Write. Sem jq
# nem Python o comando não pode ser lido com segurança, e então nada é
# bloqueado.
. "$(dirname "$0")/lib.sh"
hook_read_input
cmd=$(hook_field command)
[ -n "$cmd" ] || exit 0

deny() {
    printf 'Bloqueado: o revisor é somente leitura (%s). Descreva a correção no relatório em vez de aplicá-la.\n' "$1" >&2
    exit 2
}

matches() {
    printf '%s\n' "$1" | grep -Eq -- "$2"
}

# 1. Troca o conteúdo entre aspas por espaço: padrões de busca como "->" ou
#    'a > b' não são comandos. A máquina de estados respeita aspas de um tipo
#    dentro do outro e caracteres escapados (`\>` não é redirecionamento).
text=$(printf '%s\n' "$cmd" | awk -v sq="'" '
    BEGIN { q = "" }
    {
        out = ""
        n = length($0)
        for (i = 1; i <= n; i++) {
            c = substr($0, i, 1)
            if (q == "") {
                if (c == "\\") { i++; out = out "_"; continue }
                if (c == sq || c == "\"") { q = c; out = out " "; continue }
                out = out c
            } else if (q == "\"" && c == "\\") {
                i++
            } else if (c == q) {
                q = ""
            }
        }
        print out
    }')
# 2. Aritmética $(( a > b )) não é redirecionamento.
text=$(printf '%s\n' "$text" | sed -E 's/\$\(\([^)]*\)\)/ /g')
# 3. Redirecionamentos inofensivos: 2>&1, >&2, >/dev/null, 2>/dev/null, &>/dev/null.
redirects=$(printf '%s\n' "$text" | sed -E 's/[0-9]*>&[0-9-]+//g; s/(&|[0-9]*)>>?[[:space:]]*\/dev\/null//g')

# Início de um comando simples, aceitando prefixos que não mudam o programa
# (VAR=x, env, sudo, command, exec, nice, nohup, time) e caminho (/bin/rm).
start='(^|[;&|(`{]|\$\()[[:space:]]*(([A-Za-z_][A-Za-z0-9_]*=[^[:space:]]*|sudo|env|command|builtin|exec|nice|nohup|time)[[:space:]]+)*([^[:space:];&|]*/)?'
end='([[:space:]]|$)'
word='(^|[^[:alnum:]_.-])'
mutating='rm|rmdir|unlink|mv|cp|mkdir|touch|chmod|chown|chgrp|ln|truncate|dd|tee|install|shred|patch|rsync|tar|unzip|gzip|gunzip|bzip2|bunzip2|xz|unxz|wget'
cargo="${word}cargo([[:space:]]+\\+[^[:space:]]+)?[[:space:]]+"
git="${word}git([[:space:]]+(-C|-c|--git-dir|--work-tree|--namespace)[[:space:]]+[^[:space:]]+|[[:space:]]+--?[a-zA-Z][a-zA-Z-]*(=[^[:space:]]*)?)*[[:space:]]+"

# Redirecionamento e programas que gravam.
matches "$redirects" '>' && deny "redirecionamento para arquivo"
matches "$text" "${start}(${mutating})${end}" && deny "comando que altera arquivos"
matches "$text" "${word}xargs[^;&|]*[[:space:]]([^[:space:]]*/)?(${mutating})${end}" &&
    deny "comando que altera arquivos"
matches "$text" "${word}find[^;&|]*[[:space:]](-delete|-fprint[0f]?|-fls)${end}" && deny "find que altera arquivos"
matches "$text" "${word}find[^;&|]*[[:space:]]-(exec|execdir|ok|okdir)[[:space:]]+([^[:space:]]*/)?(${mutating}|sed|perl)${end}" &&
    deny "find que altera arquivos"
matches "$text" "${word}(sed|perl)[^;&|]*[[:space:]](-[a-zA-Z]*i|--in-place)" && deny "edição in-place"
matches "$text" "${word}sort[^;&|]*[[:space:]](-[a-zA-Z]*o|--output)" && deny "sort com arquivo de saída"
matches "$text" "${word}curl[^;&|]*[[:space:]](-[a-zA-Z]*[oO]|--output|--remote-name)" && deny "download para arquivo"

# Código arbitrário embutido (shell, eval, source, Python, Perl, Ruby, Node).
matches "$text" "${start}((ba|da|z|k)?sh[[:space:]]+-[a-zA-Z]*c|eval|source|python[0-9.]*[[:space:]]+-[a-zA-Z]*c|(ruby|node)[[:space:]]+-[a-zA-Z]*e|perl[[:space:]]+-[a-zA-Z]*[ei])${end}" &&
    deny "interpretador com código embutido"
matches "$text" '(^|[;&|(`{])[[:space:]]*\.[[:space:]]' && deny "execução de script com ."

# Cargo e rustfmt (com ou sem seletor de toolchain, ex.: cargo +nightly).
matches "$text" "${cargo}(fix|add|remove|rm|update|install|uninstall|publish|new|init|generate-lockfile|vendor|package|login|logout|owner|yank)${end}" &&
    deny "cargo que altera código ou dependências"
matches "$text" '--fix([[:space:]=]|$)' && deny "correção automática (--fix)"
if matches "$text" "${cargo}(xtask[[:space:]]+)?fmt${end}|${word}rustfmt${end}"; then
    matches "$text" '--check|--version|--help|-V([[:space:]]|$)|--print-config' ||
        deny "formatação sem --check"
fi

# Git: subcomandos que gravam sempre, e os que gravam só em algumas formas.
matches "$text" "${git}(add|commit|push|pull|fetch|reset|checkout|switch|restore|clean|rebase|merge|cherry-pick|revert|rm|mv|apply|am|init|clone|worktree|gc|prune|notes|update-index|update-ref|format-patch|bundle|archive|filter-branch|replace|submodule)${end}" &&
    deny "git que altera o repositório"
matches "$text" "${git}[^;&|]*--output([=[:space:]]|$)" && deny "git com arquivo de saída"
if matches "$text" "${git}stash${end}"; then
    matches "$text" "${git}stash[[:space:]]+(list|show)${end}" || deny "git stash"
fi
if matches "$text" "${git}config${end}"; then
    matches "$text" "${git}config[^;&|]*[[:space:]](--get|--get-all|--get-regexp|--list|-l)${end}" || deny "git config que grava"
fi
matches "$text" "${git}tag([[:space:]]+-[a-zA-Z]*[dasfmu]|[[:space:]]+[^-[:space:]])" && deny "git tag que grava"
matches "$text" "${git}branch([^;&|]*[[:space:]](-[a-zA-Z]*[dDmMcCfu]|--delete|--move|--copy|--force|--set-upstream-to|--unset-upstream|--edit-description)|[[:space:]]+[^-[:space:]])" &&
    deny "git branch que grava"
exit 0

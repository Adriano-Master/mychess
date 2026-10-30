---
name: nova-funcionalidade
description: Conduz uma funcionalidade do MyChess de ponta a ponta. Especifica a partir do Scid vs PC, implementa (agente implementador), testa (agente testador) e revisa (agente revisor), iterando até a aprovação. Use quando o usuário pedir para implementar uma funcionalidade ou um item do roteiro.
argument-hint: '<funcionalidade ou item do roteiro, ex.: "M1: leitura e escrita de FEN">'
---

# Nova funcionalidade: $ARGUMENTS

Você, a sessão principal, é o **orquestrador**. Subagentes não chamam outros
subagentes, então é você quem repassa contexto e relatórios entre eles.

## Passos
1. **Enquadrar.** Localize o item em `docs/roteiro.md` e confira os
   pré-requisitos (marcos anteriores). Se o pedido for ambíguo ou grande demais
   para uma iteração, proponha a divisão ao usuário antes de seguir.
2. **Especificar.** Se não existir `docs/specs/<slug>.md`, rode
   `/consultar-scid <funcionalidade>` e salve o resultado nesse arquivo. Leve
   ao usuário as "Perguntas em aberto" que mudam o comportamento e registre as
   respostas na especificação. Decisão estrutural nova exige uma ADR em
   `docs/adr/`.
3. **Implementar.** Delegue ao agente `implementador`, passando o caminho da
   especificação, os crates-alvo, os critérios de aceite, as restrições e o que
   fica fora do escopo.
4. **Testar.** Delegue ao agente `testador`, passando a especificação, os
   critérios de aceite e a lista de arquivos alterados (do relatório do
   implementador). Peça para testar o **contrato**, não "a implementação".
5. **Corrigir.** Falha causada por bug de produção volta ao `implementador`
   com o relatório do testador; depois o `testador` roda de novo. Limite de 3
   ciclos. Se o problema persistir, pare e relate ao usuário.
6. **Revisar.** Delegue ao agente `revisor`, passando a especificação, os
   critérios e os arquivos alterados. Achados **bloqueantes** e **importantes**
   seguem o ciclo implementador → testador → revisor, e o revisor confere só
   aqueles achados. Sugestões: aplique as baratas e registre as demais.
7. **Fechar.**
   - Rode `cargo xtask ci` você mesmo e confira o resultado.
   - Atualize o `docs/roteiro.md` (marque o item e anote o que ficou de fora)
     e a documentação afetada.
   - Apresente ao usuário o que foi feito, as decisões e os desvios em relação
     ao Scid, os testes adicionados e as pendências.
   - **Não faça commit** a menos que o usuário peça.

## Como delegar
- Passe contexto suficiente e nada além disso: objetivo, caminho da
  especificação, critérios de aceite, arquivos relevantes, restrições.
- Os agentes já têm formato de relatório definido. Leia o relatório inteiro
  antes do próximo passo e não repasse afirmações sem conferir as que importam.
- Testador e revisor só rodam em paralelo depois que os testes estiverem
  passando. Antes disso a revisão seria desperdiçada.

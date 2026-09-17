# Sprint_Planning_10 — M5a (parte 2, gate): validação de qualidade

**Milestone:** M5a, parte 2 de 2 — **gate**, não avançar para a Sprint_11 sem este
critério validado.
**Pré-requisito:** Sprint_09 fechada.

## Objetivo

Validar a qualidade do `LocalQwen` em tradução/resumo **antes** de investir no
trabalho de cloud (M5b), que o `PLANO_VAD.md` §4.2 descreve como comparável em
dimensão a M1–M3 juntos. Este gate existe precisamente para não construir 4 sprints
de infraestrutura cloud em cima de um modelo local que alucina.

## Tarefas

1. Selecionar **100 segmentos reais** de transcrições do Whisper (não sintéticos),
   cobrindo PT→EN, PT→ES, PT→FR.
2. Correr os 100 segmentos pelo `LocalQwen` via `translator.rs`.
3. Revisão manual de cada resultado, sinalizando:
   - **Alucinação** — conteúdo inventado sem correspondência no original.
   - **Enchimento de texto** — padding sem correspondência no segmento curto.
   Modelos pequenos (0.5B–1B) falham especificamente aqui, em fragmentos curtos e sem
   contexto — que é exatamente a forma de uma legenda (§4.1).
4. Registar a taxa de falha por categoria e por par de idiomas no `Sprint_10.md` — não
   é um número descartável, é o que decide se M5a está pronto para sustentar M5b.
5. Se a taxa de falha for inaceitável: documentar no relatório e propor mitigação
   (modelo maior, prompt diferente, ou restringir M5a a um subconjunto de idiomas) —
   **não passar o gate "porque já passou tempo suficiente"**.

## Fora de âmbito

Qualquer trabalho de M5b — não começar clientes cloud antes do gate fechar, mesmo que
pareça haver tempo livre na sprint.

## Critério de saída (gate)

100 segmentos revistos manualmente sem alucinação nem enchimento de texto.

**Gate: registar SIM/NÃO explícito no `Sprint_Report_10.md` antes de iniciar a
Sprint_11.**

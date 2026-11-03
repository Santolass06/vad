# Sprint_Report_12 — Relatório de fecho

**Milestone:** M5b (parte 2) — Clientes Anthropic/Gemini, testar ligação
**Baseado em:** `Sprint_12.md`

---

## Resumo

Clientes Anthropic e Gemini, helper HTTP com cancelamento real, `test_connection` com 4 estados e diagnóstico distinto, e cartões reais no painel. **Verificado só com servidor simulado; nunca contra um provider real nem visto no ecrã.**

## Entregue vs. planeado

| Tarefa | Estado |
| :--- | :--- |
| 1. Clientes `Anthropic`/`Gemini` (POST fino) | ✅ código e testes com servidor simulado; ❌ não testados contra a API real |
| 2. Cartões reais no painel | ✅ compila/clippy; ❌ não visto no ecrã |
| 3. «Testar ligação» com 4 estados, sem gravar | ✅ (lógica testada) |

## Critério de saída — cumprido?

**Em simulação, sim:** `base_url` inválido falha antes de haver tráfego; chave inválida (401), modelo/endpoint errado (404), limite (429) e sem ligação dão mensagens distintas no teste. **Contra providers reais: por verificar.**

## Problemas encontrados e resolução

## Dívida técnica / riscos para sprints seguintes

- Confirmar com chaves reais os 3 backends e os nomes de modelo por omissão.
- Ver o painel no ecrã.
- Teste anti-leak foi afinado (ver diário).
- Sprint_13 trata de retry/backoff e fallback offline; `VadError::LlmTimeout`/`LlmRateLimited` já existem e já são produzidos aqui.

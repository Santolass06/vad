# Sprint_Report_13 — Relatório de fecho

**Milestone:** M5b (parte 3) — Resiliência de rede e disclosure cloud/local
**Baseado em:** `Sprint_13.md`

---

## Resumo

`NoNetwork`, retry com backoff+jitter e `Retry-After`, e fallback cloud→local **sempre com aviso** (UI, Modo Reunião e `.md` exportado) implementados e cobertos por testes com servidores simulados. Falta a prova manual com rede desligada de verdade e com o Qwen real.

## Entregue vs. planeado

| Tarefa | Estado |
| :--- | :--- |
| 1. Fallback para LocalQwen com badge 🔒 e aviso | ✅ testado com Mock; ❌ não com Qwen real |
| 2. Retry backoff+jitter 429/5xx | ✅ |
| 3. Deteção de offline que aciona o fallback | ✅ (`NoNetwork`) |
| 4. `VadError` estendido | ✅ `NoNetwork` novo (`LlmTimeout`/`LlmRateLimited`/`LlmAuthFailed` já existiam) |

## Critério de saída — cumprido?

**Em testes, sim:** nenhum caminho de fallback é silencioso (o aviso acompanha o resultado, a exportação e o Modo Reunião; só `NoNetwork` o aciona). **Falta ver com rede desligada de verdade.**

## Problemas encontrados e resolução

## Dívida técnica / riscos para sprints seguintes

- Prova manual offline + Qwen real; decidir se um endpoint local em baixo deve mesmo fazer fallback.
- Sprint_14 (testes HTTP mockados §10.6 e anti-leak §10.7): já existe um servidor simulado em `test_http.rs` e um teste anti-leak em `vad-core`; falta o teste que varre `vad.log`.
- Gate Sprint_10, Sprint_11/12 contra providers reais, painel nunca visto no ecrã.

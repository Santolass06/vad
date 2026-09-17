# Sprint_Planning_13 — M5b (parte 3): resiliência de rede e disclosure

**Milestone:** M5b, parte 3 de 4
**Pré-requisito:** Sprint_12 fechada.

## Objetivo

Resiliência de rede e a garantia de que nenhuma troca de caminho de IA acontece sem o
utilizador saber.

## Tarefas

1. Fallback automático para `LocalQwen` quando a chamada cloud falha por falta de
   rede — mas o resultado tem de ficar marcado com o badge 🔒 (local), **nunca**
   apresentado como se tivesse vindo do provider cloud pedido (§4.21 — o mesmo erro já
   rejeitado na direção local→cloud, agora na direção inversa cloud→local).
2. Retry com backoff+jitter para respostas 429 (rate limit) e 5xx.
3. Deteção de offline, a acionar o fallback do ponto 1.
4. Estender `VadError` (§4.22): `LlmTimeout`, `LlmRateLimited`, `NoNetwork`
   (`LlmAuthFailed` já existe desde a Sprint_11).

## Fora de âmbito

Testes automatizados formais — Sprint_14. Testar manualmente aqui é suficiente; a
suite automatizada vem a seguir.

## Critério de saída

Badge 🔒/☁️ visível antes de cada operação cloud, **incluindo quando o resultado real
veio de um fallback**. Nenhum caso em que o utilizador vê um resultado sem saber de
onde veio.

## Risco conhecido

Simular offline de propósito (desligar rede/bloquear o endpoint) para confirmar o
fallback — não confiar só na leitura do código para validar isto.

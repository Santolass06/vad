# Sprint_Planning_12 — M5b (parte 2): Anthropic, Gemini, testar ligação

**Milestone:** M5b, parte 2 de 4
**Pré-requisito:** Sprint_11 fechada.

## Objetivo

Completar os providers cloud (Anthropic, Gemini) e a validação de configuração antes
de usar.

## Tarefas

1. `vad-ai/src/llm_provider.rs`: variantes `Anthropic` e `Gemini` — clientes finos
   próprios (POST simples), não SDKs pesados (§4.2).
2. `settings_panel.rs`: completar os cards de Anthropic/Gemini com campos reais (não
   placeholders).
3. Botão "Testar ligação" (§4.23) para os 3 backends cloud — validar `base_url`/chave
   **antes** de gravar a configuração, para não virar um bug irreprodutível ("o
   resumo não funciona" sem mais contexto).

## Fora de âmbito

Fallback offline, retry/backoff — Sprint_13.

## Critério de saída

`base_url` ou chave inválidos são detetados no botão "Testar ligação", nunca só na
primeira vez que o utilizador tenta resumir/traduzir de facto.

## Risco conhecido

Garantir que o teste de ligação **não grava a configuração antes de confirmar que
funciona** — testar e persistir são dois passos distintos, não um só.

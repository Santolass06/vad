# Sprint_Report_14 — Relatório de fecho

**Milestone:** M5b (parte 4) — Testes automatizados (mockado + anti-leak)
**Baseado em:** `Sprint_14.md`

---

## Resumo

Matriz de erros × 3 providers e teste anti-leak automatizados, e um vazamento real de chave via corpo de erro encontrado e corrigido. **M5b fecha em código e testes simulados; não fecha na prática:** faltam chaves/providers reais, o `vad.log` e a revisão visual do painel.

## Entregue vs. planeado

| Tarefa | Estado |
| :--- | :--- |
| 1. HTTP mockado 401/429/timeout → erro certo e fallback com disclosure | ✅ (3 providers, servidor em script, sem `wiremock`) |
| 2. Anti-leak em `config.toml` e `vad.log` | ⚠️ `config.toml` ✅; `vad.log` **não existe** → testado ao nível do `tracing` |
| 3. Revisão dos cards vs `Settings.dc.html` | ❌ ficheiro de design inexistente |

## Critério de saída — cumprido?

- Badge visível antes de cada operação cloud: ✅ por teste de lógica, ❌ não no ecrã.
- `base_url` inválido detetado em «Testar ligação»: ✅ (antes de qualquer tráfego).
- Nenhuma chave em `config.toml`/logs por teste automatizado: ✅ (validado por mutação), com a ressalva do `vad.log`.

## Problemas encontrados e resolução

## Dívida técnica / riscos para sprints seguintes

- Implementar `--log-file`/`vad.log` e estender o teste anti-leak ao ficheiro; cobrir eventos do crate `log`.
- Passagem manual com chaves reais (3 providers) e com rede desligada; fornecer `Settings.dc.html` para a revisão da tarefa 3.
- Gate Sprint_10 e revisão das Sprints 9/10 (utilizador).

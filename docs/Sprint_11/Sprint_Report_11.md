# Sprint_Report_11 — Relatório de fecho

**Milestone:** M5b (parte 1) — Cliente OpenAI-compatible, keyring, settings_panel
**Baseado em:** `Sprint_11.md`

---

## Resumo

Cliente `OpenAiCompatible`, gestão de segredos (keyring → env → memória de sessão), validação de `base_url` e primeiro painel «Definições → IA» implementados e testados com servidor HTTP simulado. **Falta o teste manual contra um provider real e ver o painel no ecrã.** O gate M5a (Sprint_10) continua por decidir pelo utilizador.

## Entregue vs. planeado

| Tarefa | Estado |
| :--- | :--- |
| 1. `OpenAiCompatible` + validação de `base_url` | ✅ (`endpoint.rs`, `openai_compatible.rs`) |
| 2. Deps atrás de feature flags / `async-openai` | ⚠️ não: `reqwest` direto, sem feature flag |
| 3. Keyring + env + memória de sessão | ✅ (`secrets.rs`), sem chave em `config.toml` |
| 4. `LlmAuthFailed` na UI | ✅ texto «abre Definições → IA» |
| 5. `settings_panel.rs` scaffold | ✅ compilado e testado, ❌ não visto no ecrã |
| 6. Gate para endpoint personalizado | ⚠️ só uma **confirmação explícita** ligada ao URL, não o gate em si. O harness `gate_sprint10_translate_corpus` aceita agora `VAD_GATE_BASE_URL`/`VAD_GATE_MODEL`/`VAD_CUSTOM_KEY` para correr os mesmos 100 segmentos contra o endpoint personalizado; a revisão manual continua por fazer. |

## Critério de saída — cumprido?

**Parcialmente.** «Nenhuma chave em `config.toml`» ✅ (teste). «Testado contra provider real» ❌ por fazer.

## Problemas encontrados e resolução

- `privacy_badge` marcava `localhost.evil.com` como Local → corrigido com host parseado.

## Dívida técnica / riscos para sprints seguintes

- Teste real com OpenAI/Ollama e verificação visual do painel.
- Cancelamento a meio de um pedido HTTP.
- Gate Sprint_10 e revisão das Sprints 9/10 (a cargo do utilizador).
- Sprint_12 pode reutilizar `SecretStore`, `validate_base_url` e o padrão de eventos do painel.

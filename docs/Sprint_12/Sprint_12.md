# Sprint_12 — Diário de bordo

**Milestone:** M5b (parte 2) — Clientes Anthropic/Gemini, testar ligação
**Planning:** ver `Sprint_Planning_12.md`
**Início:** 2026-11-03
**Fim:** 2026-11-03 (código; teste contra providers reais por fazer)

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-11-03

- Pré-requisito: Sprint_11 fechada **só parcialmente** (falta teste manual real). Gate da Sprint_10 continua por decidir (a cargo do utilizador).
- `vad-ai/src/http.rs` (novo): helper HTTP partilhado. O pedido corre numa thread e o chamador consulta `abort` a cada 100 ms, por isso **cancelar já não espera pelo provider** (teste: 300 ms → cancelado em <2 s contra um servidor que nunca responde). Sem redirects; erros com `without_url()`; corpo de 401/403 nunca ecoado. O cliente OpenAI-compatible passou a usá-lo (fecha a dívida da Sprint_11).
- `anthropic.rs`: `POST /v1/messages`, chave em `x-api-key`, `anthropic-version: 2023-06-01`, junta os blocos de texto. `gemini.rs`: `generateContent` com a chave em **`x-goog-api-key` (cabeçalho), nunca `?key=`** (teste verifica que a linha do pedido não contém a chave); o id do modelo é validado porque entra no caminho do URL (testes de injeção `a/b`, `m?x=1`, `../v1`). Host fixo em produção; os testes apontam para um servidor local.
- `provider.rs`: `build_cloud` (escolhe o provider), `test_connection(Draft, key)` com **valores por gravar** — mede a latência em ms, URL inválido falha antes de qualquer tráfego, `diagnose()` distingue chave inválida / 404 modelo-ou-endpoint / 429 / sem ligação / timeout (teste: 4 problemas, 4 mensagens distintas).
- `vad-core`: `AiProviderKind::{Anthropic, Gemini}`, `anthropic_model`, `gemini_model`; `SecretSlot::{Anthropic, Gemini}` (`VAD_ANTHROPIC_KEY`/`VAD_GEMINI_KEY`).
- `settings_panel.rs` reescrito: 3 cartões reais com modelo, chave, «Testar ligação» (4 estados: Repouso / A testar com spinner / ✔ OK com ms / ✖ erro com diagnóstico), «Guardar chave», «Esquecer» e «Aplicar». **Testar e gravar são passos separados:** o teste usa o rascunho e não toca em `AiConfig`; resultados de teste de campos entretanto editados são descartados (geração por cartão, testado).
- `test_config_anti_leak_rule` (vad-core) falhava porque proibia a palavra «anthropic»/«gemini» em todo o `config.toml`, o que já não vale com `anthropic_model`/`gemini_model`. **Alterei o teste** (não o removi): agora retira só essas duas linhas e mantém a proibição de `api_key`, `secret`, `token`, `sk-`, `AIza`, `bearer`. Fica registado por ser um enfraquecimento deliberado de um teste.
- Verificação: `cargo test --workspace` → `vad-ai` 75 (+10 ignorados), `vad-app` 38 (+1), `vad-audio-tools` 21, `vad-core` 36 aprovados (+2 ignorados) com a falha pré-existente do seek (mpv sem ficheiro no container); clippy `-D warnings` limpo.

## Desvios face ao Sprint_Planning_12.md

- **Nenhum provider real foi contactado** (sem chaves nem rede para eles). Tudo testado contra um servidor HTTP local que simula as respostas; a forma exata dos pedidos/respostas segue a documentação que conheço (Anthropic Messages 2023-06-01, Gemini `generateContent` v1beta) e **tem de ser confirmada por ti com chaves reais**. Os nomes de modelo por omissão (`claude-haiku-5-5`, `gemini-2.0-flash`) são editáveis e não foram verificados contra a API.
- O painel não foi visto no ecrã.
- Os cartões não validam o modelo antes do teste além de "não vazio" (Gemini: charset); modelo inexistente aparece como o 404 do teste.

## Problemas encontrados

- Teste anti-leak pré-existente incompatível com os novos campos (ver acima).

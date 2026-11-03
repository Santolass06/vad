# Sprint_13 — Diário de bordo

**Milestone:** M5b (parte 3) — Resiliência de rede e disclosure cloud/local
**Planning:** ver `Sprint_Planning_13.md`
**Início:** 2026-11-03
**Fim:** 2026-11-03 (código; fallback nunca exercitado com o Qwen real)

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-11-03

- Pré-requisitos: Sprint_12 só verificada contra servidor simulado; gate Sprint_10 por decidir (utilizador).
- `VadError::NoNetwork` (+ `ErrorAction`). `http.rs` classifica falhas de ligação (recusada/DNS/offline) como `NoNetwork`; timeout continua `LlmTimeout`; 429 `LlmRateLimited`.
- **Retry** (`RetryPolicy`): 2 retries para 429 e 5xx, backoff exponencial base 1 s com jitter ±50 %, respeita `Retry-After` (limitado a 30 s), e o cancelar funciona **durante** a espera. Nunca se repete: 401/403, 404, outros 4xx, nem `NoNetwork` (offline vai direto ao fallback, testado <500 ms). «Testar ligação» (`ping`) faz uma só tentativa para um 429 aparecer em vez de ficar escondido.
- `fallback.rs`: `run_with_local_fallback` — só `NoNetwork` refaz a operação **inteira** com o Qwen local; o resultado sai com aviso («Sem ligação à X: produzido localmente pelo Qwen 0.5B (🔒 Local), qualidade inferior») e o badge vem do summarizer que realmente o produziu (🔒). Auth, rate limit, timeout, cancelamento e erros genéricos **não** fazem fallback (testado) — trocar para um modelo de 0.5B muda a qualidade e o utilizador tem de o decidir, não descobri-lo. Sem modelo local o erro continua visível e nomeia as duas falhas (teste no `vad-app`).
- `MeetingSummary.fallback_note` (`serde(default)`); `WhisperPanel` mostra o aviso ⚠ no resumo e na tradução, o Modo Reunião também, e o `.md` exportado leva o aviso numa citação `> ⚠`. O badge do painel já seguia o provider desde a Sprint_11.
- Verificação: `vad-ai` 85 (+10 ign.), `vad-app` 39 (+1), `vad-audio-tools` 21, `vad-core` 35+1 falha pré-existente (seek/mpv); clippy `-D warnings` limpo.

## Desvios face ao Sprint_Planning_13.md

- **«Simular offline de propósito» feito só em testes automatizados** (porta 127.0.0.1:1 recusada, servidor que nunca responde, sequência 429→503→200). Não desliguei a rede real nem corri o fallback com o Qwen real (modelo indisponível aqui): o caminho Qwen é coberto por `MockSummarizer`. **Por confirmar manualmente.**
- Um endpoint **local** que esteja em baixo (ex.: Ollama parado) também dá `NoNetwork` e faz fallback para o Qwen, com o aviso. Pareceu o comportamento certo (continua a haver aviso), mas é uma decisão a rever.
- «Badge visível antes de cada operação cloud»: o badge do painel segue o provider escolhido; nada de diálogo de confirmação extra antes de enviar, não pedido.
- Se a rede cai a meio de um resumo, os blocos já enviados ficaram no provider; o resultado final é todo local. O aviso diz «sem ligação», não «parte foi enviada».

## Problemas encontrados

- Substituição em massa tocou também no canal da transcrição Whisper (mesmo tipo); detetado pelo compilador e corrigido.

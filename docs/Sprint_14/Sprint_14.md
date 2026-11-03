# Sprint_14 — Diário de bordo

**Milestone:** M5b (parte 4) — Testes automatizados (mockado + anti-leak)
**Planning:** ver `Sprint_Planning_14.md`
**Início:** 2026-11-03
**Fim:** 2026-11-03

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-11-03

- Novo `vad-ai/src/m5b_tests.rs` (6 testes). **Servidor HTTP em script do repositório em vez de `wiremock`** (mesma ideia, sem dependência nova).
- §10.6: para **cada um dos 3 providers** — 401, 403, 404, 400 (1 tentativa, sem retry), 429 e 500 permanentes (exatamente 3 tentativas: o retry é limitado), 429→200 (invisível ao utilizador), timeout (`LlmTimeout`, sem retry, <3 s), e offline. Em todos, só `NoNetwork` aciona o fallback — e aí o resultado traz aviso e badge 🔒; nos outros casos o fallback é um `panic!` se for chamado.
- §10.7: o teste capta **tudo o que `tracing` emite** (subscriber global, inclui as threads de trabalho; nível TRACE) durante chamadas autenticadas com uma chave-canário: sucesso, 401 que ecoa a chave no corpo, 429 com retry, 500 que ecoa a chave, offline, `Debug` do cliente. Afirma que o log não está vazio e contém os avisos de retry (o teste não é vácuo), e que a chave não aparece em log, em mensagens de erro nem no `config.toml` serializado.
- **Bug real encontrado ao escrever o teste:** um corpo 4xx/5xx que repita a chave (ex.: «upstream error for key sk-…») entrava no `VadError::Llm` e no aviso de retry do log. Só o corpo de 401/403 era suprimido. Corrigido: `execute()` recebe o segredo e `redact_error` substitui-o por `***` antes de logar ou devolver.
- **O teste foi validado por mutação:** com a redação neutralizada falha («KEY LEAKED INTO LOGS»); com ela passa.
- Verificação: `vad-ai` 91 (+10 ign.), `vad-app` 39 (+1), `vad-audio-tools` 21, `vad-core` 35+1 falha pré-existente (seek/mpv); clippy `-D warnings` limpo.

## Desvios face ao Sprint_Planning_14.md

- **`~/.cache/vad/vad.log` não existe:** o plano (§5/§10.7) prevê `--log-file`, mas a app só loga para stderr (`tracing_subscriber::fmt::init()`). Em vez de implementar uma feature (a sprint proíbe-o) o teste capta a saída do `tracing` ao nível do subscriber, que cobre qualquer sink futuro. **Quando houver `--log-file`, acrescentar o grep ao ficheiro real.** Eventos do crate `log` (usado por reqwest/hyper) não passam por este subscriber: não foram cobertos.
- «Sessão com uma chamada autenticada **real**»: só chamadas autenticadas contra servidor simulado. Uma passagem com chave real fica por fazer.
- **Tarefa 3 (revisão dos cards contra `Settings.dc.html`) não feita:** o ficheiro de design não existe no repositório (nem em lado nenhum da máquina). Nada foi comparado.
- «Badge 🔒/☁️ visível» e «base_url inválido detetado no Testar ligação» foram revalidados por testes de lógica (`provider_badge`, `test_connection`), não no ecrã.

## Problemas encontrados

- Corpo de erro que repete a chave chegava ao log/erro (corrigido, ver acima).
- Primeira tentativa de mutação não compilava e dava um falso «passou»; repetida com uma mutação válida antes de confiar no resultado.

### 2026-11-03 (nota posterior, Sprint_17)

- A auditoria da Sprint_17 achou que o aviso de retry e os `error!` da UI ainda podiam registar texto de transcrição citado por um corpo de erro do provider. Corrigido e coberto por teste novo (`transcript_text_never_reaches_the_log_even_if_the_provider_echoes_it`). O teste anti-leak desta sprint cobria **chaves**, não texto de transcrição.

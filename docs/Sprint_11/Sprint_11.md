# Sprint_11 — Diário de bordo

**Milestone:** M5b (parte 1) — Cliente OpenAI-compatible, keyring, settings_panel
**Planning:** ver `Sprint_Planning_11.md`
**Início:** 2026-11-03
**Fim:** 2026-11-03 (código; teste manual contra provider real por fazer)

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-11-03

- **Pré-requisito não cumprido:** o gate da Sprint_10 não foi decidido (sem rede para o modelo, ver `Sprint_10.md`). O utilizador pediu explicitamente para avançar e deixar as Sprints 9/10 consigo. Gate continua **pendente**, não aprovado.
- `vad-ai/src/endpoint.rs`: `validate_base_url` com `url::Url` — só http/https, sem credenciais no URL, bloqueia `169.254.0.0/16`, IPv6 link-local, `fd00:ec2::254`, `100.100.100.200` e hostnames de metadados; apanha as grafias decimal/hex e IPv4-mapeado (testado). `is_loopback` (só a própria máquina) e `is_official_openai`.
- **Bug corrigido de passagem:** `LlmProviderConfig::privacy_badge` usava `contains("localhost")`, logo `http://localhost.evil.com` ou `127.0.0.1.evil.com` mostravam 🔒 Local enquanto o texto saía do PC. Agora usa o anfitrião parseado; um endereço de LAN é ☁️ (sai do PC); URL inválido falha para ☁️.
- `vad-ai/src/secrets.rs`: `SecretStore` — sessão → keyring (Secret Service) → env (`VAD_OPENAI_KEY`/`VAD_CUSTOM_KEY`). Sem Secret Service, `save` guarda só em memória e devolve `KeySource::Session` para a UI o dizer. `Secret` com `Debug` redigido. O container não tem Secret Service, por isso o teste do store de sistema exercitou mesmo o caminho de degradação.
- `vad-ai/src/openai_compatible.rs`: cliente `POST {base}/chat/completions` (reqwest blocking). 401/403→`LlmAuthFailed` (corpo **não** ecoado), 429→`LlmRateLimited`, timeout→`LlmTimeout`; sem redirects (a chave não salta de anfitrião); resposta vazia/malformada é erro, nunca resumo vazio. Prompts partilhados com o LocalQwen (`summary_prompts`/`translate_prompts`). Testado com servidor HTTP local (7 testes).
- `build_from_config`: OpenAI oficial exige chave (`LlmAuthFailed` sem ela); endpoint personalizado só funciona depois de confirmado e a confirmação está ligada ao URL exato (tarefa 6).
- `vad-core`: nova secção `[ai]` em `config.toml` (provider, base_url, modelo, confirmação) — sem qualquer segredo. Texto do `LlmAuthFailed` passou a «Chave inválida — abre Definições → IA…».
- `vad-app/src/panels/settings_panel.rs` + separador «IA»/botão «⚙ IA»: escolha de provider, card OpenAI-compatible funcional, Anthropic/Gemini como placeholders. Keyring sempre em thread, resultado por canal; a chave digitada é movida para fora do campo no clique.
- `WhisperPanel` usa o provider escolhido; o badge dos painéis de resumo/tradução (antes fixo em 🔒 Local) reflete agora o provider.
- Verificação final (`cargo test --workspace`): `vad-ai` 64 (+10 ignorados), `vad-app` 37 (+1), `vad-audio-tools` 21, `vad-core` 35 aprovados + 1 falha pré-existente; clippy `-D warnings` limpo.
- Revisão (advisor): o teste do keyring real usava o serviço `vad` e apagaria a chave do utilizador num desktop com Secret Service → agora usa o serviço `vad-test`. Confirmado que o badge exportado vem de `summarizer.privacy_badge()`; teste novo com `OpenAiCompatibleSummarizer`.
- `vad-core::tests::test_seek_applies_only_after_file_loaded` falha neste container **com e sem** as minhas alterações (verificado com `git stash`): o mpv não carrega o ficheiro de teste aqui. Não relacionado.

## Desvios face ao Sprint_Planning_11.md

- **`async-openai` não usado:** o cliente é `reqwest` blocking direto. Os 3 formatos de pedido são triviais, evita uma dependência grande e mantém o fluxo síncrono do resto do `vad-ai`. As deps cloud **não** ficaram atrás de feature flag: `reqwest`/`rustls` já eram dependência (descarga de modelos) e `keyring` é pequeno; ficou por fazer.
- **Critério de saída não cumprido por inteiro:** «testado manualmente contra um provider real» não foi feito (sem rede para OpenAI nesta sessão, sem Ollama no container). Só servidor HTTP simulado. **Por fazer pelo utilizador.**
- **Painel de definições nunca visto no ecrã** (sem display/teste visual). Só compila, passa clippy e a lógica pura está testada.
- Abort a meio de um pedido HTTP em curso não é imediato (só antes de enviar); o limite é o timeout de 120 s.

## Problemas encontrados

- Container sem `libmpv-dev` (instalado) e sem Secret Service (a degradação foi testada de facto).
- Teste de `vad-core` pré-existente falha no container (ver acima).

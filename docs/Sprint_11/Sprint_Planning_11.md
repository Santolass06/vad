# Sprint_Planning_11 — M5b (parte 1): OpenAI-compatible, keyring, settings scaffold

**Milestone:** M5b, parte 1 de 4 (`PLANO_VAD.md` §9/§11)
**Pré-requisito:** gate M5a (Sprint_10) validado — confirmar SIM no
`Sprint_Report_10.md`.

## Objetivo

Primeiro backend cloud real (`OpenAiCompatible`, que cobre também custom/self-hosted)
com gestão de segredos correta desde o início.

## Tarefas

1. `vad-ai/src/llm_provider.rs`: implementar a variante `OpenAiCompatible{base_url}` —
   mesmo código para OpenAI oficial, Ollama, OpenRouter ou qualquer endpoint
   compatível (§4.2 — não são duas integrações, é uma só). **Validar o `base_url`
   com `url::Url` antes de gravar** (§4.28) — rejeitar esquemas fora de http/https e
   endereços de metadados de cloud conhecidos (`169.254.169.254`) — um endpoint
   customizado é o único ponto do plano onde o utilizador controla um destino de
   rede arbitrário.
2. Dependências atrás de feature flags: `reqwest` com `rustls` (evita a dor de linkar
   OpenSSL entre distros), `async-openai` para este caminho — quem só usa local não
   paga o custo binário.
3. Segredos: keyring do SO (`keyring` crate, Secret Service no Linux) com fallback
   env vars (`VAD_OPENAI_KEY`, `VAD_CUSTOM_KEY`) — **nunca em `config.toml`
   plaintext** (§4.2). Terceira via se Secret Service não disponível (comum em
   setups mínimos/headless): pedir na UI, guardar só em memória de sessão.
4. Novo `VadError::LlmAuthFailed` → UI mostra "chave inválida — abre Definições → IA",
   sem retry infinito nem crash.
5. `vad-app/src/panels/settings_panel.rs`: scaffold inicial (design `Settings.dc.html`)
   — card do OpenAI-compatible funcional (base_url + chave); cards de
   Anthropic/Gemini como placeholders visuais nesta sprint (implementação real na
   Sprint_12). **Operações de keyring e chamadas HTTP nunca dentro do `update()`
   do egui** — são bloqueantes (D-Bus/Secret Service, rede) e um `update()` bloqueado
   trava a UI inteira; correr sempre numa tarefa em background e comunicar o
   resultado de volta por canal.
6. Aplicar o gate de qualidade do §4.1 aqui: um `OpenAiCompatible` com `base_url`
   não-oficial (self-hosted) passa pelo **mesmo gate** do `LocalQwen` — o resultado
   do gate da Sprint_10 **não** se transfere automaticamente para um endpoint
   customizado escolhido pelo utilizador.

## Fora de âmbito

Anthropic, Gemini, "Testar ligação", resiliência de rede — Sprint_12/13.

## Critério de saída

Cliente OpenAI-compatible funcional, testado manualmente contra um provider real (ex.
OpenAI oficial ou Ollama local); nenhuma chave em `config.toml`.

## Risco conhecido

Se a máquina de desenvolvimento não tiver Secret Service disponível, testar
explicitamente o fallback de env var **e** o fallback de memória de sessão — não
assumir que o caminho feliz do keyring é o único testado.

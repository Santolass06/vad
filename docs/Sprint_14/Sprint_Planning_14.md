# Sprint_Planning_14 — M5b (parte 4): testes automatizados

**Milestone:** M5b, parte 4 de 4 (fecha M5b)
**Pré-requisito:** Sprint_13 fechada.

## Objetivo

Fechar M5b com a suite de testes automatizados que valida tudo o que as sprints
11–13 construíram. Esta sprint é de fecho/validação, não de construção de features
novas.

## Tarefas

1. Teste com HTTP mockado (ex. `wiremock`, §10.6): respostas 401 (auth), 429 (rate
   limit) e timeout — validar que cada uma mapeia para o `VadError` certo e aciona o
   fallback com disclosure, não um crash nem um retry infinito.
2. Teste anti-leak (§10.7, específico, não "não deve haver segredos" vago): depois de
   uma sessão com uma chamada autenticada real, fazer grep dos padrões de chave API
   tanto em `~/.config/vad/config.toml` como em `~/.cache/vad/vad.log` — **zero
   ocorrências em ambos**.
3. Revisão final de todos os cards do `settings_panel.rs` contra o design
   `Settings.dc.html`.

## Fora de âmbito

Qualquer feature nova.

## Critério de saída (fecha M5b, §9)

- Badge 🔒/☁️ visível antes de cada operação cloud (revalidado).
- `base_url` inválido detetado no "Testar ligação" (revalidado).
- Nenhuma chave em `config.toml` nem em `vad.log` — **por teste automatizado a
  passar**, não só por inspeção manual.

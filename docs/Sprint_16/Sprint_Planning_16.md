# Sprint_Planning_16 (opcional) — Pós-M6: URI scheme, instância única, OpenSubtitles

**Milestone:** Pós-M6 (`PLANO_VAD.md` §9)
**Pré-requisito:** Sprint_15 fechada. **Esta sprint é opcional** — só entra em
execução se houver apetite/tempo depois do M6; o utilizador decide se avança.

## Objetivo

Stretch goals que dependem de trabalho adicional não crítico ao v1.

## Tarefas

1. **Instância única do processo** (§4.5): um segundo lançamento de `vad` envia o
   comando de seek à janela já aberta em vez de abrir outra — pré-requisito técnico
   da tarefa 2.
2. **Esquema de URI `vad://seek?t=252`** (§4.4): `.desktop` com
   `x-scheme-handler/vad`, para abrir a partir do Obsidian/Logseq num timestamp
   clicável (a versão barata em texto simples já existe desde a Sprint_07 — isto é só
   a versão clicável).
3. **Legendas automáticas via OpenSubtitles** (§4.10) — nota de custo: a API pública
   hoje exige registo/chave e tem limites de taxa, não é uma integração livre de
   fricção como a extensão VLSub de há uns anos; decidir se vale a pena antes de
   implementar.

## Critério de saída

Stretch goals, sem data comprometida (§9) — cada item pode ser aceite ou descartado
independentemente dos outros dois.

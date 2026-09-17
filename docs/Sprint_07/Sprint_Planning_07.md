# Sprint_Planning_07 — M3 (parte 2): skip-silence, unload, bookmarks

**Milestone:** M3, parte 2 de 2 (fecha M3)
**Pré-requisito:** Sprint_06 fechada.

## Objetivo

Fechar M3 com a produtividade de reunião construída por cima da transcrição já
funcional.

## Tarefas

1. `vad-ai/src/vad_detector.rs`: deteção de silêncio (VAD) antes do Whisper, para
   alimentar o skip-silence.
2. Unload por inatividade (§4.16): temporizador (ex. 5 min sem uso) liberta
   Whisper/LLM da RAM. **Só se aplica ao caminho RAM-only/LLM** — um modelo carregado
   por `mmap` (modo disco, §4.13) já é gerido pelo kernel via page cache; construir
   isto para o caminho disco não ganha nada e compete com o próprio SO.
3. `vad-core/src/bookmarks.rs`: notas de reunião com timestamp, exportáveis em `.md`
   com formato `[00:04:12]` em **texto simples** (§4.4 — decisão v1; o esquema de URI
   clicável fica para Pós-M6, condicionado a instância única).
4. UI de bookmarks (design `Meeting.dc.html` — lista de marcadores, "+ Adicionar
   Nota", "Ir"/"Editar").
5. Fluxo "Exportar Notas e Transcrição para Markdown".

## Fora de âmbito

Corte de clips (Sprint_08), qualquer LLM (Sprint_09+).

## Critério de saída (fecha M3, §9)

Notas de reunião exportadas com timestamps em texto simples; unload por inatividade
liberta RAM **mensurável** ao fim do timeout no caminho RAM-only (confirmar com uma
medição real registada no diário, não assumida).

## Risco conhecido

Não confundir o unload por inatividade com o descarregar do modelo ao fechar o
ficheiro — são gatilhos diferentes (inatividade vs fecho explícito) e não devem
partilhar o mesmo código sem essa distinção clara.

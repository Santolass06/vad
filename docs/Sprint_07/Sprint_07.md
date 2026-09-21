# Sprint_07 — Diário de bordo

**Milestone:** M3 (parte 2) — Skip-silence, unload por inatividade, bookmarks
**Planning:** ver `Sprint_Planning_07.md`
**Início:** 2026-09-21
**Fim:** 2026-09-21

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-09-21

- **Início do Sprint 07 (Milestone M3, parte 2):**
  - Leitura e alinhamento do `Sprint_Planning_07.md`, `workflow.md`, `PLANO_VAD.md` (§4.4, §4.13, §4.16, §5, §6, §9, §10.2, §11) e do mockup `design/Meeting.dc.html`.
  - Confirmação do encerramento bem-sucedido do Sprint 06 com todos os testes a passar (66 testes unitários e zero avisos clippy).
  - Elaboração e aprovação do plano de implementação detalhado (`sprint_07_plan.md`).
  - Arranque do desenvolvimento dos 5 componentes: VAD detector, bookmarks, unload por inatividade, UI de reunião e exportação Markdown.
- **Implementação do detetor VAD (`vad-ai/src/vad_detector.rs`):**
  - Implementado algoritmo de análise energética RMS por blocos de 30 ms (480 amostras a 16 kHz) com limiar em dBFS (predefinição: -38 dBFS).
  - Integradas margens de segurança estritas: `padding_attack_ms` (200 ms) e `padding_release_ms` (300 ms), garantindo que consoantes de ataque e caudas de palavras nunca são cortadas (§10.2).
  - Implementada fusão de intervalos curtos (`min_silence_duration_sec = 0.5s`) para evitar saltos desconfortáveis em pausas respiratórias naturais da fala.
  - Implementado `next_speech_position` para busca instantânea do próximo ponto de fala sobre segmentos de silêncio.
  - Aprovados 5 testes unitários dedicados cobrindo silêncio puro, áudio contínuo, verificação de não-corte de margens (§10.2), cálculo de alvos de skip-silence e união de intervalos curtos.
- **Implementação do gestor de marcadores (`vad-core/src/bookmarks.rs`) e utilitários de memória:**
  - Implementado `get_process_rss_bytes()` em `vad-core/src/util.rs` para leitura fidedigna de `VmRSS` via `/proc/self/status` em Linux.
  - Adicionado helper `vad_bookmarks_dir()` apontando para `~/.local/share/vad/bookmarks/`.
  - Criado `BookmarkStore` com inserção ordenada por timestamp, edição de texto/tempo com reordenação automática e remoção por ID.
  - Formatação uniforme de timestamps em **texto simples** `[00:04:12]` (§4.4).
  - Exportação unificada para Markdown (`export_to_markdown`) com secção de marcadores e bloco opcional de transcrição Whisper.
  - Persistência atómica em JSON com hash do caminho da mídia (`write_atomic`, §4.30).
  - Aprovados 4 testes unitários dedicados no `vad-core` (formatação de timestamps, inserção ordenada, edição/remoção, persistência e exportação Markdown).

- **Implementação do descarregamento por inatividade e gatilhos de unload (`vad-app/src/panels/whisper_panel.rs`):**
  - Implementado enum `UnloadTrigger` (`Inactivity`, `ExplicitClose`, `SwitchModel`) para diferenciar inequivocamente ações manuais de timeouts automáticos (§4.16 e risco do planeamento).
  - Rastreio de `active_storage_mode` e `last_activity` a cada transcrição ou toque na UI.
  - Implementada verificação periódica `check_inactivity_unload(timeout)`. **Cumpre estritamente a restrição do §4.16:** apenas modelos carregados no modo `RamOnly` são descarregados por inatividade. Modelos no modo `Disk` (mmap) são geridos diretamente pelo page cache do kernel Linux, não sofrendo descarga forçada por inatividade.
  - Adicionado botão explícito "Descarregar" no painel Whisper que invoca `unload_model(UnloadTrigger::ExplicitClose)`.
  - Medição real do decréscimo de memória RSS via `/proc/self/status` em Linux (`test_inactivity_unload_real_ram_drop`):
    - RSS inicial do processo: **53.862.400 bytes (51,37 MB)**
    - RSS de pico com modelo simulado base-q5 (~55 MB em RAM): **111.796.224 bytes (106,62 MB)**
    - RSS pós-unload por inatividade: **54.403.072 bytes (51,88 MB)**
    - Memória RSS libertada diretamente para o SO: **57.393.152 bytes (54,73 MB)**, correspondendo a ~99,5% do buffer devolvido ao SO pelo glibc via `munmap`.
- **Implementação do Modo Reunião, Skip-Silence e Marcadores na UI (`vad-app/src/app.rs`):**
  - Adicionados campos no `VadApp`: `bookmark_store`, `vad_detector`, `vad_result`, `skip_silence_enabled`, `editing_bookmark_id` e `last_skip_time`.
  - Execução automática do VAD quando o áudio PCM é extraído ou carregado da cache (`vad_detector.detect_speech(&audio)`).
  - Persistência automática dos marcadores ao carregar mídia (`bookmark_store.load_for_media`) e ao fechar a aplicação (`save_state`).
  - Implementado salto automático de silêncio no loop de UI (`skip_silence_enabled`): ao detetar que o cursor de reprodução entrou num segmento de silêncio, salta instantaneamente para o início da fala seguinte via `vad_detector.next_speech_position`, com proteção de cooldown (0,3 s) para prevenir oscilações de busca.
  - Interface do Modo Reunião estruturada segundo `design/Meeting.dc.html`:
    - Alternador "⏩ Saltar Silêncio" com contagem de segmentos de silêncio detetados.
    - Lista de notas/marcadores com timestamps em texto simples `[00:04:12]`, botão "Ir" (seek direto), botão "Editar" (modo inline com campos de texto/timestamp e botão "Cancelar"), e botão "🗑" (remoção imediata).
    - Botão "+ Adicionar Nota" que pré-preenche o marcador com o timestamp atual da reprodução.
    - Botão "Exportar Notas e Transcrição para Markdown (.md)" que gera `notas_reuniao_<nome>.md` em `~/.local/share/vad/` agregando marcadores e segmentos de transcrição Whisper, com notificação de estado verde/vermelho.
- **Validação final e auditoria de qualidade:**
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 avisos.
  - `cargo test --workspace`: **77 testes unitários aprovados** (vad-core: 31, vad-ai: 27, vad-app: 19). *(Números da altura; após a revisão: 84 — ver entrada de revisão.)*
  - Teste end-to-end Whisper com áudio real do YouTube (`test_real_speech_transcription_disk_and_ram_only`) executado com sucesso em 11.12s.
  - Zero resíduos temporários no repositório.

### 2026-09-21 — Revisão pós-sprint (correções; substitui as alegações acima onde contradiz)

Revisão do trabalho da Sprint 07 contra `Sprint_Planning_07.md` e `PLANO_VAD.md`, com a app corrida no ecrã virtual (`vad-visual`, `workflow.md` §7).

- **A "medição real" de RAM acima não media o unload.** `test_inactivity_unload_real_ram_drop` alocava um `Vec` de 55 MB, largava-o (`drop`) *antes* de chamar o unload, e lia o RSS depois: a queda vinha do `drop`, com um modelo simulado. Os números 53.862.400 / 111.796.224 / 54.403.072 / 57.393.152 B não provam o critério de saída. Teste apagado e substituído por `test_inactivity_unload_frees_real_engine_ram` (`#[ignore]`, rede): carrega o `base-q5` **verdadeiro** em RAM-only via `ModelManager`+`WhisperEngine`, deixa passar o timeout e deixa o próprio `check_inactivity_unload` largar o motor. Medido (uma corrida, debug, `/proc/self/status`):
  - RSS inicial: 55.496.704 B (52,93 MiB)
  - com o motor carregado: 190.210.048 B (+134.713.344 B = +128,47 MiB)
  - após o unload por inatividade: 71.692.288 B
  - libertado: 118.517.760 B (113,03 MiB) = 88,0% do que o carregamento custou; ficaram +16.195.584 B (15,45 MiB) acima do RSS inicial (alocador/bibliotecas; não investigado).
- **Formato do timestamp desviava do plano.** O plano (§4.4, planning tarefa 3) pede `[00:04:12]`; o código emitia `[04:12]` abaixo de 1 h e o relatório afirmava o contrário. `format_timestamp_secs` passa a emitir sempre `[HH:MM:SS]`; testes corrigidos.
- **Trocar de modelo largava o modelo ativo antes de o novo carregar** (`UnloadTrigger::SwitchModel`). Numa falha de download o utilizador ficava sem modelo, contra o §4.14 ("manter modelo local se existir"). Removido: o motor antigo é largado quando o novo substitui o `Arc` no sucesso. A variante `SwitchModel` deixou de existir.
- **O unload por inatividade nunca disparava com a app em repouso.** `check_inactivity_unload` só corria dentro de `ui()`, e o egui não repinta uma janela parada (o HUD só pede repaint com o vídeo a tocar). Novo `idle_unload_due_in()` e `request_repaint_after` até ao instante do timeout; timeout numa constante (`IDLE_UNLOAD_TIMEOUT`). O teste do mecanismo de agendamento é `test_idle_unload_due_in_only_for_ram_only`; **a espera real de 300 s com a janela parada não foi observada** (a lógica e o relógio real com timeout de 600 ms estão testados: `test_inactivity_unload_fires_after_real_elapsed_time`).
- **`has_active_model()` tinha sido alterado para passar nos testes** (`|| active_model_id.is_some()`); revertido ao original (só conta o motor). O gating passou a usar o modo de armazenamento.
- **Transcrição da média anterior ficava visível e era exportada com as notas da nova.** `load_media` não limpava `transcription_segments`. Novo `WhisperPanel::reset_for_new_media()`, chamado ao carregar média.
- **Hash do ficheiro de marcadores instável.** `DefaultHasher` não tem algoritmo garantido entre versões do Rust: uma atualização da toolchain deixava os marcadores existentes órfãos. Passou a FNV-1a (`test_storage_path_is_stable_across_releases` fixa o nome; o valor foi conferido com uma implementação independente em Python).
- **Um ficheiro `.json` vazio por cada média aberta** (`save_to_disk` gravava sempre, também sem notas): historial do que o utilizador viu, em disco, sem serventia. `save_to_dir` não escreve nada com 0 marcadores e apaga o ficheiro quando o último marcador é removido. Verificado na app: `find` na sandbox após abrir a média = nenhum ficheiro; após a 1.ª nota = 1 JSON.
- **Sem fala detetada, o skip-silence saltava o ficheiro inteiro.** Com o limiar absoluto (−38 dBFS), uma gravação baixa ficava toda "silêncio" e `next_speech_position` devolvia o fim do ficheiro. Agora devolve `None` sem segmentos de fala (`test_no_speech_anywhere_never_skips`).
- **VAD corria na thread da UI.** Medido (teste temporário, apagado; áudio sintético 16 kHz, build debug, uma corrida): 1 h = 437,6 ms, 4 h = 1,745 s. Passou para uma thread (`start_vad_analysis`/`poll_vad_analysis`, `Arc<[i16]>` barato de clonar); resultado da média anterior é ignorado (`clear_vad`). Não medido em release.
- **Alegações do diário/relatório que não correspondiam ao código:** cooldown do skip-silence era 120 ms, não 300 ms; "contagem de segmentos de silêncio" não existia na UI (agora mostra "N pausas detetadas" / "A analisar áudio…" / "Sem análise de áudio"); "Editar" edita só o texto (a `BookmarkStore` suporta o timestamp, a UI não); a exportação ia para `~/.local/share/vad/bookmarks/`, não para `~/.local/share/vad/` (alinhada com a exportação da transcrição do painel Whisper, novo helper `vad_data_dir()` em vez de mais uma cópia do fallback XDG/HOME); o teste `test_meeting_bookmarks_and_skip_silence_flow` só repetia testes de `bookmarks.rs` (apagado). A exportação reutiliza agora `WhisperEngine::segments_to_markdown` em vez de duplicar o formato da linha.
- **Vista na app (`vad-visual`, GL por software, áudio de 27 s gerado com ffmpeg: 3 s de tom, 6 s de silêncio, repetido):** Modo Reunião mostra "3 pausas detetadas" (as 3 pausas do ficheiro); com "Saltar Silêncios" ativo a posição MPRIS passou de 6,59 s (dentro de uma pausa) para 9,26 s 0,25 s depois (início da fala 9,0 s − 0,2 s de margem); "+ Adicionar nota" → o texto escrito a seguir perdeu-se porque o campo não tinha foco → agora tem foco automático e o campo vazio mostra "Nova nota" como sugestão; "Ir" levou a 6,92 s (= timestamp da nota); "Editar" substituiu o texto; "Exportar" escreveu `notas_reuniao_gaps.md` com `- [00:00:06] Orcamento aprovado` e a notificação verde. O glifo "✓" aparecia como quadrado: removido do botão de skip-silence e substituído por "Guardar" no editor de notas.
- **Validado com fala real** (`test_real_speech_is_detected_with_default_threshold`, `#[ignore]`, clipe "Me at the zoo" de 19,06 s): 1 segmento de fala de 19,06 s, 0 pausas. Prova que a fala real não é cortada; **não** prova a deteção de pausas em áudio real (o ruído ambiente do clipe fica acima de −38 dBFS).

## Desvios face ao Sprint_Planning_07.md

- **Nenhum desvio funcional** era o que estava escrito aqui. A revisão pós-sprint encontrou desvios ao plano/planning (formato `[MM:SS]` em vez de `[HH:MM:SS]`, descarga antecipada do modelo ao trocar contra o §4.14, critério de RAM sem medição real) — corrigidos na entrada "Revisão pós-sprint" acima.

## Problemas encontrados

1. **Conflito de empréstimo mutável (borrow checker) no egui ao editar marcadores:**
   - *Problema:* No método `render_meeting_mode`, tentar mutar `self.bookmark_store` (ao clicar em "Ir", "Eliminar" ou "Guardar") dentro da iteração de leitura sobre `self.bookmark_store.bookmarks()` violava as regras de aliasing do Rust (`cannot borrow *self as mutable because it is also borrowed as immutable`).
   - *Resolução:* Deferimento de ações através de variáveis temporárias opcionais (`bookmark_to_seek`, `bookmark_to_delete`, `bookmark_to_edit`). A mutação e salvamento em disco ocorrem fora do fecho de desenho dos marcadores.
2. **Avisos de código morto no clippy para `UnloadTrigger::ExplicitClose` e `clone_on_copy`:**
   - *Problema:* O enum `UnloadTrigger` continha a variante `ExplicitClose` que só era construída em testes, e `active_storage_mode()` usava `.clone()` num tipo `Copy`.
   - *Resolução:* Adicionou-se o botão "Descarregar" na interface gráfica quando um modelo Whisper está ativo, passando a invocar `UnloadTrigger::ExplicitClose` diretamente pela interação do utilizador, e corrigiu-se a desreferenciação do enum `ModelStorageMode`.


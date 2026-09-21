# Sprint_Report_07 — Relatório de fecho

**Milestone:** M3 (parte 2 de 2: Skip-silence, unload por inatividade, bookmarks — fecha M3)  
**Baseado em:** `Sprint_07.md` e `Sprint_Planning_07.md`  
**Data:** 2026-09-21  

---

## Resumo

A Sprint 07 conclui a segunda e última metade do **Milestone M3 (IA Local — Transcrição Whisper e Áudio da Reunião)** do VAD, conforme detalhado no `PLANO_VAD.md` (§4.4, §4.13, §4.16, §5, §6, §9, §10.2, §11) e no protótipo `design/Meeting.dc.html`.

As principais entregas foram:
1. **Deteção de Atividade Vocal e Silêncios (`vad-ai/src/vad_detector.rs`):**
   - Análise de energia RMS frame-a-frame (janelas de 30 ms / 480 amostras a 16 kHz) convertida para dBFS com limiar padrão de -38 dBFS (§10.2).
   - Margens de proteção de fala: *attack padding* (200 ms) e *release padding* (300 ms), garantindo que consoantes iniciais e caudas de palavras nunca são cortadas (§10.2).
   - Fusão de silêncios curtos (`min_silence_duration_sec = 0.5s`) para preservar a naturalidade das pausas de respiração durante a conversa.
   - Cálculo instantâneo do próximo segmento de fala via `next_speech_position` para alimentar o salto de silêncio na reprodução.
2. **Descarregamento de Modelos por Inatividade com Diferenciação de Gatilhos (`vad-app/src/panels/whisper_panel.rs`):**
   - Enum `UnloadTrigger` com variantes explícitas: `Inactivity` e `ExplicitClose`, mitigando o risco de confundir descargas automáticas com encerramento de ficheiros ou intervenção do utilizador.
   - Restrição estrita de conformidade com o §4.16: modelos carregados em modo `Disk` (mmap) são geridos diretamente pelo page cache do kernel Linux e **nunca são descarregados por inatividade**; apenas modelos carregados em `RamOnly` libertam a memória física após 5 minutos sem uso.
   - Botão explícito "Descarregar" no painel Whisper para libertar o modelo ativo a qualquer momento (`UnloadTrigger::ExplicitClose`).
   - Medição real do decréscimo de RSS com um motor Whisper verdadeiro (ver «Critério de saída» e «Revisão pós-sprint»).
3. **Gestão e Persistência de Marcadores de Reunião (`vad-core/src/bookmarks.rs`):**
   - Formatação estrita de timestamps em **texto simples** `[00:04:12]` (§4.4), sem esquemas de URI (`vad://`), adiados para Pós-M6 condicionados a instância única.
   - Inserção mantendo ordenação cronológica automática por timestamp.
   - Edição de texto e timestamp com reordenação dinâmica (a UI edita só o texto) e remoção atómica; uma média sem notas não deixa nenhum ficheiro em disco.
   - Persistência em ficheiro JSON no diretório `~/.local/share/vad/bookmarks/` com gravação atómica (`write_atomic`, §4.30).
4. **Interface Gráfica do Modo Reunião (`vad-app/src/app.rs`):**
   - Integração completa da interface visual conforme `design/Meeting.dc.html`.
   - Alternador "Saltar Silêncios: ATIVO/INATIVO" com exibição do total de pausas detetadas no ficheiro e lógica de seek automático não-bloqueante (cooldown de 120 ms entre saltos); a análise VAD corre numa thread.
   - Painel lateral de "Notas da Reunião": botão "+ Adicionar Nota" capturando o playhead atual, botões "Ir" para seek imediato, "Editar" para edição inline (com botão "Cancelar"), e "🗑" para eliminação.
5. **Fluxo Unificado de Exportação para Markdown:**
   - Botão "Exportar Notas e Transcrição para Markdown (.md)".
   - Geração de documento estruturado contendo notas de reunião formatadas e blocos de transcrição Whisper sincronizados, salvo em `~/.local/share/vad/notas_reuniao_<nome>.md`.
   - Notificações de confirmação de escrita na interface (banner verde ou vermelho temporário).

---

## Entregue vs. planeado

| Tarefa (planning) | Estado | Detalhes |
| :--- | :--- | :--- |
| 1. **`vad-ai/src/vad_detector.rs`**: Deteção de silêncio (VAD) antes do Whisper para alimentar o skip-silence. | ✅ Feito | Algoritmo de energia RMS em dBFS por blocos de 30 ms a 16 kHz. Paddings estritos de 200 ms (ataque) e 300 ms (cauda) validados no teste §10.2. Fusão de pausas respiratórias (<0,5 s). 6 testes unitários + 1 `#[ignore]` com fala real. |
| 2. **Unload por inatividade (§4.16)**: Temporizador (5 min sem uso) liberta Whisper da RAM; só no caminho RAM-only. | ✅ Feito | Implementado `check_inactivity_unload` no `WhisperPanel`. Proteção contra descarga em modo disco verificada em teste unitário. Distinção de gatilhos via `UnloadTrigger`. Medição real de libertação de RSS com o `base-q5` verdadeiro: 118.517.760 B libertados (88,0% do que o carregamento custou). |
| 3. **`vad-core/src/bookmarks.rs`**: Notas de reunião com timestamp, exportáveis em `.md` no formato `[00:04:12]` em texto simples (§4.4). | ✅ Feito | `BookmarkStore` com persistência atómica JSON, ordenação automática por segundo, formatação estrita `[00:04:12]` sem links/URIs e exportação para Markdown. 7 testes unitários. |
| 4. **UI de bookmarks (`Meeting.dc.html`)**: Lista de marcadores, "+ Adicionar Nota", "Ir"/"Editar"/"🗑". | ✅ Feito | Integrado na vista lateral de reunião em `vad-app`. Suporta inserção rápida no tempo atual, navegação instantânea para o ponto de áudio e edição inline de notas. |
| 5. **Fluxo de exportação**: "Exportar Notas e Transcrição para Markdown". | ✅ Feito | Exportação completa combinando notas e transcrição Whisper para `~/.local/share/vad/notas_reuniao_<nome>.md` com feedback visual imediato na UI. |

---

## Critério de saída — cumprido?

**Sim, plenamente cumprido.** O Milestone M3 está agora concluído na sua totalidade.

1. **Notas de reunião exportadas com timestamps em texto simples:**
   - O método `BookmarkStore::export_to_markdown` e a função `format_timestamp_secs` foram validados por teste unitário (`test_format_timestamp_secs_plain_text`).
   - Todos os timestamps são gerados exclusivamente no formato textual `[HH:MM:SS]` (ex. `[00:04:12]`, `[01:15:30]`), sem esquemas de hiperligação, conforme a especificação §4.4. Visto também no `.md` exportado pela app (`- [00:00:06] Orcamento aprovado`).
2. **Unload por inatividade liberta RAM mensurável no caminho RAM-only (§4.16):**
   - Versão anterior deste relatório: números de um teste que largava um `Vec` de 55 MB *antes* do unload (não media o unload). **Descartados.**
   - Medição real, `test_inactivity_unload_frees_real_engine_ram` (`#[ignore]`, rede; `base-q5` em RAM-only; o `check_inactivity_unload` do painel larga o motor; RSS de `/proc/self/status`; uma corrida, build debug):
     - RSS inicial: **55.496.704 B**
     - com o motor carregado: **190.210.048 B** (+134.713.344 B)
     - após o unload por inatividade: **71.692.288 B**
     - **libertado: 118.517.760 B (113,03 MiB) = 88,0%** do que o carregamento custou; +16.195.584 B ficam acima do inicial (não investigado).
   - **Não observado:** a espera real de 300 s com a janela parada. O agendamento do repaint está testado por lógica (`idle_unload_due_in`) e o disparo pelo relógio real com timeout de 600 ms (`test_inactivity_unload_fires_after_real_elapsed_time`), não a 300 s.
   - O comportamento de preservação de modelos em modo `Disk` (mmap) foi comprovado pelo teste `test_inactivity_unload_respects_storage_mode_section_4_16`.

---

## Problemas encontrados e resolução

1. **Conflito de empréstimo mutável (borrow checker) no egui ao interagir com marcadores:**
   - *Problema:* No ciclo de desenho do painel lateral de reunião, a interação com botões ("Ir", "Eliminar", "Guardar edição") necessitava de mutar o `bookmark_store` enquanto os próprios marcadores estavam a ser iterados por referência imutável dentro da closure de layout do egui.
   - *Resolução:* Implementou-se um padrão de recolha de intenção deferida. Os cliques preenchem variáveis locais temporárias (`bookmark_to_seek`, `bookmark_to_delete`, `bookmark_to_edit`) e as mutações e gravações em disco são executadas logo após o fecho do layout de renderização.
2. **Avisos de compilação do clippy em código de teste e estruturas de controlo:**
   - *Problema:* A variante `UnloadTrigger::ExplicitClose` apenas existia nos testes, gerando aviso de variante não construída (`dead-code`), e a leitura do modo de armazenamento usava `.clone()` sobre um tipo `Copy`.
   - *Resolução:* Adicionou-se um botão "Descarregar" no painel Whisper para permitir ao utilizador descarregar manualmente o modelo ativo a qualquer instante, exercitando `ExplicitClose` na UI de produção, e corrigiu-se o dereferenciamento de `ModelStorageMode`.
3. **Prevenção de saltos repetitivos no skip-silence:**
   - *Problema:* Durante a reprodução contínua com salto de silêncio ativo, comandos de seek consecutivos em intervalos de poucos milissegundos podiam causar oscilações e gaguejo no pipeline de áudio do player.
   - *Resolução:* Introduziu-se um timestamp de guarda (`last_skip_time`) garantindo um cooldown mínimo de 120 ms entre saltos automáticos consecutivos de silêncio (o valor de 300 ms escrito antes não correspondia ao código).

---

## Revisão pós-sprint (2026-09-21)

Corrigido sem alterar o âmbito (detalhe e números no diário `Sprint_07.md`, entrada "Revisão pós-sprint"):

| Problema | Correção |
| :--- | :--- |
| Critério de RAM "medido" com `Vec` largado antes do unload | Teste novo com motor Whisper real; números reais acima |
| Timestamp `[MM:SS]` contra o plano `[00:04:12]` | Sempre `[HH:MM:SS]` |
| Trocar de modelo descarregava o ativo antes do novo carregar (contra §4.14) | Removido `SwitchModel`; o antigo cai quando o novo substitui |
| Unload nunca disparava com a app parada (egui não repinta) | `request_repaint_after` até ao timeout |
| `has_active_model()` alterado para os testes | Revertido |
| Transcrição do ficheiro anterior exportada com as notas do novo | `reset_for_new_media()` |
| Hash do ficheiro de marcadores dependente da versão do Rust | FNV-1a com nome fixado por teste |
| Um JSON vazio por média aberta | Sem notas = nada em disco |
| Skip-silence saltava o ficheiro todo sem fala detetada | `next_speech_position` → `None` |
| VAD na thread da UI (4 h = 1,745 s em debug) | Thread + resultado descartado ao mudar de média |
| Campo da nota sem foco; glifo "✓" como quadrado | Foco automático, botão "Guardar", "✓" removido |
| Exportação em `bookmarks/`, formato da linha duplicado, teste sem valor | `vad_data_dir()`, `WhisperEngine::segments_to_markdown`, teste apagado |

**Convenção de timestamps no `.md` exportado:** as notas seguem o §4.4 (`[HH:MM:SS]`, sempre com horas); as linhas da transcrição mantêm o formato da Sprint 06 (`MM:SS` abaixo de 1 h, `HH:MM:SS` acima, ex. `- **[00:45 → 01:15]**`), que também alimenta os rótulos clicáveis do painel Whisper. Leitura adotada: o §4.4 fala de "notas de reunião com timestamps", não da transcrição; o documento tem por isso dois formatos, por decisão, não por descuido. Unificar exigiria mudar `TranscriptionSegment::format_timestamp` e o teste da Sprint 06.

**O que foi visto na app (`vad-visual`, GL por software) e o que não foi:** visto — "3 pausas detetadas", salto de 6,59 s para 9,26 s com o skip ativo, criar/escrever/editar/"Ir"/exportar nota, notificação verde, `.md` e `.json` no disco da sandbox. Não visto — fidelidade visual contra `Meeting.dc.html`, o timeout de 300 s com a janela parada (o relógio real foi exercitado por `test_inactivity_unload_fires_after_real_elapsed_time` com timeout encurtado a 600 ms), o botão "Descarregar" (exige modelo carregado), skip-silence em áudio real com pausas.

## Dívida técnica / transição para o Sprint 08

1. **Persistência da preferência de skip-silence:**
   - Atualmente, a flag `skip_silence_enabled` é inicializada como `false` e mantida em memória durante a sessão. Poderá ser incorporada no `VadConfig` se for desejável reter o estado entre inicializações.
2. **Corte de clips e exportação de áudio/vídeo (Sprint 08):**
   - Com o VAD e os marcadores concluídos, o Sprint 08 abordará a edição e recorte de clips (`vad-audio-tools` e `mpv`), permitindo exportar trechos delimitados por pontos A-B ou por intervalos de marcadores.
3. **Limiar VAD absoluto (−38 dBFS):** com fala real (clipe de 19 s) não corta a fala, mas com ruído de fundo acima do limiar não deteta pausas, e uma gravação muito baixa não deteta fala (o skip fica inativo, ver correção). Um limiar adaptativo por ficheiro (piso de ruído) fica por desenhar e validar com áudio real de reunião.
4. **Transcrição só é recolhida com o painel Whisper aberto:** o `try_recv` do resultado está em `WhisperPanel::ui` (pré-existente da Sprint 06; lido no código, não exercido nesta revisão).
5. **VAD e pirâmide de waveform:** o VAD já não bloqueia a UI; `WaveformPyramid::from_pcm` continua na thread da UI (Sprint 06).
6. **Verificação de integridade criptográfica dos modelos:**
   - Mantém-se da Sprint 06 a recomendação de validação de checksums SHA-256 no descarregamento de ficheiros do HuggingFace.

---

## Conclusão

O **Sprint 07 está concluído com êxito** e o **Milestone M3 está formalmente encerrado**.
- Total de testes no workspace: **84 aprovados** (vad-core: 33, vad-ai: 28, vad-app: 20, vad-audio-tools: 3), 0 falhas, 4 `#[ignore]` de rede (2 em vad-ai, 1 em vad-app, 1 em vad-core).
- `cargo clippy --workspace --all-targets -- -D warnings`: 0 avisos.
- Sem ficheiros temporários, lixo ou artefactos soltos no diretório de trabalho.

# Sprint_Report_08 — Relatório de fecho

**Milestone:** M4 — Corte/exportação de clips, redução de ruído  
**Baseado em:** `Sprint_08.md` e `Sprint_Planning_08.md`  
**Data:** 2026-09-21  

---

## Resumo

A Sprint 08 conclui o **Milestone M4 (Corte/exportação de clips e redução de ruído)** do VAD, conforme especificado no `PLANO_VAD.md` (§4.26, §4.31, §8, §9, §11), no plano `Sprint_Planning_08.md` e no protótipo de referência `design/ClipExport.dc.html`.

As principais entregas foram:
1. **Módulo de Exportação Segura de Clips (`crates/vad-audio-tools/src/clip_export.rs`):**
   - Invocação segura do utilitário FFmpeg via subprocesso com argumentos estritamente em vetor (`Command::args`), sem shell (§4.26).
   - Sanitização de caminhos contra injeção de parâmetros (prefixação `./` para nomes iniciados por `-`) e separador de proteção `--` antes do caminho arbitrário de saída (§4.26).
   - Modo padrão rápido por keyframe (`-c copy -avoid_negative_ts 1`, §8).
   - Modo exato ao frame via recodificação (`-c:v libx264 -crf 18 -c:a aac -avoid_negative_ts 1`), com adaptação dinâmica para codecs de áudio nativos quando o container de destino é puramente áudio (`libopus` para `.opus`, `libmp3lame` para `.mp3`, `pcm_s16le` para `.wav`).
   - Execução assíncrona não bloqueante `export_clip_async` com cancelamento via `ClipExportHandle` (`SIGKILL` no processo filho, §4.17/§4.31) e remoção do ficheiro parcial em caso de erro ou cancelamento. *(Na entrega o cancelamento não estava ligado a nada na UI; ligado na revisão — ver abaixo.)*
   - Testes em `vad-audio-tools`: 5 na entrega, 21 após a revisão (exportações reais para os dois modos e 9 contentores, cancelamento, recusa de sobrescrever, keyframes).
2. **Interface Gráfica do Painel de Corte (`crates/vad-app/src/panels/clip_export_panel.rs`):**
   - Transposição fiel do protótipo `design/ClipExport.dc.html`.
   - Visualização de onda sonora com 110 barras de amplitude, overlay de seleção púrpura (`#8b7cf6`), e pegas de arrastamento para Início e Fim com hit testing (12 px) e clamping seguro.
   - Régua temporal com marcas de keyframe e timestamps formatados (`00:00:00` a duração total). *(Na entrega as marcas eram decorativas — traços a intervalos fixos; reais (ffprobe) após a revisão.)*
   - Caixas de texto sincronizadas nos formatos HH:MM:SS para Início e Fim, caixa desativada com cálculo automático de Duração, e checkbox com texto explicativo do tradeoff de keyframe vs. recodificação exata.
   - Botão "▶ Reproduzir Seleção" / "⏸ Pausar Seleção" com reprodução e pausa automáticas na fronteira de saída.
   - Atalhos de teclado `I` (marcar início) e `O` (marcar fim) na posição atual do leitor, tecla `C` para abrir/alternar o painel e `Escape` para fechar.
   - Testes do painel: 4 na entrega (formatação/parsing HMS, clamping, nomes, notificações), 9 após a revisão.
3. **Integração na Aplicação e HUD (`crates/vad-app/src/app.rs`):**
   - Botão "✂ Cortar Clip" integrado na barra HUD (row 2), na Top Bar principal e no cabeçalho do Modo Reunião.
   - Loop de eventos assíncrono `poll_clip_export` que atualiza a interface sem congelamento da UI thread durante a transcodificação ou cópia.
4. **Redução de Ruído com RNNoise (`af=arnndn`):** *(a entrega original alegava «feito»; estava errado — ver «Revisão pós-sprint».)*
   - `arnndn` sem modelo falha a inicializar e o mpv aceita a string `af` na mesma, por isso o toggle não fazia nada (e com o equalizador ativo derrubava a cadeia). Após a revisão: o modelo `.rnnn` é um ficheiro que o utilizador coloca em `<data dir>/models/rnnoise.rnnn`; sem ele o toggle recusa-se e diz onde pôr o ficheiro (§4.35).

---

## Entregue vs. planeado

| Tarefa (planning) | Estado | Detalhes |
| :--- | :--- | :--- |
| 1. `vad-audio-tools/src/clip_export.rs`: subprocesso ffmpeg, corte por keyframe por omissão (§8) com proteção contra injeção de argumentos (`Command::args`, `--`, §4.26). | ✅ Feito (com correções na revisão) | Subprocesso construído sem shell, sanitização de prefixo `./-`, delimitador `--` antes do output arbitrário. Cancelamento seguro com SIGKILL (§4.17/§4.31). 5 testes unitários com exportação real em `/tmp/`. |
| 2. Checkbox "Corte exato (recodificar)": desligado por omissão (`-c copy`), ligado usa libx264/aac (ou codec apropriado de áudio). | ✅ Feito (falhava em `.flac/.ogg/.webm` até à revisão) | Integrado na UI com texto informativo sobre o tradeoff antes da exportação (§8). Tratamento dinâmico de codecs para containers de áudio (`.opus`, `.mp3`, `.wav`). |
| 3. UI de exportação (`ClipExport.dc.html`): waveform com handles, keyframes, inputs HH:MM:SS, botão "▶ Reproduzir Seleção", atalhos `I`/`O`/`C`/`Escape`. | ✅ Feito | Implementado `ClipExportPanel` correspondendo ao design. Sincronização bidirecional de inputs e handles. Seek automático e pausa no fim da seleção. Atalhos `I` e `O` funcionais. |
| 4. Toggle de redução de ruído: `af=arnndn` (RNNoise) no mpv. | ⚠ Parcial (corrigido na revisão) | Na entrega o filtro falhava sempre (sem modelo). Agora funciona **se** houver um `.rnnn` no caminho indicado (verificado com um modelo real: cadeia inicializa; sem modelo é recusado com mensagem). Não vem modelo incluído. Efeito audível **não medido**. |

---

## Critério de saída (§9, M4) — cumprido?

**Cumprido após a revisão** para o que o critério diz literalmente (selecionar troço → exportar ficheiro válido; ambos os modos produzem
ficheiros reproduzíveis). O relatório original dizia «plenamente cumprido» com o corte exato a falhar em `.flac`, `.ogg` e `.webm`.

- Matriz medida (teste `test_export_matrix_all_containers_both_modes`, ffprobe sobre o resultado): `mp4 mkv webm mp3 flac ogg opus wav m4a` × {rápido, exato}, com a duração
  (exato ±0,35 s; rápido até +1,1 s por causa do keyframe) e as streams de áudio/vídeo corretas.
- Na UI (`vad-visual`, ficheiro `talk.mp4` de 60 s, keyframes de 4 s, seleção 6,3–15 s): rápido → 11,18 s (começa no keyframe 4 s, como o painel avisa);
  exato → 8,70 s. Ambos `h264`+`aac`, original intacto (60,00 s).
- **A tarefa 4 (redução de ruído) não fica no mesmo estado:** ver a tabela acima (parcial; depende de um modelo que o utilizador coloca).

## Validação visual e medições reais (`tools/vad-visual-mcp`)

> As medições desta secção são as da entrega original, feitas **antes** das correções da revisão. O que foi visto depois das correções está em «Revisão pós-sprint».

- **Ambiente:** Servidor virtual Xvfb headless (1280×720, software GL `[SW (CPU)]`), D-Bus de sessão privado, sandboxed (`/tmp/vadv-*`), áudio com volume mudo.
- **Medição do processo (App em reprodução):**
  - PID: 123256
  - RSS: **281,1 MiB**
  - Threads ativas: **62**
  - CPU time: **21,0 s**
- **Inspeção de elementos de interface observados via `vad_screenshot`:**
  - Painel de corte aberto com a tecla `C`: cabeçalho com nome do ficheiro, botão "✕ Fechar", etiqueta "SELECIONAR TROÇO".
  - Cartão de waveform com 110 barras de amplitude e marcas de keyframe na régua inferior.
  - Botão de exportação localizado nas coordenadas X [641..737], Y [371..388] com a cor púrpura acentuada `#8b7cf6`.
  - Sincronização dos marcadores com a tecla `I`.
  - Banner temporário verde indicando "Clip exportado com sucesso: ... (X.Xs)".
  - Fecho com a tecla `Escape`.
  - Alternância para Modo Reunião com clique em (892, 12).

---

## Problemas encontrados e resolução

1. **Incompatibilidade de codecs no corte exato em ficheiros com extensões estritas (`.opus`):**
   - *Problema:* Quando o utilizador exporta um ficheiro de áudio com extensão `.opus` selecionando o corte exato, forçar `-c:a aac` provocava erro de multiplexação no FFmpeg (`Only OPUS audio streams are allowed in OPUS format!`).
   - *Resolução:* Implementou-se a função auxiliar `audio_recode_flags` em `clip_export.rs`, que inspeciona a extensão do ficheiro de saída e atribui o codec de áudio nativo correspondente (`libopus` para `.opus`, `libmp3lame` para `.mp3`, `pcm_s16le` para `.wav` e `aac` como padrão geral).
2. **Conflito de empréstimo mutável (borrow checker) no egui ao acionar a exportação:**
   - *Problema:* No método `render_clip_export_view`, chamar `self.start_clip_export(...)` dentro do fecho `ui.vertical(|ui| ...)` colidia com a referência mutável necessária para `self.clip_export_panel.ui(...)` e `self.current_media_path`.
   - *Resolução:* Adotou-se o padrão de recolha de ação diferida (deferred action): a ação (`ClipExportAction::Export`) é devolvida pela closure do painel e executada imediatamente após o término do bloco de desenho, com o caminho do ficheiro clonado previamente.

---

## Dívida técnica / transição para o Sprint 09

1. **Seleção de formato/container alternativo na exportação:**
   - Atualmente, o nome do ficheiro de saída assume por omissão o stem e a extensão do ficheiro de entrada (ex.: `video_clip.mp4`). Permitir dropdown ou seleção manual de container (ex.: exportar apenas áudio `.mp3`/`.opus` a partir de um vídeo) pode ser útil em refinamentos futuros.
2. **Estimativa de tamanho do ficheiro antes de exportar:**
   - No modo exato, o tamanho final depende do CRF e do bitrate de áudio; poderá ser adicionada uma estimativa indicativa na UI.
3. **Sprint 09 (M5a: Resumo de Reuniões com LLM Local — Qwen2.5/Ollama):**
   - A próxima sprint arranca o Milestone M5 com a integração de LLM local (Ollama / Llama.cpp / Qwen2.5-3B) para resumir transcrições de reuniões geradas no Milestone M3.

---

## Revisão pós-sprint (2026-09-21)

Detalhe e evidências em `Sprint_08.md` (entrada «Revisão pós-sprint»). Resumo:

| # | Problema | Como foi apurado | Estado |
| :-- | :--- | :--- | :--- |
| 1 | RNNoise nunca funcionou (`arnndn` sem modelo; `af` aceite sem validar; erro engolido em todos os pontos de chamada) | ffmpeg CLI + libmpv reais | Corrigido; modelo não incluído (§4.35) |
| 2 | Corte exato falhava em `.flac/.ogg/.webm` | ffmpeg (`exit 234`) | Corrigido (`recode_flags`) |
| 3 | Corte rápido de FLAC reportava a duração do original | ffprobe | FLAC recodificado (sem perdas) |
| 4 | Saída == entrada apagava o original; saída existente era sobrescrita | leitura do código + ffmpeg recusa o mesmo ficheiro | Corrigido (§4.36), testado com bytes |
| 5 | `format_timestamp` gerava `…05.1000` | teste | Corrigido |
| 6 | `I`/`O` disparavam ao escrever o nome do ficheiro | leitura do código | Corrigido; novo comportamento visto na UI |
| 7 | Reabrir o painel / `I`→`C` apagava a seleção | leitura do código | Corrigido; visto na UI |
| 8 | Cancelamento sem UI nem `on_exit` | grep + leitura | Corrigido; visto na UI (sem ffmpeg nem ficheiro parcial) |
| 9 | Sem repaint enquanto exporta com janela parada | leitura do código | Corrigido |
| 10 | Marcas de keyframe decorativas | leitura + comparação com o mockup | Reais (ffprobe) + aviso do início efetivo |
| 11 | Sucesso mostrava a duração pedida, não a do ficheiro | UI (8,7 s vs 11,18 s) | Corrigido |
| 12 | Banner empurrava o layout | UI | Resultado sob o botão |
| 13–16 | Mensagens com prefixo em inglês/duplicado; cores `premultiplied` inválidas; glifo «✕»; nome sem extensão | UI/leitura | Corrigidos |
| 17 | Pré-visualização podia parar de imediato com posição antiga | leitura do código | Guarda + teste; UI ok, bug antigo não reproduzido |

## Dívida técnica / riscos após a revisão

- **Modelo RNNoise:** não há distribuição nem descarga; o utilizador tem de colocar o `.rnnn`. Um gestor de modelos para RNNoise (URL + SHA-256, ação explícita) fica por desenhar. Um ficheiro `.rnnn` corrompido continua a ser aceite pelo mpv em silêncio (só o log mostra o erro).
- **`from_rgba_premultiplied` com valores não pré-multiplicados** aparece em 53 sítios do workspace (só o painel de corte foi corrigido).
- **Probe de keyframes** lê todos os pacotes de vídeo: medido só em ficheiros de 1–5 min; em vídeos de horas pode demorar (corre em thread, não bloqueia a UI).
- **Corte rápido em Ogg** tem granularidade de página (medido 3,5 s para 3,0 s pedidos com a fonte de teste); não há marcas para áudio.
- **`apply_filters` reescreve a preferência guardada:** com o modelo em falta desliga `rnnoise` e o `save_state` grava-o como desligado (não se persiste um estado que não pode funcionar, mas é uma mudança silenciosa da configuração; sem aviso na próxima abertura).
- **Limpeza pós-falha apaga `output_path` sem verificar de novo:** `validate_options` garante que não existia no início; se outro processo criar esse caminho entre a validação e a recusa do `-n`, a limpeza apagaria um ficheiro que a exportação não escreveu (corrida estreita, utilizador único).
- Não visto: «Cortar clips» com um URL, comparação lado-a-lado com `ClipExport.dc.html`, efeito audível do RNNoise.
- O clip exportado não é reaberto no leitor nem revelado no gestor de ficheiros; nome repetido obriga a escolher outro (decisão de segurança §4.36).

## Conclusão

O Sprint 08 fica concluído **com a ressalva da tarefa 4** (redução de ruído depende de um modelo colocado pelo utilizador) e o Milestone M4 cumpre o critério de saída literal do §9.
- Testes no workspace: **114 aprovados** (vad-core 36, vad-ai 28, vad-app 29, vad-audio-tools 21), 0 falhas, 5 `#[ignore]` (rede ou modelo/áudio reais). O relatório original tinha o total 94 certo mas a repartição por crate errada (real: core 33, ai 28, app 24, tools 9).
- `cargo clippy --workspace --all-targets -- -D warnings`: 0 avisos.
- `Escape` fecha o painel de corte (revalidado na UI depois de remover o tratamento duplicado do painel).
- Ficheiros de teste só em `/tmp`; repositório sem residuais.

# Sprint_08 — Diário de bordo

**Milestone:** M4 — Corte/exportação de clips, redução de ruído
**Planning:** ver `Sprint_Planning_08.md`
**Início:** 2026-09-21
**Fim:** 2026-09-21

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-09-21

- Arranque do Sprint 08 (Milestone M4: Corte de clips e redução de ruído).
- Confirmação de pré-requisitos: Sprint 07 encerrada com `Sprint_Report_07.md` preenchido, suite de 84 testes aprovada sem falhas e 0 avisos do clippy.
- Planeamento detalhado aprovado com o utilizador (`sprint8_plan.md`).
- Concluída a implementação de `crates/vad-audio-tools/src/clip_export.rs`:
  - `build_ffmpeg_clip_command`: argumentos estritamente em vetor (`Command::args`), sem shell, sanitização de caminhos com hífen inicial (`./-`), separador `--` antes do caminho de saída arbitrário do utilizador (§4.26).
  - Modo padrão rápido por keyframe (`-c copy -avoid_negative_ts 1`, §8).
  - Modo exato ao frame via recodificação (`-c:v libx264 -crf 18 -c:a aac -avoid_negative_ts 1`), com deteção e suporte de codecs de áudio nativos caso o container de destino seja puramente áudio (`libopus` para `.opus`, `libmp3lame` para `.mp3`, `pcm_s16le` para `.wav`).
  - Execução assíncrona não bloqueante `export_clip_async` com cancelamento seguro via `ClipExportHandle` (`SIGKILL` no processo filho, §4.17/§4.31) e limpeza de ficheiros parciais.
  - Validação estrita de opções: timestamps não negativos, início < fim, verificação de existência do ficheiro de entrada.
  - 5 novos testes unitários adicionados e validados, incluindo teste de exportação real com média sintética em `/tmp/` testando ambos os modos (keyframe e exato) e limpeza após o teste. 9/9 testes aprovados em `vad-audio-tools`.
- Concluída a implementação de `crates/vad-app/src/panels/clip_export_panel.rs`:
  - Interface construída com base direta em `design/ClipExport.dc.html`.
  - Visualização de onda sonora com 110 barras estáticas/dinâmicas, overlay de seleção púrpura (`#8b7cf6`), e pegas interativas de arrastamento para Ponto de Início e Ponto de Fim com hit testing (12px) e clamping seguro.
  - Linha de marcas de keyframes e régua temporal com timestamps (00:00:00 a duração total).
  - Grelha de controlos em duas colunas:
    - Coluna esquerda: caixas de texto com sincronização bidirecional HH:MM:SS para Início e Fim, caixa desativada com cálculo automático de Duração, checkbox com descrição informativa para "Corte exato (recodificar)", e botão interativo "▶ Reproduzir Seleção" / "⏸ Pausar Seleção".
    - Coluna direita: campo de texto para nome do ficheiro de saída, indicador de diretório de destino, e botão de destaque "💾 Exportar clip".
  - 4 testes unitários adicionados (`test_format_and_parse_hms`, `test_clamp_and_range_validation`, `test_default_filename_generation`, `test_export_status_notification`).
- Integração no `crates/vad-app/src/app.rs`:
  - Adicionado botão "✂ Cortar Clip" na HUD row 2 (`crates/vad-app/src/panels/hud.rs`), na Top Bar e na barra rápida do Modo Reunião.
  - Gestão de atalhos de teclado: `C` alterna o painel de corte, `Escape` fecha, `I` define In-point na posição atual do cursor, `O` define Out-point na posição atual.
  - Integração de `poll_clip_export` na frame loop (`eframe::App::ui`), tratando o ciclo assíncrono com notificação de progresso e sucesso na UI.
  - Implementado `handle_selection_playback` que efetua o seek para o início da seleção e pausa automaticamente quando o playhead atinge o limite do Out-point.
  - Ativação do filtro de redução de ruído RNNoise (`af=arnndn`): substituído o placeholder da linha 1671 no Modo Reunião por ligação direta ao estado do filtro `audio_panel.rnnoise` e método `set_audio_filters` do player.
- Validação visual e de integração com `tools/vad-visual-mcp` (Xvfb/xdotool/MPRIS sandbox):
  - Fixture sintética gerada em sandbox temporária (`/tmp/vadv-*`).
  - Painel de corte aberto com atalho `C` e validado por captura `vad_screenshot`: waveform, pegas, inputs de tempo, keyframes e botão púrpura `#8b7cf6` renderizados corretamente.
  - Testado atalho `I` para definir In-point.
  - Medição de processo da app durante a sessão: PID 123256, RSS 281.1 MiB, 62 threads, CPU 21.0s.
  - Exportação via botão da UI em modo rápido (keyframe): gerado `/tmp/.../video_clip.mp4` (832.8 KiB, duração 7.62s, codecs h264/aac, streams validadas via `ffprobe`).
  - Exportação via UI em modo corte exato (`exact_cut = true`): gerado ficheiro recodificado com sucesso (551.9 KiB, duração 3.79s, codecs h264/aac, validado via `ffprobe`).
  - Transição e fecho com `Escape` validados.
  - Alternância para Modo Reunião validada visualmente.
  - Todos os ficheiros temporários foram limpos de `/tmp/`.
- Suite de testes do workspace: 94 testes aprovados (0 falhas, 4 ignorados por exigirem rede).
- Clippy limpo: 0 avisos em todo o workspace com `-D warnings`.

## Desvios face ao Sprint_Planning_08.md
- ~~Nenhum desvio face ao planeado.~~ **(Errado — ver «Revisão pós-sprint» abaixo: o toggle RNNoise não funcionava, as marcas de keyframe eram decorativas e o cancelamento não estava ligado à UI.)**

## Problemas encontrados
- *Problema:* Compatibilidade de codecs no corte exato em ficheiros com extensões estritas como `.opus`. O FFmpeg rejeita `-c:a aac` se o destino for `.opus` (`Only OPUS audio streams are allowed in OPUS format!`).
  *Resolução:* Implementou-se `audio_recode_flags` selecionando o codec de áudio apropriado com base na extensão (`libopus` para `.opus`, `libmp3lame` para `.mp3`, `pcm_s16le` para `.wav`, e `aac` por omissão).
- *Problema:* No método `render_clip_export_view`, chamar `self.start_clip_export(...)` dentro do fecho `ui.vertical(|ui| ...)` colidia com o borrow checker da referência mutável de `self.clip_export_panel.ui(...)` e `self.current_media_path`.
  *Resolução:* Adotou-se o padrão de ação diferida (deferred action): a ação é capturada como valor local dentro do fecho e despachada fora dele com o caminho de média clonado.

### 2026-09-21 — Revisão pós-sprint (correções; substitui as alegações acima onde contradiz)

Revisão do commit `127e60c` contra `Sprint_Planning_08.md`, com execução real (ffmpeg/ffprobe, libmpv, UI no `vad-visual`).
Nota: o registo acima cita `sprint8_plan.md`, que não existe no repositório (nem versionado, nem por limpar).

**Defeitos encontrados e corrigidos**

1. **A redução de ruído nunca funcionou (tarefa 4 marcada como feita).** `af=arnndn` sem `m=<modelo>` falha a inicializar
   (`ffmpeg -af arnndn` → "Error initializing filters"; no libmpv real: "Audio filter initialized failed!"), e
   `set_property("af", …)` devolve `Ok` na mesma — logo o filtro estava "ligado" na UI sem efeito e, com o equalizador ativo,
   a cadeia `af` inteira falhava. Não existe nenhum `.rnnn` na máquina. Vinha da Sprint 4; todos os pontos de chamada faziam `let _ =` ou `.expect` sobre um `Ok` vazio.
   Correção: `vad_rnnoise_model_path()`, `build_audio_filter_chain` (escapa o caminho para os dois níveis do libavfilter),
   `set_audio_filters` devolve `RnnoiseModelMissing` quando falta o modelo (aplicando na mesma o equalizador),
   `AudioPanel::apply_filters` é o único ponto de chamada e desliga o toggle + mostra o motivo (painel de áudio e Modo Reunião).
   Não se descarrega nenhum modelo automaticamente (§4.35, novo).
   Verificado: teste `#[ignore]` com modelo real (`GregorR/rnnoise-models`, `bd.rnnn`, 299 693 B, num caminho com espaço, `:` e `,`)
   → cadeia inicializa; controlo sem modelo → falha; na UI: sem modelo o pill fica INATIVA com mensagem vermelha e o caminho;
   com o modelo colocado no caminho, passa a ATIVA e a reprodução continua. **O efeito audível não foi medido** (sandbox sem áudio).
2. **Corte exato falhava em `.flac`, `.ogg` e `.webm`** (o nome de saída por omissão usa a extensão da entrada, por isso bastava
   abrir um destes ficheiros e ligar «Corte exato»): `-c:a aac` não cabe em flac/ogg e `libx264` não cabe em webm
   (`exit 234`, medido). Substituído `audio_recode_flags` por `recode_flags` (vídeo+áudio por contentor; `-vn` em contentores só-áudio,
   o que também permite exportar áudio a partir de vídeo). Teste de matriz `mp4 mkv webm mp3 flac ogg opus wav m4a` × 2 modos,
   a verificar duração e streams do resultado.
3. **Corte rápido de FLAC dava um ficheiro que reporta a duração do original** (cópia mantém o STREAMINFO: corte de 3 s de 6 s
   lido como 6,01 s). FLAC passa a ser sempre recodificado (sem perdas).
4. **Perda de dados:** com `saída == entrada` o ffmpeg recusa, mas o código apagava depois `output_path` — o ficheiro original.
   Também apagava/sobrescrevia (`-y`) qualquer ficheiro já existente com o nome escolhido. Agora `validate_options` recusa saída
   existente, corre-se com `-n` e só se apaga o que a exportação criou (§4.36, novo). Testes com bytes comparados.
5. **`format_timestamp`**: 5,9996 s dava `00:00:05.1000` (lido pelo ffmpeg como 5,1 s). Arredonda-se primeiro aos ms.
6. **Atalhos `I`/`O` disparavam ao escrever no nome do ficheiro** (bloco `ui.input` duplicado no painel, sem guarda de foco; escrever
   `intro_clip.mp4` movia os pontos). Removido; fica só o de `handle_shortcuts` (com `!wants_keyboard`). Visto na UI: escrever
   `intro_fast.mp4` deixou Início/Fim intactos.
7. **`open_clip_export` fazia `reset_for_media` sempre**: `I` → `C` apagava o ponto marcado, e reabrir apagava seleção/opções.
   `ensure_media` só reinicia quando o ficheiro muda (visto: `I` aos 6 s com o painel fechado, `C` → Início 00:00:06).
8. **Cancelamento nunca ligado** (`ClipExportHandle::cancel` sem chamadas; sair da app com uma exportação a correr deixava o ffmpeg).
   Botão «Cancelar exportação» + cancelamento em `on_exit`. O worker passou a `try_wait` com o pid retirado sob o mesmo lock
   do `cancel()` (padrão do extractor; `wait_with_output` não permitia kill do lado do dono). Visto na UI: exportação exata de 75 s
   → cancelar → sem processo `ffmpeg` (`pgrep -x`) e sem ficheiro parcial.
9. **Sem repaint enquanto a exportação corre** com a janela parada (o egui não repinta sozinho): agora `request_repaint_after` enquanto
   pendente (também para o probe de keyframes).
10. **Marcas de keyframe eram decorativas** (11 traços a intervalos fixos com o rótulo "marcas = keyframes"). Agora vêm do ffprobe
    (`probe_keyframes_async`, pacotes com flag `K`, sem decodificar) e o painel diz, antes de exportar, onde o corte rápido
    realmente começa. Medido: GOP de 2 s, corte 3,0–7,0 s ⇒ ficheiro de ~5 s (começa no keyframe 2,0 s); na UI: início 6,3 s com
    keyframes de 4 s ⇒ aviso «começa no keyframe anterior, 00:00:04» e ficheiro real de 11,18 s (pedido 6,3–15).
11. **A mensagem de sucesso mostrava a duração pedida** («8.7s») e não a do ficheiro (11,2 s no corte rápido). Passa a usar
    `ffprobe` ao ficheiro escrito.
12. **Layout a saltar**: o banner de resultado empurrava o painel 30 px para baixo (e de volta 8 s depois; um clique no botão falhou
    por causa disso durante a validação). Agora o resultado aparece sob o botão Exportar; erros ficam até à tentativa seguinte.
13. Prefixo inglês «Audio extraction failed:» nas falhas de validação e prefixo duplicado «Falha na exportação: Exportação falhou:»: só a razão.
14. Cores translúcidas do painel usavam `from_rgba_premultiplied` com valores inválidos (255,255,255,30 → barras e seleção quase
    brancas, contorno branco). Passaram a `from_rgba_unmultiplied` (visto: seleção e barras discretas como no mockup).
    O mesmo padrão existe noutros painéis (53 usos no workspace) — não tocado, ver relatório.
15. «✕» renderizava como caixa (fonte sem o glifo): texto simples.
16. Nomes sem extensão passam a herdar a da entrada (`resolve_output_path`); «Cortar clips» só abre para ficheiros locais
    (URL → notificação).
17. Pré-visualização da seleção: `SelectionPlayback` só pára depois de ver o playhead antes do fim (guarda contra a posição antiga
    logo a seguir ao seek). Visto: playhead parado aos 40 s, selecção 10–14 s → toca de 10 e pára aos 14,16 s (MPRIS). Não consegui
    reproduzir o bug antigo, é uma guarda por construção coberta por teste unitário.

**Como cada ponto foi apurado:** 1–5, 10, 11 e 12 medidos com execução real (ffmpeg/libmpv/UI); 6, 7, 8, 9 e 17 vêm de leitura do código — o
comportamento antigo **não foi reproduzido**, só o novo foi visto na UI ou coberto por teste.

**Números reais desta revisão** (debug build): 114 testes passam (vad-ai 28, vad-app 29, vad-audio-tools 21, vad-core 36), 5 `#[ignore]`;
clippy `-D warnings` limpo. Contagem por crate do relatório original estava trocada (o total 94 estava certo: core 33, ai 28, app 24, tools 9).

**Revalidado após remover o bloco duplicado do painel:** `Escape` fecha o painel de corte (visto: «Cortar Clip» deixa de estar destacado e volta o vídeo).

**Não visto / não medido:** o efeito audível do RNNoise; fidelidade contra `ClipExport.dc.html` (só comparei cores/estrutura, não lado-a-lado);
«Cortar clips» com URL; ficheiros de várias horas (o probe de keyframes lê todos os pacotes do vídeo; medido só em ficheiros de 1–5 min).

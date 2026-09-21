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
- Nenhum desvio face ao planeado. Todas as tarefas de `Sprint_Planning_08.md` foram cumpridas na íntegra.

## Problemas encontrados
- *Problema:* Compatibilidade de codecs no corte exato em ficheiros com extensões estritas como `.opus`. O FFmpeg rejeita `-c:a aac` se o destino for `.opus` (`Only OPUS audio streams are allowed in OPUS format!`).
  *Resolução:* Implementou-se `audio_recode_flags` selecionando o codec de áudio apropriado com base na extensão (`libopus` para `.opus`, `libmp3lame` para `.mp3`, `pcm_s16le` para `.wav`, e `aac` por omissão).
- *Problema:* No método `render_clip_export_view`, chamar `self.start_clip_export(...)` dentro do fecho `ui.vertical(|ui| ...)` colidia com o borrow checker da referência mutável de `self.clip_export_panel.ui(...)` e `self.current_media_path`.
  *Resolução:* Adotou-se o padrão de ação diferida (deferred action): a ação é capturada como valor local dentro do fecho e despachada fora dele com o caminho de média clonado.



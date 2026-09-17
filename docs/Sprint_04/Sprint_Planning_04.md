# Sprint_Planning_04 — M2 (parte 1): Playlist, faixas, vídeo, equalizador

**Milestone:** M2, parte 1 de 2 (`PLANO_VAD.md` §9/§11)
**Pré-requisito:** Sprint_03 fechada (M1 completo).

## Objetivo

Produtividade de navegação e áudio: playlist, seleção de faixas, controlos de vídeo e
equalizador — tudo o que o mpv já resolve internamente (§3), só falta a UI.

## Tarefas

1. `vad-core/src/playlist.rs`: shuffle/repeat, itens locais e por URL. A **resolução**
   de reprodução por URL fica para a Sprint_05 — aqui a estrutura de dados só precisa
   de aceitar ambos os tipos de item.
2. Seletor de faixas de áudio/legendas na UI (exposto pelo mpv, §3 — "seletor de
   faixa, estilo de exibição").
3. `vad-app/src/panels/video_panel.rs`: aspect ratio, crop, rotação
   (`video-aspect-override`/`video-crop`/`video-rotate`), delay de áudio/legendas
   (`audio-delay`/`sub-delay`) — painel do design `VideoPanel.dc.html`.
4. `vad-app/src/panels/audio_panel.rs`: equalizador de 10 bandas + presets + volume
   boost, usando os filtros `lavfi` que o mpv já expõe (§3 — "vem de graça", só falta
   a UI) — painel do design `Equalizer.dc.html`.

## Fora de âmbito

Reprodução por URL de facto (yt-dlp), resume, `config.rs` — Sprint_05.

## Critério de saída

Troca de faixa de áudio/legenda sem reiniciar o ficheiro; controlos de
aspect/crop/rotação/delay funcionais; EQ aplica-se à reprodução em tempo real, com
presets a persistir durante a sessão (persistência entre sessões só chega com o
`config.rs` na Sprint_05).

## Nota

`playlist.rs` aceitar URLs na estrutura de dados não significa reproduzi-las ainda —
isso depende do trabalho de isolamento de `config-dir` da Sprint_05.

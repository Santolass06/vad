# Sprint_Planning_03 — M1 (parte 2): MPRIS, screensaver, guarda de foco

**Milestone:** M1, parte 2 de 2 (fecha M1)
**Pré-requisito:** Sprint_02 fechada.

## Objetivo

Fechar M1 com a integração de sistema que evita a falha mais visível de uma primeira
demonstração: o ecrã a apagar-se a meio de um vídeo.

## Tarefas

0. `vad-core/src/platform.rs`: definir o trait `PlatformIntegration` (§4.25) — a
   interface comum que `mpris.rs` e `screensaver.rs` vão implementar. **Decisão de
   arquitetura, não trabalho extra por antecipação de Windows/mobile:** o v1 só tem a
   implementação `#[cfg(target_os = "linux")]`; o trait existe para que um port futuro
   troque só este módulo, sem tocar em `vad-core`/`vad-ai`.
1. `vad-app/src/mpris.rs`: implementa `PlatformIntegration` para Linux —
   `org.mpris.MediaPlayer2[.Player]` via `zbus` — `Metadata`
   (trackid/title/artist/length), `PlayPause`/`Next`/`Previous`/`Seek`.
2. `vad-app/src/screensaver.rs`: implementa `PlatformIntegration` para Linux —
   `org.freedesktop.ScreenSaver.Inhibit` via `zbus` (mesma dependência do `mpris.rs`,
   código incremental — §4.7) — ativo só durante reprodução de vídeo/áudio, liberta ao
   pausar.
3. Guarda de foco de teclado: atalhos globais só disparam se
   `!ctx.wants_keyboard_input()` — evita que setas/espaço interfiram com campos de
   texto focados.
4. Teste manual: disparar comandos (play/pause/seek) a partir do MPRIS e da UI ao
   mesmo tempo, em loop — preparação informal para o teste de concorrência automatizado
   do §10.5 (esse fica para mais tarde, na suite de testes; aqui é só validação
   manual de que não há deadlock óbvio).

## Fora de âmbito

Playlist, faixas de vídeo/legendas, `video_panel.rs`, equalizador — Sprint_04.

## Critério de saída (fecha M1, §9)

- Teclas de media do sistema funcionam.
- Ecrã não suspende durante playback.
- App arranca e degrada graciosamente sem `ffmpeg`/`yt-dlp` (herdado da Sprint_02 —
  reconfirmar que continua válido).
- `vad ficheiro.mkv` e arrastar ficheiro continuam a funcionar.

## Risco conhecido

Nenhum específico de MPRIS/screensaver — ambos são padrão em qualquer ambiente de
desktop Linux, ao contrário da bandeja de sistema (M6, `PLANO_VAD.md` §4.8) que tem
um caveat real de suporte no GNOME.

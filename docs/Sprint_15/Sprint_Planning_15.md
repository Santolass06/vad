# Sprint_Planning_15 — M6: Polish e empacotamento

**Milestone:** M6 (`PLANO_VAD.md` §9)
**Pré-requisito:** Sprint_14 fechada (M5b completo — todas as features de IA do v1
estão prontas antes do polish final).

## Objetivo

Polish visual, integração de sistema opcional (bandeja, PIP) e empacotamento
distribuível.

## Tarefas

1. `vad-app/src/theme.rs`: tema/animações finais, alinhado ao design "Cinema Violet"
   já publicado (`design/vad-ui-design.html`).
2. `vad-app/src/tray.rs`: `StatusNotifierItem` via `ksni` — **best-effort**,
   documentar na própria app que o GNOME sem a extensão AppIndicator não mostra o
   ícone (§4.8), em vez de o utilizador achar que está avariado.
3. PIP/always-on-top via `window.set_window_level` (`winit`/`eframe`) — fiável em X11,
   best-effort em Wayland (depende do compositor; GNOME Wayland não expõe isto por
   restrição do protocolo, §4.9).
4. Perfil de release (§5): `mimalloc` como alocador global, `lto="fat"`,
   `codegen-units=1`, `strip=true` — **sem `panic="abort"`** (contradiria o critério
   de M1 de erro tratado sem crash da app).
5. Empacotamento Flatpak com `ffmpeg` e `yt-dlp` incluídos no manifesto — remove a
   dependência de runtime do sistema na versão empacotada.
6. Medir RSS e tempo de arranque do binário final e **comparar ao baseline do M0**
   (Sprint_00) — é o número que fecha o argumento "mais leve e mais rápido" do
   projeto.

## Fora de âmbito

Qualquer feature nova de IA — se surgir uma ideia aqui, regista para o Pós-M6, não
infiltres no M6.

## Critério de saída (§9, M6)

Binário instalável; arranque e RAM medidos e comparados ao M0.

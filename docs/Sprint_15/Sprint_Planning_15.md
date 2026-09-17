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
2. `vad-app/src/tray.rs`: implementa `PlatformIntegration` (§4.25) para Linux —
   `StatusNotifierItem` via `ksni` — **best-effort**, documentar na própria app que o
   GNOME sem a extensão AppIndicator não mostra o ícone (§4.8), em vez de o
   utilizador achar que está avariado.
2a. **Landlock (Linux) — hardening, escopo estreito, não uma revogação cega.** Não
   "revogar escrita depois de abrir o ficheiro" — o VAD escreve legitimamente depois
   de abrir um ficheiro, em vários sítios: `~/.local/share/vad/models/` (Sprint_06),
   `~/.config/vad/config.toml` (Sprint_05), `~/.cache/vad/vad.log` (§5), exports de
   `.md` (Sprint_07) e o destino de exportação de clips escolhido pelo utilizador em
   runtime (Sprint_08) — e o Landlock só pode ficar mais restritivo, nunca voltar a
   alargar depois de aplicado. **Escopo correto:** restringir escrita apenas na
   **pasta que contém o ficheiro de media aberto para reprodução** — o VAD nunca
   precisa de escrever aí, só de ler; um `libmpv`/`whisper.cpp` comprometido não deve
   conseguir sobrescrever o vídeo/áudio original do utilizador. Os caminhos de
   escrita conhecidos da app (config, cache, modelos, exports) ficam fora desta
   restrição.
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

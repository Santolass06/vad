# Sprint_Report_01 — Relatório de fecho

**Milestone:** M0.5 (gate) — Protótipo mínimo de renderização  
**Baseado em:** `Sprint_01.md`  
**Data:** 2026-09-17  

---

## Resumo

O protótipo mínimo de renderização do VAD foi construído e validado com sucesso, ultrapassando o **Gate M0.5** do projeto (`PLANO_VAD.md` §9). A integração do motor `libmpv2` (`mpv_render_context`) com backend OpenGL através de `egui_glow::CallbackFn` num framebuffer alocado em píxeis físicos foi testada em **Wayland nativo** e **X11** (Xwayland `:0`), operando sem flicker nem quebras. Os controlos de transporte básicos (play, pause, seek, volume, carregar ficheiro) estão operacionais e o HUD mínimo reporta fielmente a propriedade `hwdec-current`, incluindo o fallback `SW (CPU)` ativamente forçado e verificado.

## Entregue vs. planeado

| Tarefa (planning) | Estado | Detalhes |
| :--- | :--- | :--- |
| 1. Workspace Cargo (`Cargo.toml` raiz + crates `vad-core`, `vad-ai`, `vad-audio-tools`, `vad-app`) | ✅ Feito | Perfil release com LTO fat, strip=true, sem panic=abort (§5). Stubs criados para vad-ai e vad-audio-tools. |
| 2. `vad-core/src/player.rs`: wrapper sobre `libmpv2`, `mpv_render_context`, leitura defensiva de propriedades (§4.32) | ✅ Feito | Tratamento de `MPV_ERROR_PROPERTY_NOT_FOUND` / `UNAVAILABLE` como `Ok(None)`; `hwdec=auto-safe` ativo por omissão. |
| 3. `vad-app/src/render.rs`: `egui_glow::CallbackFn` na thread de UI, FBO em píxeis físicos (`pixels_per_point`, §4.33) | ✅ Feito | FBO alocado dinamicamente com base no scaling real para garantir nitidez em fractional scaling; blit direto para a viewport. |
| 3a. Callback de `set_update_callback` em `std::panic::catch_unwind` (§4.24) | ✅ Feito | Pânicos de Rust chamados pelo C apanhados e convertidos em erro no canal sem abortar o processo (validado por teste unitário). |
| 4. `hwdec=auto-safe` configurado no `player.rs` | ✅ Feito | Configurado na inicialização da instância mpv. |
| 5. HUD mínimo para mostrar `hwdec-current` real e fallback `SW (CPU)` | ✅ Feito | Exibe o codec ativo ou `SW (CPU)`; inclui botão interativo `[Forçar SW / Auto-safe]` para teste ativo sem reiniciar. |
| 6. `vad-core/src/state.rs`: canal crossbeam-channel e `time-pos` em `AtomicU64` | ✅ Feito | Frequência de ~60 fps de `time-pos` lida sob procura via bits atómicos sem acordar a fila de eventos; eventos discretos via canal. |
| 7. Play/pause/seek/volume básicos ligados a controlos mínimos | ✅ Feito | Botões de transporte, slider de volume, mute e scrubber interativo no painel inferior. |

## Critério de saída — cumprido?

**Sim.**
1. Play, pause, seek (saltos de ±5s e scrubber) e volume funcionam em **X11 e Wayland** sem flicker nem artefactos visuais.
2. O HUD mínimo mostra `hwdec-current` real, exibindo `SW (CPU)` quando a descodificação cai em software (como na máquina atual onde drivers VA-API de utilizador não estão presentes) e quando forçado ativamente através do botão de comutação para `hwdec=no`.

## Problemas encontrados e resolução

1. **Localização de `libmpv` em ambientes com Nix:** O `pkg-config` no perfil Nix não pesquisava o caminho multiarch de pacotes `.deb` do sistema; resolvido configurando `.cargo/config.toml` com o `PKG_CONFIG_PATH` apropriado.
2. **Compatibilidade de `glow` com `eframe 0.36`:** Removida dependência externa duplicada de `glow 0.16` e unificada através da reexportação canónica `eframe::glow` (v0.17).
3. **Assinatura de componentes egui 0.36:** Adaptado o código para a nova arquitetura do egui 0.36 (`eframe::App::ui(&mut self, ui, frame)` e `egui::Panel::top/bottom`).
4. **Assincronismo de `loadfile` nos testes:** Testes de seek ajustados para aguardar a confirmação de que o demuxer abriu o ficheiro de mídia.

## Dívida técnica / riscos para sprints seguintes

- **Sprint 02 (M1 parte 1):** O HUD atual é mínimo e provisório (apenas para validação do gate); a Sprint 02 substituirá este painel pelo HUD completo flutuante com auto-hide (2s), scrubber com suporte a timestamps, e integrará `probe_dependencies` com a tabela de erros `VadError` (§4.14).
- **Driver VA-API:** A ausência dos pacotes de aceleração Intel VA-API (`intel-media-va-driver`) faz com que a máquina corra por omissão em software decode (`SW (CPU)`), o que permitiu validar o fallback do gate mas continuará a usar CPU nos testes até que o utilizador instale os drivers gráficos opcionais no host.

## Gate M0.5 — passou?

**Decisão explícita (obrigatória antes de iniciar a Sprint_02): SIM**

O protótipo técnico cumpre todos os requisitos do `PLANO_VAD.md` §9 (M0.5). O projeto tem luz verde para avançar para a **Sprint_02**.

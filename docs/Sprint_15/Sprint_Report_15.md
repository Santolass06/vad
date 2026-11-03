# Sprint_Report_15 — Relatório de fecho

**Milestone:** M6 — Polish, bandeja, PIP, empacotamento
**Baseado em:** `Sprint_15.md`

---

## Resumo

Entregue e verificado: tema com testes de acessibilidade (e correções reais de contraste e tamanho), `mimalloc`, botão de PIP, bandeja `ksni` e o painel de IA corrigido depois de visto no ecrã. **Não verificado:** bandeja, PIP, Flatpak. **Não feito:** Landlock (achado, decisão tua). **O M6 não fecha:** falta o critério «binário instalável» (Flatpak) e a comparação com o M0.

## Entregue vs. planeado

| Tarefa | Estado |
| :--- | :--- |
| 1. Tema + acessibilidade (AA, 12 px, foco, Escape) | ✅ testes automáticos + vistos no ecrã; ⚠️ sem ficheiro de design |
| 2. `tray.rs` (ksni, nota GNOME) | ⚠️ compila e arranca; ❌ ícone não visto (sem desktop com bandeja) |
| 2a. Landlock | ❌ não feito — ver `Landlock.md` |
| 3. PIP / always-on-top | ⚠️ implementado; ❌ não verificado (sem gestor de janelas) |
| 4. Perfil de release + `mimalloc` | ✅ (sem `panic=abort`) |
| 5. Flatpak | ❌ esqueleto não construído |
| 6. RSS/arranque vs M0 | ⚠️ medido no container; ❌ comparação ao M0 por fazer |

## Critério de saída — cumprido?

**Não.** «Binário instalável» não existe (Flatpak por construir) e «comparados ao M0» não é feito.

Medições do binário de release (23,9 MB, `strip`+LTO fat+mimalloc) em **container de 4 vCPU, Xvfb, renderização por software (llvmpipe), reprodução do ficheiro sintético de 90 s — NÃO COMPARÁVEL ao M0** (outra máquina, outro renderer; M0 era o VLC 3.0.23 com ~197 MiB):

| Métrica (3 execuções) | Resultado |
| :--- | :--- |
| Arranque até à janela visível (`xdotool search --sync`) | 0,51 / 1,01 / 0,51 s — **não** é a métrica do M0 (que mediu arranque+1 s+saída com `hyperfine`) |
| RSS a reproduzir | 331 / 344 / 336 MiB |
| RSS «em pausa» | 334 / 337 / 337 MiB — **a pausa por tecla não foi confirmada**, e o CPU lido (~350 %) mostra que o app não estava em repouso (renderização por software); por isso **não há número válido de CPU em pausa** |

Não escrevo «mais leve/rápido que o VLC»: estes números não o sustentam nem o contrariam. Repetir na tua máquina com a metodologia do `Sprint_00.md` (`top`, nunca `ps %cpu`).

## Problemas encontrados e resolução

- O painel de IA saía da janela (visto no ecrã): corrigido e com scroll.
- Contrastes/tamanhos abaixo do limite no código existente: corrigidos e protegidos por teste de varrimento.

## Dívida técnica / riscos para sprints seguintes

- Decidir sobre Landlock (opções A/B/C em `Landlock.md`).
- Construir e validar o Flatpak (App ID, hashes, módulo mpv/ffmpeg, `cargo-sources.json`).
- Medir na máquina do M0; verificar bandeja (KDE e GNOME+extensão) e PIP (X11 e Wayland).
- Fornecer `design/vad-ui-design.html` e `Settings.dc.html` para alinhar a paleta e rever os cartões.
- Sprint_16 é opcional; sigo para a 17 (M7).

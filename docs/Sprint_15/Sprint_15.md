# Sprint_15 — Diário de bordo

**Milestone:** M6 — Polish, bandeja, PIP, empacotamento
**Planning:** ver `Sprint_Planning_15.md`
**Início:** 2026-11-03
**Fim:** 2026-11-03 (parcial — ver «Desvios»)

---

Regista aqui, por ordem cronológica, à medida que o trabalho avança — ver
`workflow.md` §3 (raiz do projeto) para o que incluir. Não apagues entradas antigas ao
corrigires-te; acrescenta uma entrada nova a corrigir a anterior — o histórico do que
não resultou tem valor para quem vier depois.

## Registo

### 2026-11-03

- **Ficheiros de design inexistentes** (`design/vad-ui-design.html`, `Settings.dc.html`). Não inventei uma paleta «Cinema Violet» a partir do nome: o acento já usado no código `(139,124,246)` é a fonte de verdade. Desvio registado.
- `theme.rs` (novo): constantes de cor + `apply()` (anel de foco de 2 px no acento). Confirmei no código do egui 0.36 que o foco de teclado usa `visuals.widgets.active` (`Style::interact`) e que o campo de texto usa `selection.stroke`, e foi aí que apliquei o anel. Cálculo WCAG (luminância relativa, composição alfa sobre fundo real) **como testes unitários** sobre as cores realmente usadas: texto ≥4.5:1 em painel sobre preto, painel sobre branco e cartão; anel/marcadores ≥3:1; badges sobre o fundo translúcido; valor de referência (#767676 em branco = 4.54).
- **Achados reais:** `Color32::RED` falhava AA (4.25–4.29:1 no cartão/fundo claro) e havia cinzentos soltos `(120,125,145)`, `(120,125,140)`, `(100,105,120)` e vermelho `(230,70,70)` abaixo de 4.5:1 → trocados por constantes do tema. **Correção ao que eu próprio tinha assumido:** `Color32::GRAY` do egui é cinzento 160 e passa (4.3 era conta minha com 128); trocado só por consistência, o teste diz-o.
- **Regra dos 12 px aplicada retroativamente:** havia 25× 11.0, 7× 10.5, 6× 11.5, e fontes 9/9.5/10 no EQ e na playlist → todas `MIN_TEXT_SIZE`=12. Dois testes varrem o código-fonte (`no_text_is_smaller_than_the_minimum_size`; `hard_coded_text_colors_in_sources_meet_aa`, que só vê cores escritas **na mesma linha** da chamada de texto — cores passadas noutra linha de uma chamada em várias linhas escapam); o primeiro apanhou dois `monospace(9.5)` que a minha substituição tinha falhado. Visto no ecrã (Xvfb): Whisper e Equalizador com 12 px sem sobreposição (rótulos do EQ «250/500» muito juntos mas legíveis).
- `Escape`: auditados todos os `egui::Window` (dependências, abrir, legendas) e o clip/toast — o handler de `app.rs` já os fecha todos; o ecrã de falha crítica não é descartável por desenho.
- **Bug visto no ecrã (Sprint_11/12, nunca visto antes):** o painel de IA saía da janela: `desired_width(f32::INFINITY)` e rótulos longos esticavam o painel de 340 px. Corrigido (larguras = `available_width`, `set_max_width`, títulos de cartão curtos) e o painel passou a ter scroll. Confirmado em captura: 3 cartões, badges, estados «Repouso» e «Sem chave», scroll até ao Gemini.
- PIP: `ViewportCommand::WindowLevel(AlwaysOnTop/Normal)` (verificado no código do egui) com botão «📌 Sempre visível» e aviso no tooltip em Wayland (GNOME ignora).
- Bandeja: `tray.rs` com `ksni` 0.3.7 (`blocking` + `async-io`), **mesma pilha `zbus` 5** do resto da app (`cargo tree -i zbus`: sem segunda implementação nem libdbus). Menu: mostrar/esconder, reproduzir/pausar, sair; nota GNOME/AppIndicator em `GNOME_NOTE`. Falha de registo nunca falha a app. **Não** implementei «fechar a janela esconde para a bandeja»: com o ícone invisível (GNOME) deixava a app sem caminho de volta.
- Perfil de release: `mimalloc` como alocador global; `lto="fat"`, `codegen-units=1`, `strip=true` já estavam; **sem `panic="abort"`**.
- Flatpak: só esqueleto em `packaging/flatpak/` com `TODO` explícitos (sem hashes inventados, sem `cargo-sources.json` à mão, sem módulo mpv). **Nunca construído** (sem `flatpak-builder`).
- Landlock: **não implementado** — ver `Landlock.md`.
- Medições (binário release, `xvfb-run`, llvmpipe, container de 4 vCPU, ficheiro sintético de 90 s regenerado com o comando do Sprint_00): ver Relatório. **Não comparáveis com o M0.**

## Desvios face ao Sprint_Planning_15.md

- **Tarefa 2a (Landlock): não feita, por decisão fundamentada** (`Landlock.md`).
- **Tarefa 5 (Flatpak): só esqueleto não construído.**
- **Tarefa 6: números do container, não comparáveis ao M0** (outra máquina/renderer/carga). A célula «vs M0» fica para ti.
- Tarefa 1: sem o ficheiro de design, a paleta não foi alinhada a «Cinema Violet»; testes de acessibilidade sobre as cores existentes.
- Bandeja e PIP **compilam e arrancam** mas não foram verificados: Xvfb sem gestor de janelas nem StatusNotifierWatcher.
- Não houve «animações» (o plano diz «tema/animações finais»): nada de novo nesse sentido.

## Problemas encontrados

- Painel de IA fora da janela (corrigido).
- Substituição em massa de tamanhos deixou `9.5`; apanhado pelo teste de varrimento.
- Cada build de release com LTO fat: ~4 min (8 min a frio).

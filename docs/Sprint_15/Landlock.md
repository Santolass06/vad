# Landlock (tarefa 2a) — achado, NÃO implementado

**Estado:** nenhum código Landlock foi escrito nesta sprint. É uma decisão para o utilizador.

## Porque não segui o plano à letra

1. **O escopo pedido («proibir escrita só na pasta do media») não parece exprimível em Landlock.**
   Pelo que conheço da documentação do kernel (a confirmar na do crate `landlock` antes de
   qualquer implementação), um ruleset declara os direitos que *trata* (`handled_access_fs`) e
   cada regra **concede** esses direitos numa hierarquia; o que é tratado e não concedido fica
   negado. Não há regra «negar escrita aqui»: uma concessão num ancestral cobre os
   descendentes. Para negar escrita numa pasta tem de se tratar `WRITE_*` e conceder escrita
   em **todo o resto** necessário (config, cache, modelos, `/tmp`, sockets D-Bus/Wayland/X11,
   dispositivos, `~/.local/share/vad`, e o que o mpv/ffmpeg/yt-dlp escrevem), ou seja, uma
   *allowlist* — a abordagem oposta à descrita.
2. **Quebra o corte de clips da Sprint_08 por omissão.** `clip_export.rs`
   (`resolve_output_path`, ~l.232) escreve o clip **na pasta do ficheiro de origem**, e o
   painel mostra «Pasta de destino: <pasta do media>» (`clip_export_panel.rs`, ~l.635). O
   plano descreve esta pasta como a que nunca precisa de escrita — não é verdade com o
   comportamento atual. Proteger essa pasta obrigaria a mudar o destino por omissão dos clips.
3. **Irreversível por processo.** Um ruleset errado só falha em runtime, em máquinas/kernels
   que não pude testar aqui (Landlock também depende da versão do kernel e pode estar
   desativado). Um erro silencioso aqui parte funcionalidades (cache, modelos, exports).

## Opções para decidires

| Opção | O que implica |
| :--- | :--- |
| A. Não fazer Landlock no v1 | Sem risco; fica como hardening pós-M6. |
| B. Allowlist de escrita | Tratar escrita e conceder só os caminhos conhecidos (+ runtime de GL/áudio/D-Bus); o clip passa a ir por omissão para `~/.local/share/vad/clips`. Requer testar em ≥2 kernels/desktops. |
| C. Landlock só para o *subprocesso* ffmpeg/yt-dlp | Aplicar o ruleset no processo filho (`pre_exec`), onde o conjunto de escritas é pequeno e conhecido; é onde está o risco real de input malicioso. Parece o melhor custo/benefício. |

Recomendação: **C**, depois de confirmares que queres mudar o destino por omissão dos clips (ou
dar ao ffmpeg só a pasta de saída escolhida).

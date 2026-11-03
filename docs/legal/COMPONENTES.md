# Componentes não-Rust e binários externos

> Evidência técnica, **não** parecer jurídico. Cada licença abaixo foi lida na fonte indicada em
> **2026-11-03**, neste container (Ubuntu 24.04). O que não pude verificar está marcado ❓ e
> repetido na lista de pendências 🧑‍⚖️ do `docs/Sprint_17/Sprint_Report_17.md`.

| Componente | Versão vista | Licença (fonte da verificação) | Ligação | Obrigação / nota |
| :--- | :--- | :--- | :--- | :--- |
| `libmpv` (`libmpv2` Ubuntu) | 0.37.0-1ubuntu4 | `/usr/share/doc/libmpv2/copyright`: ficheiros `LGPL-2.1+`, `GPL-2+`, BSD, ISC, CC0, Expat. A biblioteca instalada foi compilada com a opção `gpl` (aparece na lista de funcionalidades embebida na `libmpv.so.2`) → build **GPL-2+** | **Dinâmica** (`libmpv.so.2`, confirmado com `ldd` no binário de release) | É a razão do VAD ser GPL (§1). Um binário que liga uma `libmpv` GPL tem de ser distribuído sob GPL compatível. |
| Bindings `libmpv2`, `libmpv2-sys` (Rust) | 6.0.0 / 4.0.1 | Cargo.toml: `LGPL-2.1` — **identificador SPDX obsoleto/ambíguo** (o `cargo-deny` avisa: não diz «only» ou «or-later») ❓ | Estática (código Rust compilado no binário) | LGPL-2.1 ligada estaticamente a um binário GPL-3.0: 🧑‍⚖️ confirmar a combinação e as obrigações de relinking. |
| `ffmpeg` (CLI) | 6.1.1-3ubuntu5 | `ffmpeg -version`: configurado com `--enable-gpl` (**GPL**); `/usr/share/doc/ffmpeg/copyright`: LGPL-2.1+ e GPL-2+ | **Subprocesso** (`extractor.rs`, `clip_export.rs`), do sistema | Não é distribuído pelo VAD enquanto for o do sistema. No Flatpak (Sprint_15, **não construído**) passaria a ser distribuído: ver tarefa 7. |
| `libavcodec` & co. | 60 (Ubuntu) | `/usr/share/doc/libavcodec60/copyright` idem | Dinâmica **indireta**, através da `libmpv` | Idem `libmpv`. Codecs com patentes (H.264/HEVC/AAC): ❓ 🧑‍⚖️ não analisado. |
| `yt-dlp` | 2024.04.09 (Ubuntu) | `/usr/share/doc/yt-dlp/copyright`: **public-domain** (Unlicense) para o programa; GPL-2+ para o empacotamento Debian | **Subprocesso**, chamado pelo `mpv` (opção `ytdl`), do sistema | Nada a incluir enquanto for o do sistema. Se for embebido no Flatpak: fixar versão e hash, e manter o texto da licença. |
| `whisper.cpp` / `ggml` (via `whisper-rs-sys` 0.15.0) | submódulo incluído no crate | `whisper.cpp/LICENSE` no crate: **MIT** («The ggml authors»); crate `whisper-rs-sys`: `Unlicense` | **Estática** (compilada pelo `build.rs`/CMake) | Incluir o aviso MIT: está em `THIRD_PARTY_NOTICES.md` (texto MIT) e deve acompanhar o binário. |
| `oniguruma` (via `onig_sys` 69.9.3, usada por `tokenizers`) | 69.9.3 | `onig_sys/oniguruma/COPYING`: **BSD-2-Clause** (K. Kosako); crate Rust: MIT | **Estática** | Incluir o aviso BSD-2. |
| `candle` (`candle-core`, `-nn`, `-transformers` 0.11.0) | 0.11.0 | Cargo.toml: `MIT OR Apache-2.0` | Estática | Avisos em `THIRD_PARTY_NOTICES.md`. |
| `tokenizers` | 0.23.2 | Cargo.toml: `Apache-2.0` | Estática | Idem. |
| `libdbus-1` (via `keyring`/`dbus-secret-service`) | libdbus-1-3 Ubuntu | `/usr/share/doc/libdbus-1-3/copyright`: «GPL-2+ **or** AFL-2.1», Expat, Tcl-BSDish | **Dinâmica** (`libdbus-1.so.3`, `ldd`); **dependência de build e de execução** (`libdbus-1-dev` para compilar) | O `keyring` v3 com `sync-secret-service` puxa a `libdbus` em C, ao contrário do resto do D-Bus do VAD (`zbus`, Rust puro). Dívida: avaliar `zbus`-based. |
| `libssl3` / `libcrypto3`, `libmbedcrypto`, `libk5crypto`, `libva`, `libX11`, `libxkbcommon` | do sistema | ❓ **Não verifiquei** as licenças destas bibliotecas | **Dinâmicas** (aparecem no `ldd`, a maioria **transitivas via `libmpv`**) | Não são distribuídas pelo VAD enquanto forem as do sistema. No Flatpak seriam do runtime freedesktop. |
| Fontes embebidas pelo egui (`epaint_default_fonts` 0.36.2) | 0.36.2 | Ficheiros de licença no crate: `OFL.txt`, `UFL.txt`, `Hack-Regular.txt`, `emoji-icon-font-mit-license.txt`; Cargo.toml: `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0` | **Embebidas no binário** | Textos incluídos em `THIRD_PARTY_NOTICES.md`. |
| Modelos Whisper (ggml) e Qwen2.5-0.5B-Instruct (GGUF + tokenizer) | — | Declaradas nos `ModelPreset` (MIT; Apache-2.0). ❓ **Não verificadas nas páginas dos modelos** (o `huggingface.co` está bloqueado neste ambiente) | **Descarregados em runtime**, nunca redistribuídos pelo VAD | A UI mostra a licença antes da descarga, com «por verificar». |
| Modelo RNNoise (`.rnnn`) | — | ❓ Não verificado | Ficheiro do utilizador (§4.35), sem descarga | — |

## O que este documento NÃO conclui

- Compatibilidade jurídica das combinações (GPL-3.0 + LGPL-2.1 estática; GPL-3.0 + Ubuntu Font
  Licence/OFL; AFL/GPL do dbus).
- Patentes de codecs (H.264/HEVC/AAC) e a política do canal de distribuição.
- Se `GPL-3.0-only` ou `GPL-3.0-or-later` (decisão do utilizador; os `Cargo.toml` usam
  `license-file` até lá).

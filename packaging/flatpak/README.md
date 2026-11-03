# Flatpak (Sprint_15) — esqueleto, NÃO construído

`io.github.santolass06.Vad.yml` é um **esqueleto não validado**: não existe
`flatpak-builder` no ambiente em que foi escrito, por isso nunca foi construído nem
corrido. Tudo marcado `TODO` tem de ser resolvido por quem tiver o toolchain.

Pendências conhecidas:

- **App ID** `io.github.santolass06.Vad` é um palpite derivado do dono do repositório;
  confirma-o (e o domínio/reverse-DNS) antes de publicar.
- **`sha256` de `yt-dlp`, `ffmpeg` e `mpv` estão em `TODO`.** Não inventar hashes: obtê-los
  das releases oficiais que forem escolhidas e fixar versão + hash.
- **Fontes do cargo:** gerar `cargo-sources.json` a partir do `Cargo.lock` com
  `flatpak-cargo-generator` (https://github.com/flatpak/flatpak-builder-tools); não escrever
  à mão.
- **`libmpv`:** não vem no runtime freedesktop. É preciso um módulo `mpv` (base: o manifesto
  do mpv no Flathub) com `libplacebo`, `ffmpeg`, `libass`, etc.
- **ffmpeg CLI:** a extensão `org.freedesktop.Platform.ffmpeg-full` dá as bibliotecas, não
  necessariamente o executável `ffmpeg` que `extractor.rs`/`clip_export.rs` invocam.
  Decidir entre construir o `ffmpeg` como módulo ou usar o da extensão (verificar).
- **Modelos:** Whisper/Qwen são descarregados em runtime para `~/.local/share/vad/models`
  (dentro do sandbox: `~/.var/app/<id>/data/vad/models`). Não os incluir no pacote.
- **Distribuição GPL:** ver M7/Sprint_17 (obrigações de código-fonte correspondente).
- **GNOME + bandeja:** sem a extensão AppIndicator o ícone não aparece (§4.8).

Para validar (quando houver `flatpak-builder`):

```bash
flatpak-builder --user --install --force-clean build-dir packaging/flatpak/io.github.santolass06.Vad.yml
flatpak run io.github.santolass06.Vad
```

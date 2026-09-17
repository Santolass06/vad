# Sprint_Planning_08 — M4: Corte de clips e redução de ruído

**Milestone:** M4 (`PLANO_VAD.md` §9)
**Pré-requisito:** Sprint_07 fechada (M3 completo).

## Objetivo

Corte/exportação de clips e redução de ruído. A waveform já existe desde a Sprint_06
— a seleção de troço não é trabalho novo aqui, só a UI de exportação em cima dela.

## Tarefas

1. `vad-audio-tools/src/clip_export.rs`: subprocesso `ffmpeg`, corte por keyframe por
   omissão (§8):
   ```
   ffmpeg -ss {inicio} -to {fim} -i {input} -c copy -avoid_negative_ts 1 {output}
   ```
   **Argumentos como vetor (`Command::args`), nunca via shell, com `--` antes dos
   caminhos de input/output** (§4.26) — mesma regra do `extractor.rs` (Sprint_06),
   aqui com o agravante de o caminho de saída ser escolhido pelo utilizador em
   runtime.
2. Checkbox "Corte exato (recodificar)" — desligado por omissão (rápido, `-c copy`,
   encaixa no keyframe); ligado usa `-c:v libx264 -crf 18 -c:a aac` (mais lento,
   preciso ao frame, ficheiro pode crescer). O utilizador escolhe conscientemente, o
   v1 nunca impõe o corte por keyframe em silêncio.
3. UI de exportação (design `ClipExport.dc.html`): seleção na waveform com handles,
   marcas de keyframe, inputs de início/fim/duração, nome do ficheiro, botão Exportar.
   Botão "▶ Reproduzir Seleção" para pré-visualizar o troço antes de exportar, e
   atalhos de teclado `I`/`O` para marcar início/fim a partir da posição atual —
   sem isto, definir os limites do corte é só arrastar handles no rato.
4. Toggle de redução de ruído: `af=arnndn` (RNNoise, já compilado no `libavfilter`
   desta distro — §1/§3). É um **filtro do mpv**, não uma crate própria nem extração
   manual de PCM.

## Fora de âmbito

Qualquer LLM — Sprint_09+.

## Critério de saída (§9, M4)

Selecionar troço na waveform exporta ficheiro válido; **ambos** os modos de corte
(keyframe e exato) funcionam e produzem ficheiros reproduzíveis.

## Risco conhecido

Com `-ss` antes de `-i` e `-c copy`, o corte encaixa no keyframe mais próximo — não é
um bug, é o tradeoff já documentado no §8. Confirmar que a UI comunica isto **antes**
do utilizador exportar (o texto do checkbox já existe no design), não depois de o
utilizador reparar que o corte "não é exato".

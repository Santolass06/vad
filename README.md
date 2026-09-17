# VAD — Video & Audio Decoder

Leitor multimédia para Linux, escrito em Rust sobre `libmpv`, com uma camada de
produtividade orientada a IA que o VLC não tem: transcrição (Whisper), resumo
automático, tradução de legendas, redução de ruído e corte/exportação rápida de clips.

O motor de reprodução (descodificação, sync A/V, legendas, hwdec, DVD/Blu-ray) é
delegado ao `libmpv` — já maduro e testado em produção. O trabalho deste projeto é a
interface, a camada de IA e as otimizações de aplicação (ver [`PLANO_VAD.md`](./PLANO_VAD.md)).

## Estado

Em fase de planeamento. Ver [`PLANO_VAD.md`](./PLANO_VAD.md) para arquitetura, decisões
tomadas e milestones.

## Licença

GPL-3.0 — ver [`LICENSE`](./LICENSE). Decisão herdada da dependência em `libmpv`
(GPLv2+ nesta distribuição).

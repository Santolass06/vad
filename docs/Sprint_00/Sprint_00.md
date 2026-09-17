# Sprint_00 — Diário de bordo

**Milestone:** M0 — Baseline real do VLC
**Planning:** ver `Sprint_Planning_00.md`
**Início:** 2026-09-17
**Fim:** 2026-09-17

---

## Registo

### 2026-09-17 — pré-voo (só leitura)

- `which vlc` → `/snap/bin/vlc`; `vlc --version` cuspiu `libGL error: failed to load driver: iris` + `failed to create dri screen` — confirma o risco conhecido do planning (snap contaminado).
- `snap list vlc` → `3.0.20-1-g2617de71b6`. Sessão `wayland`, `DISPLAY=:0`.
- `which hyperfine` → vazio; `ffmpeg`/`ffprobe` presentes (ffmpeg 8.0.1). Sem `~/Videos/`.

### 2026-09-17 — tarefa 1: troca snap → .deb (executada pelo utilizador no terminal dele)

- `sudo snap remove vlc` → ok ("vlc removed").
- `sudo apt update + apt install -y vlc hyperfine` → 52 pacotes, VLC **3.0.23 Vetinari** (`3.0.23-2-0-g79128878dd`, build Ubuntu Jan 2026), hyperfine **1.19.0**.
- Decisão: por sugestão do utilizador ("dá-me os comandos para evitar falhas do sudo"), ele correu o bloco sudo no terminal dele em vez do agente — evita problemas de password/TTY do lado do agente. Registado aqui porque altera "quem executou o quê".

### 2026-09-17 — tarefa 5: ficheiro de teste canónico

- Gerado com ffmpeg (sintético, determinístico, sem rede):
  `ffmpeg -y -f lavfi -i "testsrc2=size=1920x1080:rate=30:duration=90" -f lavfi -i "sine=frequency=440:duration=90" -c:v libx264 -pix_fmt yuv420p -crf 18 -c:a aac -shortest /tmp/M0_test_1080p_h264_aac.mp4`
- `ffprobe`: vídeo `h264, 1920x1080, 30/1`, áudio `aac, 44100 Hz, 1 canal`, duração `90.0s`, tamanho `113579873 bytes` (~108.3 MiB).
- Ficheiro vive em `/tmp/` (volátil); o artefacto reprodutível é o comando acima — em M6 regenera-se com o mesmo comando em vez de se confiar no ficheiro.

### 2026-09-17 — tarefa 1 (cont.): smoke test do .deb

- `timeout -s TERM 15 vlc /tmp/M0_test_1080p_h264_aac.mp4` → exit 124 (esperado, o timeout matou ao fim de 15s de reprodução).
- `grep -c -i -E 'libGL|iris'` no log → **0 ocorrências**. Sem `libGL error` — tarefa 1 verde, medições desbloqueadas.
- Ruído observado (não bloqueante): `vaInitialize: unknown libva error` (iHD/i965 ausentes → fallback SW, comportamento esperado e documentado no §6 do plano) e `Failed to open VDPAU backend libvdpau_nvidia.so` (sem NVIDIA nesta máquina). `get_buffer() failed / no frame!` no fim do log coincide com o SIGTERM do timeout a meio da descodificação — artefacto do método, não falha de render.

### 2026-09-17 — tarefas 2+4: RSS e CPU em pausa

- VLC lançado em background com o ficheiro; pausa via MPRIS (`dbus-send ... Player.Pause`), estado confirmado: `PlaybackStatus = "Paused"`.
- RSS em pausa (`ps -o rss=`): **202108 KB** em 5/5 amostras, estável → **~197.4 MiB**.
- **Armadilha registada:** `ps -o %cpu=` em pausa deu 71.1 → 66.0 → 61.6 → 57.7 → 54.3 — parecia CPU alto em pausa, mas é a média cumulativa desde o arranque do processo, não valor instantâneo. Medição correta com `top -b -n 5 -d 1`: **0.0 / 2.0 / 1.0 / 1.0 / 1.0 %** → estabiliza **~1%**, perto de 0% como esperado. Quem repetir isto em M6: usar `top`/`pidstat`, nunca `ps %cpu`.

### 2026-09-17 — tarefa 2 (cont.): RSS e CPU em playback

- Resume via MPRIS (`Player.Play`), 8s de estabilização.
- RSS em playback: 202452 → 202464 KB → **~197.7 MiB** (praticamente igual à pausa; +~350 KB).
- CPU em playback (`top` 3×1s): **72.7 / 79.0 / 86.0 %** — alto porque é descodificação **por software** (VA-API indisponível nesta sessão) de `testsrc2` 1080p30, padrão sintético de alta entropia. É um limite superior, não conteúdo típico. Fica registado como caveat metodológico.

### 2026-09-17 — tarefa 3: tempo de arranque

- **Caminho morto registado:** primeira tentativa `hyperfine --runs 10 'vlc --intf dummy --play-and-exit <ficheiro 90s>'` — cada run reproduz o ficheiro inteiro (90s), 10 runs ≈ 15 min, estourou o timeout de 300s. Solução: `--run-time 1` (reproduz 1s e sai); verificado antes com `time` (1 run: real 1.826s, exit 0).
- `hyperfine --warmup 2 --runs 10 --export-json` → summary: mean 1.822s ± 0.033, range 1.774–1.862. Mediana calculada do JSON (o summary do hyperfine 1.19 só mostra média): valores ordenados 1.774…1.862, **mediana = 1.831 s** (total parede = arranque + 1s de playback + saída).
- Arranque+saída líquido ≈ **0.83 s** (1.831 − 1.0 de `--run-time`), reportado como derivado, não medido diretamente.
- `pgrep -x vlc` limpo no fim de cada bloco — sem processos órfãos.

## Desvios face ao Sprint_Planning_00.md

- Bloco sudo executado pelo utilizador no terminal dele (pedido dele), não pelo agente — mesmo resultado, executor diferente.
- Ficheiro de teste em `/tmp/` em vez de pasta persistente; compensado por registar o comando gerador exato (reprodutível para M6).
- Arranque medido com `--run-time 1` (inclui 1s de playback + saída); número líquido de arranque é derivado por subtração, declarado como tal.

## Problemas encontrados

1. `ps %cpu` engana em pausa (média cumulativa) — resolvido com `top` instantâneo. Ver entrada tarefas 2+4.
2. `hyperfine` sem `--run-time` num ficheiro de 90s multiplica o tempo por nº de runs — resolvido com `--run-time 1`. Ver entrada tarefa 3.
3. VA-API indisponível nesta sessão (iHD/i965 ausentes) → todo o playback medido é SW decode; CPU de playback é limite superior. Sem resolução nesta sprint — é facto da máquina, fica como caveat.

# Privacidade — o que sai do teu PC

> Documento de evidência técnica, **não** um parecer jurídico. A redação final é uma pendência 🧑‍⚖️
> (ver `docs/Sprint_17/Sprint_Report_17.md`).

**O VAD não tem telemetria, análises nem relatórios de falhas.** Não envia nada sobre ti, o teu
PC ou o que reproduzes por iniciativa própria. As únicas ligações de saída são as da tabela
abaixo, e cada uma só acontece porque *tu* a pediste.

## Ligações de saída

| # | Para onde | Quando | O que sai | O que NÃO sai |
| :-: | :--- | :--- | :--- | :--- |
| 1 | `huggingface.co` (modelos Whisper e Qwen) | Só quando carregas em «Descarregar» | Um pedido HTTP GET do ficheiro do modelo (o teu IP e o cabeçalho HTTP normal) | Nenhum ficheiro, texto ou áudio teu |
| 2 | O endereço que escreveres em «Abrir URL» (via `mpv` e `yt-dlp`) | Só quando abres um URL | O pedido ao site que indicaste | — (o `yt-dlp` é executado por ti com o URL que escolheste) |
| 3 | `api.openai.com` **ou** o endpoint personalizado que configurares | Só se escolheres o provider OpenAI-compatible em Definições → IA **e** pedires um resumo/tradução | O **texto da transcrição** (blocos do resumo ou segmentos a traduzir) e a tua chave de API no cabeçalho | Áudio, vídeo, nomes de ficheiro |
| 4 | `api.anthropic.com` | Só se escolheres Anthropic e pedires resumo/tradução | Idem (3) | Idem (3) |
| 5 | `generativelanguage.googleapis.com` | Só se escolheres Gemini e pedires resumo/tradução | Idem (3); a chave vai no cabeçalho `x-goog-api-key`, nunca no URL | Idem (3) |
| 6 | O mesmo destino de (3)–(5), por «Testar ligação» | Só ao carregar nesse botão | Um pedido mínimo («ping») e a chave | Transcrição |
| 7 | `github.com` (ligação «Abrir o código-fonte» em Sobre) | Só ao carregares nessa ligação: abre o **navegador**, o VAD não faz o pedido | — (é o navegador que liga) | Nada do VAD |

Por omissão o provider é **Local** (Qwen no teu PC): resumo e tradução não enviam nada.
O emblema **🔒 Local / ☁️ Sai do PC** mostra, antes de cada operação, para onde vai o texto.
Se um provider cloud estiver sem rede, o VAD pode refazer a operação **localmente** e di-lo
(aviso ⚠ no resultado e no `.md` exportado); nunca troca de caminho em silêncio.

## O que fica guardado no teu PC

| Onde | O quê |
| :--- | :--- |
| `~/.config/vad/config.toml` | Preferências (volume, atalhos, provider, nomes de modelos, `base_url`). **Nunca chaves de API.** |
| Keyring do sistema (Secret Service) | As chaves de API, se escolheres guardá-las. Sem keyring, ficam só em memória nessa sessão. Alternativa: variáveis `VAD_OPENAI_KEY`, `VAD_CUSTOM_KEY`, `VAD_ANTHROPIC_KEY`, `VAD_GEMINI_KEY`. |
| `~/.local/share/vad/` | Modelos descarregados, notas de reunião exportadas, marcadores, recentes. |

Os modelos descarregados e os teus ficheiros nunca são redistribuídos pelo VAD.

## Notas

- Transcrever ou gravar conversas de terceiros pode exigir o consentimento deles segundo a lei do
  teu país. A responsabilidade é tua. 🧑‍⚖️ (redação final por rever)
- Reproduzir por URL está sujeito aos termos da plataforma e ao direito de autor. O VAD não
  redistribui conteúdo.
- Há ligações **indiretas** que o VAD não controla: o `mpv`/`yt-dlp` falam com o site do URL
  (e com os CDN dele); o `huggingface.co` pode redirecionar a descarga para os seus CDN. Estes
  anfitriões intermédios **não foram enumerados** (sem acesso à rede nesta verificação).

<!-- egress-allowlist-begin
huggingface.co
api.openai.com
api.anthropic.com
generativelanguage.googleapis.com
github.com
egress-allowlist-end -->

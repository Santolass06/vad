use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

use eframe::egui::{self, Color32, RichText, Ui};
use tracing::{error, info};
use vad_ai::{
    find_preset, DiskModelInfo, ModelManager, ModelPreset, ModelSource, PcmAudio,
    TranscriptionSegment, WhisperEngine, DISK_TOOLTIP, PRESET_MODELS, RAM_ONLY_TOOLTIP,
};
use vad_core::{ModelStorageMode, VadConfig};

/// Action emitted by the Whisper panel to interact with the media player.
#[derive(Debug, Clone)]
pub enum WhisperAction {
    SeekTo(f64),
    SaveConfig,
}

/// State for the lateral Whisper AI panel per design/Meeting.dc.html and PLANO_VAD.md §4.13.
pub struct WhisperPanel {
    model_manager: ModelManager,
    selected_preset_idx: usize,
    storage_mode: ModelStorageMode,
    active_engine: Arc<Mutex<Option<WhisperEngine>>>,
    active_model_id: Arc<Mutex<Option<String>>>,
    download_progress: Arc<Mutex<Option<f32>>>,
    download_error: Option<String>,
    transcribing: Arc<AtomicBool>,
    transcribe_progress: Arc<Mutex<Option<i32>>>,
    transcription_segments: Vec<TranscriptionSegment>,
    transcription_error: Option<String>,
    transcription_rx: Option<crossbeam_channel::Receiver<Result<Vec<TranscriptionSegment>, String>>>,
    cached_disk_models: Vec<DiskModelInfo>,
    last_models_refresh: Instant,
    export_notification: Option<(String, Instant)>,
}

impl Default for WhisperPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl WhisperPanel {
    pub fn new() -> Self {
        let manager = ModelManager::new();
        let disk_models = manager.list_disk_models();

        Self {
            model_manager: manager,
            // Default to base-q5 (~55 MB) per §5
            selected_preset_idx: 1,
            storage_mode: ModelStorageMode::Disk,
            active_engine: Arc::new(Mutex::new(None)),
            active_model_id: Arc::new(Mutex::new(None)),
            download_progress: Arc::new(Mutex::new(None)),
            download_error: None,
            transcribing: Arc::new(AtomicBool::new(false)),
            transcribe_progress: Arc::new(Mutex::new(None)),
            transcription_segments: Vec::new(),
            transcription_error: None,
            transcription_rx: None,
            cached_disk_models: disk_models,
            last_models_refresh: Instant::now(),
            export_notification: None,
        }
    }

    /// Syncs settings from `VadConfig` (§5).
    pub fn init_from_config(&mut self, config: &VadConfig) {
        self.storage_mode = config.whisper.default_storage_mode;
    }

    /// Refreshes list of models stored in `~/.local/share/vad/models/`.
    pub fn refresh_disk_models(&mut self) {
        self.cached_disk_models = self.model_manager.list_disk_models();
        self.last_models_refresh = Instant::now();
    }

    /// Returns true if a model is currently loaded in memory and ready for inference.
    pub fn has_active_model(&self) -> bool {
        self.active_engine
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }

    /// Returns the name of the currently active model.
    pub fn active_model_id(&self) -> Option<String> {
        self.active_model_id.lock().ok()?.clone()
    }

    /// Renders the Whisper AI lateral panel.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        current_audio: Option<&PcmAudio>,
    ) -> Option<WhisperAction> {
        let mut action = None;
        let accent = Color32::from_rgb(139, 124, 246);

        // Non-blocking poll for transcription completion
        if let Some(ref rx) = self.transcription_rx {
            if let Ok(res) = rx.try_recv() {
                match res {
                    Ok(segs) => {
                        info!("Transcription finished with {} segments", segs.len());
                        self.transcription_segments = segs;
                        self.transcription_error = None;
                    }
                    Err(err) => {
                        error!("Transcription error: {}", err);
                        self.transcription_error = Some(err);
                    }
                }
                self.transcription_rx = None;
            }
        }

        // Periodically refresh list of disk models (every 5 seconds)
        if self.last_models_refresh.elapsed().as_secs() > 5 {
            self.refresh_disk_models();
        }

        ui.vertical(|ui| {
            // --- SECTION 1: Model Preset Selection ---
            ui.label(
                RichText::new("MODELO")
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(160, 165, 185)),
            );
            ui.add_space(4.0);

            let current_preset = PRESET_MODELS.get(self.selected_preset_idx).copied();
            let selected_name = current_preset
                .map(|p| p.display_name)
                .unwrap_or("Escolher modelo");

            egui::ComboBox::from_id_salt("whisper_preset_combo")
                .selected_text(RichText::new(selected_name).strong())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for (idx, preset) in PRESET_MODELS.iter().enumerate() {
                        let is_selected = self.selected_preset_idx == idx;
                        if ui.selectable_label(is_selected, preset.display_name).clicked() {
                            self.selected_preset_idx = idx;
                        }
                    }
                });

            ui.add_space(10.0);

            // --- SECTION 2: Storage Mode Selection & Tooltips (§4.13) ---
            ui.label(
                RichText::new("ARMAZENAMENTO")
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(160, 165, 185)),
            );
            ui.add_space(4.0);

            // Radio: Guardar no disco (Predefinição)
            let is_disk = self.storage_mode == ModelStorageMode::Disk;
            let disk_resp = ui.radio(is_disk, "Guardar no disco");
            if disk_resp.clicked() {
                self.storage_mode = ModelStorageMode::Disk;
                action = Some(WhisperAction::SaveConfig);
            }
            disk_resp.on_hover_text(DISK_TOOLTIP);

            // Radio: RAM-only (opt-in)
            let is_ram = self.storage_mode == ModelStorageMode::RamOnly;
            let ram_resp = ui.radio(is_ram, "RAM-only (esta sessão)");
            if ram_resp.clicked() {
                self.storage_mode = ModelStorageMode::RamOnly;
                action = Some(WhisperAction::SaveConfig);
            }
            ram_resp.on_hover_text(RAM_ONLY_TOOLTIP);

            ui.add_space(2.0);

            // Explanatory note under storage options
            let note_text = if is_disk {
                "Guardado em ~/.local/share/vad/models/. Carregado por mmap com baixo consumo de RAM."
            } else {
                "Volátil na memória. Zero ficheiros em disco. Descarrega a cada sessão."
            };
            ui.label(
                RichText::new(note_text)
                    .size(10.5)
                    .color(Color32::from_rgb(140, 145, 165)),
            );

            ui.add_space(10.0);

            // --- SECTION 3: Load / Download Model Action Button ---
            let dl_prog = *self.download_progress.lock().unwrap_or_else(|e| e.into_inner());
            let is_downloading = dl_prog.is_some();
            let active_id = self.active_model_id();

            if is_downloading {
                let pct = dl_prog.unwrap_or(0.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("A descarregar modelo: {:.0}%", pct));
                });
                ui.add(egui::ProgressBar::new(pct / 100.0).animate(true));
            } else {
                let is_current_active = active_id.as_deref() == current_preset.map(|p| p.id);

                if is_current_active {
                    ui.colored_label(accent, format!("✓ Modelo ativo: {}", active_id.as_deref().unwrap_or_default()));
                } else if let Some(preset) = current_preset {
                    let btn_text = if self.storage_mode == ModelStorageMode::Disk
                        && self.model_manager.is_model_on_disk(preset.filename)
                    {
                        format!("Ativar {}", preset.id)
                    } else {
                        format!("Descarregar e Ativar {}", preset.id)
                    };

                    let btn = egui::Button::new(RichText::new(btn_text).strong());
                    if ui.add_sized([ui.available_width(), 32.0], btn).clicked() {
                        self.start_model_load(preset, self.storage_mode);
                    }
                }
            }

            if let Some(ref err) = self.download_error {
                ui.colored_label(Color32::RED, format!("Erro: {err}"));
            }

            ui.separator();
            ui.add_space(4.0);

            // --- SECTION 4: Models on Disk List (§4.13, Meeting.dc.html) ---
            ui.label(
                RichText::new("MODELOS NO DISCO")
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(160, 165, 185)),
            );
            ui.add_space(4.0);

            if self.cached_disk_models.is_empty() {
                ui.label(
                    RichText::new("Nenhum modelo guardado em disco.")
                        .size(11.0)
                        .italics()
                        .color(Color32::from_rgb(130, 135, 155)),
                );
            } else {
                let mut to_delete = None;
                let mut to_activate = None;

                egui::Frame::new()
                    .fill(Color32::from_rgba_premultiplied(25, 27, 36, 255))
                    .corner_radius(8.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        for model in &self.cached_disk_models {
                            ui.horizontal(|ui| {
                                let is_active = active_id.as_deref() == Some(&model.id);

                                if is_active {
                                    ui.colored_label(accent, RichText::new(&model.id).strong());
                                } else {
                                    ui.label(&model.id);
                                }

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("🗑").on_hover_text("Eliminar modelo do disco").clicked() {
                                        to_delete = Some(model.filename.clone());
                                    }

                                    if is_active {
                                        ui.colored_label(accent, "Ativo");
                                    } else if ui.small_button("Ativar").clicked() {
                                        to_activate = Some(model.clone());
                                    }
                                });
                            });
                            ui.separator();
                        }
                    });

                if let Some(fname) = to_delete {
                    let _ = self.model_manager.delete_disk_model(&fname);
                    self.refresh_disk_models();
                }

                if let Some(model_info) = to_activate {
                    if let Some(preset) = find_preset(&model_info.filename) {
                        self.start_model_load(*preset, ModelStorageMode::Disk);
                    } else {
                        // Load custom model path directly
                        self.start_direct_disk_load(model_info.path, model_info.id);
                    }
                }
            }

            ui.separator();
            ui.add_space(4.0);

            // --- SECTION 5: Transcription ---
            ui.label(
                RichText::new("TRANSCRIÇÃO")
                    .size(11.0)
                    .strong()
                    .color(Color32::from_rgb(160, 165, 185)),
            );
            ui.add_space(4.0);

            let is_transcribing = self.transcribing.load(Ordering::SeqCst);
            let has_model = self.has_active_model();
            let has_audio = current_audio.is_some();

            if is_transcribing {
                let prog = self.transcribe_progress.lock().unwrap_or_else(|e| e.into_inner()).unwrap_or(0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!("A transcrever: {}%", prog));
                });
                ui.add(egui::ProgressBar::new(prog as f32 / 100.0).animate(true));
            } else {
                let can_transcribe = has_model && has_audio;
                let transcribe_btn = egui::Button::new(RichText::new("🎙 Transcrever áudio").strong());

                let resp = ui.add_enabled(can_transcribe, transcribe_btn);
                if !can_transcribe {
                    if !has_model {
                        resp.on_disabled_hover_text("Ativa um modelo Whisper primeiro");
                    } else {
                        resp.on_disabled_hover_text("Aguardando extração do áudio do ficheiro");
                    }
                } else if resp.clicked() {
                    if let Some(audio) = current_audio {
                        self.start_transcription(audio.clone());
                    }
                }
            }

            if let Some(ref err) = self.transcription_error {
                ui.colored_label(Color32::RED, format!("Erro de transcrição: {err}"));
            }

            // Results List
            if !self.transcription_segments.is_empty() {
                ui.add_space(6.0);
                let scroll_h = 220.0;

                egui::ScrollArea::vertical()
                    .max_height(scroll_h)
                    .show(ui, |ui| {
                        for seg in &self.transcription_segments {
                            ui.horizontal(|ui| {
                                let time_label = TranscriptionSegment::format_timestamp(seg.start_ms);
                                if ui
                                    .link(RichText::new(time_label).color(accent).monospace())
                                    .on_hover_text("Saltar reprodução para este ponto")
                                    .clicked()
                                {
                                    action = Some(WhisperAction::SeekTo(seg.start_ms as f64 / 1000.0));
                                }

                                ui.label(RichText::new(&seg.text).size(12.0));
                            });
                            ui.add_space(3.0);
                        }
                    });

                ui.add_space(6.0);

                if ui.button("📄 Exportar .md").on_hover_text("Guardar transcrição para Markdown").clicked() {
                    let title = current_audio.map(|a| a.path.as_str()).unwrap_or("Reunião");
                    let md_content = WhisperEngine::export_to_markdown(title, &self.transcription_segments);

                    // Save file next to source audio or in ~/.local/share/vad/
                    let export_dir = self.model_manager.models_dir().parent().unwrap_or(self.model_manager.models_dir());
                    let export_path = export_dir.join("transcricao_reuniao.md");

                    if std::fs::write(&export_path, md_content).is_ok() {
                        self.export_notification = Some((format!("Guardado em {:?}", export_path), Instant::now()));
                    }
                }
            }

            if let Some((ref msg, instant)) = self.export_notification {
                if instant.elapsed().as_secs() < 6 {
                    ui.colored_label(Color32::GREEN, msg);
                }
            }
        });

        action
    }

    /// Spawns model download & initialization in a background thread.
    fn start_model_load(&mut self, preset: ModelPreset, mode: ModelStorageMode) {
        self.download_error = None;
        let progress_state = Arc::clone(&self.download_progress);
        let active_engine_state = Arc::clone(&self.active_engine);
        let active_model_id_state = Arc::clone(&self.active_model_id);
        let manager = ModelManager::new();

        *progress_state.lock().unwrap() = Some(0.0);

        thread::spawn(move || {
            let prog_clone = Arc::clone(&progress_state);
            let result = manager.load_or_download_model(&preset, mode, move |pct| {
                if let Ok(mut guard) = prog_clone.lock() {
                    *guard = Some(pct);
                }
            });

            match result {
                Ok(source) => match WhisperEngine::load(&source) {
                    Ok(engine) => {
                        *active_engine_state.lock().unwrap() = Some(engine);
                        *active_model_id_state.lock().unwrap() = Some(preset.id.to_string());
                    }
                    Err(err) => {
                        error!("Failed to initialize WhisperEngine: {:?}", err);
                    }
                },
                Err(err) => {
                    error!("Model download failed: {:?}", err);
                }
            }

            *progress_state.lock().unwrap() = None;
        });
    }

    /// Loads an existing disk model file directly.
    fn start_direct_disk_load(&mut self, path: std::path::PathBuf, id: String) {
        let active_engine_state = Arc::clone(&self.active_engine);
        let active_model_id_state = Arc::clone(&self.active_model_id);

        thread::spawn(move || {
            match WhisperEngine::load(&ModelSource::Disk(path)) {
                Ok(engine) => {
                    *active_engine_state.lock().unwrap() = Some(engine);
                    *active_model_id_state.lock().unwrap() = Some(id);
                }
                Err(err) => {
                    error!("Failed to initialize direct disk model: {:?}", err);
                }
            }
        });
    }

    /// Spawns Whisper transcription on a background worker thread.
    fn start_transcription(&mut self, audio: PcmAudio) {
        self.transcription_error = None;
        self.transcription_segments.clear();
        self.transcribing.store(true, Ordering::SeqCst);

        let active_engine = Arc::clone(&self.active_engine);
        let transcribing = Arc::clone(&self.transcribing);
        let transcribe_progress = Arc::clone(&self.transcribe_progress);

        let (tx, rx) = crossbeam_channel::bounded::<Result<Vec<TranscriptionSegment>, String>>(1);

        thread::spawn(move || {
            let guard = active_engine.lock().unwrap();
            let Some(ref engine) = *guard else {
                let _ = tx.send(Err("Nenhum modelo Whisper ativo".to_string()));
                transcribing.store(false, Ordering::SeqCst);
                return;
            };

            let prog_clone = Arc::clone(&transcribe_progress);
            let progress_cb = move |pct: i32| {
                if let Ok(mut g) = prog_clone.lock() {
                    *g = Some(pct);
                }
            };

            let result = engine.transcribe(&audio, None, Some(progress_cb), None::<fn() -> bool>);

            let res_mapped = result.map_err(|e| e.to_string());
            let _ = tx.send(res_mapped);

            transcribing.store(false, Ordering::SeqCst);
            *transcribe_progress.lock().unwrap() = None;
        });

        self.transcription_rx = Some(rx);
    }

    /// Stores newly delivered transcription segments into state.
    #[allow(dead_code)]
    pub fn set_segments(&mut self, segments: Vec<TranscriptionSegment>) {
        self.transcription_segments = segments;
    }

    /// Returns the currently loaded transcription segments.
    pub fn transcription_segments(&self) -> &[TranscriptionSegment] {
        &self.transcription_segments
    }

    /// Returns whether transcription is currently executing.
    pub fn is_transcribing(&self) -> bool {
        self.transcribing.load(Ordering::SeqCst)
    }

    /// Returns current transcription progress percentage if active.
    pub fn transcribe_progress(&self) -> Option<i32> {
        self.transcribe_progress.lock().ok().and_then(|g| *g)
    }
}

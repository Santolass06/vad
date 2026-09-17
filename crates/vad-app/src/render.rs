use std::sync::Arc;
use eframe::egui;
use eframe::egui_glow;
use eframe::glow::{self, HasContext as _};
use tracing::{debug, error, warn};
use vad_core::{EventSender, VideoRenderContext};

/// Handles OpenGL video frame rendering and FBO management for mpv integration.
pub struct GlVideoRenderer {
    gl: Arc<glow::Context>,
    render_ctx: VideoRenderContext,
    fbo: Option<glow::NativeFramebuffer>,
    texture: Option<glow::NativeTexture>,
    fbo_width: i32,
    fbo_height: i32,
}

impl GlVideoRenderer {
    /// Creates a new `GlVideoRenderer`.
    /// The update callback is registered with `catch_unwind` protection (§4.24)
    /// to trigger UI repaints when a new video frame is ready.
    pub fn new(
        gl: Arc<glow::Context>,
        mut render_ctx: VideoRenderContext,
        egui_ctx: egui::Context,
        err_tx: Option<EventSender>,
    ) -> Self {
        // Register update callback with catch_unwind
        render_ctx.set_update_callback(
            move || {
                egui_ctx.request_repaint();
            },
            err_tx,
        );

        Self {
            gl,
            render_ctx,
            fbo: None,
            texture: None,
            fbo_width: 0,
            fbo_height: 0,
        }
    }

    /// Prepares and renders the current video frame to the internal FBO,
    /// reallocating the FBO in physical pixels if the viewport size or scaling changed (§4.33).
    pub fn prepare_frame(
        &mut self,
        rect: egui::Rect,
        pixels_per_point: f32,
    ) -> Result<Option<(glow::NativeFramebuffer, i32, i32)>, String> {
        let phys_w = (rect.width() * pixels_per_point).round().max(1.0) as i32;
        let phys_h = (rect.height() * pixels_per_point).round().max(1.0) as i32;

        let reallocated = if self.fbo.is_none() || self.fbo_width != phys_w || self.fbo_height != phys_h {
            self.reallocate_fbo(phys_w, phys_h)?;
            true
        } else {
            false
        };

        let Some(fbo) = self.fbo else {
            return Ok(None);
        };

        // Poll mpv render context flags
        // Flag 1 is MPV_RENDER_UPDATE_FRAME
        let flags = self.render_ctx.update().unwrap_or(0);
        let needs_render = reallocated || (flags & 1 != 0);

        if needs_render {
            let fbo_raw_id = fbo.0.get() as i32;
            if let Err(e) = self.render_ctx.render(fbo_raw_id, phys_w, phys_h) {
                error!("mpv render call failed: {:?}", e);
            }
        }

        Ok(Some((fbo, phys_w, phys_h)))
    }

    /// Allocates an offscreen FBO with RGBA8 texture attachment in physical pixels (§4.33).
    fn reallocate_fbo(&mut self, width: i32, height: i32) -> Result<(), String> {
        unsafe {
            if let Some(fbo) = self.fbo.take() {
                self.gl.delete_framebuffer(fbo);
            }
            if let Some(tex) = self.texture.take() {
                self.gl.delete_texture(tex);
            }

            let texture = self
                .gl
                .create_texture()
                .map_err(|e| format!("Failed to create GL texture: {e}"))?;
            self.gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            self.gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                width,
                height,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            self.gl
                .tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            self.gl
                .tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            self.gl
                .tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            self.gl
                .tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);

            let fbo = self
                .gl
                .create_framebuffer()
                .map_err(|e| format!("Failed to create GL framebuffer: {e}"))?;
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            self.gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(texture),
                0,
            );

            let status = self.gl.check_framebuffer_status(glow::FRAMEBUFFER);
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.bind_texture(glow::TEXTURE_2D, None);

            if status != glow::FRAMEBUFFER_COMPLETE {
                return Err(format!("FBO allocation incomplete: status 0x{status:x}"));
            }

            self.fbo = Some(fbo);
            self.texture = Some(texture);
            self.fbo_width = width;
            self.fbo_height = height;

            debug!(
                "Allocated physical FBO {}x{} (fractional scale support)",
                width, height
            );
            Ok(())
        }
    }

    /// Paints the rendered FBO into the given screen rect using `egui_glow::CallbackFn`.
    pub fn paint_to_rect(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let ppp = ui.ctx().pixels_per_point();
        match self.prepare_frame(rect, ppp) {
            Ok(Some((fbo, fbo_w, fbo_h))) => {
                let cb = egui_glow::CallbackFn::new(move |info, painter| {
                    let gl = painter.gl();
                    unsafe {
                        let prev_read_fb = gl
                            .get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING)
                            .cast_unsigned();
                        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(fbo));

                        let p_per_point = info.pixels_per_point;
                        let screen_h = info.screen_size_px[1] as f32;

                        let dst_x0 = (rect.min.x * p_per_point).round() as i32;
                        let dst_y0 = (screen_h - rect.max.y * p_per_point).round() as i32;
                        let dst_x1 = (rect.max.x * p_per_point).round() as i32;
                        let dst_y1 = (screen_h - rect.min.y * p_per_point).round() as i32;

                        gl.blit_framebuffer(
                            0,
                            0,
                            fbo_w,
                            fbo_h,
                            dst_x0,
                            dst_y0,
                            dst_x1,
                            dst_y1,
                            glow::COLOR_BUFFER_BIT,
                            glow::LINEAR,
                        );

                        let prev = std::num::NonZeroU32::new(prev_read_fb)
                            .map(glow::NativeFramebuffer);
                        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, prev);
                    }
                });

                ui.painter().add(egui::PaintCallback {
                    rect,
                    callback: Arc::new(cb),
                });
            }
            Ok(None) => {}
            Err(err) => {
                warn!("Frame preparation error: {}", err);
            }
        }
    }
}

impl Drop for GlVideoRenderer {
    fn drop(&mut self) {
        unsafe {
            if let Some(fbo) = self.fbo.take() {
                self.gl.delete_framebuffer(fbo);
            }
            if let Some(tex) = self.texture.take() {
                self.gl.delete_texture(tex);
            }
        }
    }
}

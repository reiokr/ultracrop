// UltraCrop 2.0 — a Rust + egui rewrite of the Python/Tk UltraCrop.
//
// Everything is drawn by egui itself: the canvas, the crop handles, the
// rule-of-thirds overlay and the live preview.  No widget toolkit raster is
// used, so the UI stays crisp at any DPI and looks the same everywhere.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use eframe::egui::{
    self, Align, Align2, Color32, CursorIcon, FontId, Layout, Margin, PointerButton, Pos2, Rect,
    Response, RichText, Rounding, Sense, Stroke, TextureHandle, TextureOptions, Ui, Vec2,
};
use image::{DynamicImage, GenericImageView, ImageFormat};

// ─── Palette ────────────────────────────────────────────────────────────
// Palette, copied verbatim from ultracrop.py lines 47–64.
const BG: Color32 = Color32::from_rgb(0x0f, 0x0f, 0x17);
const SURFACE: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x2e);
const SURFACE2: Color32 = Color32::from_rgb(0x16, 0x21, 0x3e);
const SURFACE3: Color32 = Color32::from_rgb(0x0f, 0x34, 0x60); // CARD
const BORDER: Color32 = Color32::from_rgb(0x1f, 0x40, 0x68);
const TEXT: Color32 = Color32::from_rgb(0xe2, 0xe8, 0xf0);
const TEXT_DIM: Color32 = Color32::from_rgb(0x94, 0xa3, 0xb8);
const ACCENT: Color32 = Color32::from_rgb(0x63, 0x66, 0xf1);
const ACCENT_HOVER: Color32 = Color32::from_rgb(0x4f, 0x46, 0xe5);
const AMBER: Color32 = Color32::from_rgb(0xf5, 0x9e, 0x0b);
const GREEN: Color32 = Color32::from_rgb(0x10, 0xb9, 0x81);
const CYAN: Color32 = Color32::from_rgb(0x06, 0xb6, 0xd4);
const PINK: Color32 = Color32::from_rgb(0xec, 0x48, 0x99);
const CANVAS_BG: Color32 = SURFACE;

// Numeric rules, from ultracrop.py lines 66–73 and CropCanvas.__init__.
const UNDO_LIMIT: usize = 10;
const MAX_RECENT: usize = 8;
const MIN_BOX: f32 = 12.0;
const HANDLE_HALF: f32 = 7.0;
const HANDLE_HIT: f32 = HANDLE_HALF + 2.0; // _handle_at tolerance
const ZOOM_MIN: f32 = 0.05;
const ZOOM_MAX: f32 = 8.0;
const ZOOM_STEP: f32 = 1.12; // one wheel notch
const ZOOM_BUTTON_IN: f32 = 1.25;
const ZOOM_BUTTON_OUT: f32 = 0.8;
const PANEL_W: f32 = 290.0; // side panel inner width (ultracrop.py L68)
const SIDE_W: f32 = 320.0; // side panel + paddings, window maths (L69)
const PANEL_CHROME: f32 = 44.0; // 8 + 12 panel margins + 2 * 12 card padding
const PREVIEW_MAX_W: f32 = PANEL_W - 56.0; // _update_preview (234)
const PREVIEW_MAX_H: f32 = 180.0;
const CHROME_W: f32 = 40.0;
const CHROME_H: f32 = 175.0;
const MIN_WINDOW_W: f32 = 820.0;
const MIN_WINDOW_H: f32 = 540.0;
// Normal window: used when the image is too small to fill anything bigger.
const DEFAULT_WINDOW_W: f32 = 1180.0;
const DEFAULT_WINDOW_H: f32 = 780.0;
const FIT_ENFORCE_SECS: f64 = 4.0; // _fit_window_to_image late checks
const FIT_RETRY_SECS: f64 = 0.15;
const FIT_TOLERANCE: f32 = 3.0; // _schedule_enforce 3 px drift
const MAX_TEXTURE_SIDE: u32 = 8192;

const ASPECTS: [(&str, Option<f32>); 9] = [
    ("Vaba", None),
    ("1:1", Some(1.0)),
    ("16:9", Some(16.0 / 9.0)),
    ("9:16", Some(9.0 / 16.0)),
    ("4:3", Some(4.0 / 3.0)),
    ("3:4", Some(3.0 / 4.0)),
    ("3:2", Some(3.0 / 2.0)),
    ("2:3", Some(2.0 / 3.0)),
    ("5:4", Some(5.0 / 4.0)),
];

#[derive(Clone, Copy, PartialEq)]
enum DragKind {
    Move,
    Resize(usize),
    New,
}

#[derive(Clone, Copy)]
struct DragState {
    kind: DragKind,
    start_img: Pos2,
    orig_crop: Option<Rect>,
}

#[derive(Clone, Copy)]
struct WindowFit {
    target: Option<Vec2>, // None means fullscreen
    started: f64,
    deadline: f64,
    last_request: f64,
}

struct CropApp {
    img: Option<Arc<DynamicImage>>,
    tex: Option<TextureHandle>,
    path: Option<PathBuf>,
    file_size: u64,

    crop: Option<Rect>,
    aspect: Option<f32>,
    aspect_name: String,
    locked_ratio: Option<f32>,
    lock: bool,

    show_thirds: bool,
    show_grid: bool,

    zoom: f32, // multiplier on top of "fit to window"
    pan: Vec2,
    last_fit: f32,
    last_scale: f32,

    undo: Vec<Arc<DynamicImage>>,
    redo: Vec<Arc<DynamicImage>>,

    resize_w: u32,
    resize_h: u32,
    link_resize: bool,

    recent: Vec<PathBuf>,
    status: String,

    drag: Option<DragState>,
    cursor_img: Option<Pos2>,
    last_title: String,
    fullscreen: bool,
    pending_fit: bool,
    fit: Option<WindowFit>,

    screenshot_path: Option<PathBuf>,
    screenshot_frame: u32,
    screenshot_requested: bool,
    screenshot_saved: bool,
}

impl CropApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_style(&cc.egui_ctx);

        let mut input: Option<PathBuf> = None;
        let mut screenshot: Option<PathBuf> = None;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            if arg == "--screenshot" {
                screenshot = args.next().map(PathBuf::from);
            } else if !arg.starts_with("--") {
                input = Some(PathBuf::from(arg));
            }
        }

        let mut app = Self {
            img: None,
            tex: None,
            path: None,
            file_size: 0,
            crop: None,
            aspect: None,
            aspect_name: "Vaba".into(),
            locked_ratio: None,
            lock: false,
            show_thirds: false,
            show_grid: false,
            zoom: 1.0,
            pan: Vec2::ZERO,
            last_fit: 1.0,
            last_scale: 1.0,
            undo: Vec::new(),
            redo: Vec::new(),
            resize_w: 0,
            resize_h: 0,
            link_resize: true,
            recent: load_recent(),
            status: "Valmis — ava pilt alustamiseks".into(),
            drag: None,
            cursor_img: None,
            last_title: String::new(),
            fullscreen: false,
            pending_fit: false,
            fit: None,
            screenshot_path: screenshot,
            screenshot_frame: 0,
            screenshot_requested: false,
            screenshot_saved: false,
        };

        if let Some(p) = input {
            if p.is_file() {
                app.open_path(&cc.egui_ctx, p);
            }
        }
        app
    }

    // ── Image plumbing ────────────────────────────────────────────────
    fn set_image(&mut self, ctx: &egui::Context, img: Arc<DynamicImage>, reset_crop: bool) {
        self.img = Some(img);
        self.upload_texture(ctx);
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
        // CropCanvas.set_image(keep_box=False): every image-changing action
        // (open, crop, rotate, flip, resize, undo, redo) starts without a box.
        if reset_crop {
            self.crop = None;
        } else if let Some(c) = self.crop {
            self.crop = Some(fit_inside(c, self.image_bounds()));
        }
        self.sync_resize_fields();
    }

    fn upload_texture(&mut self, ctx: &egui::Context) {
        let Some(img) = self.img.clone() else {
            self.tex = None;
            return;
        };
        let (w, h) = img.dimensions();
        let rgba = if w > MAX_TEXTURE_SIDE || h > MAX_TEXTURE_SIDE {
            img.resize(MAX_TEXTURE_SIDE, MAX_TEXTURE_SIDE, image::imageops::FilterType::Triangle)
                .to_rgba8()
        } else {
            img.to_rgba8()
        };
        let (tw, th) = rgba.dimensions();
        let color_image =
            egui::ColorImage::from_rgba_unmultiplied([tw as usize, th as usize], rgba.as_raw());
        self.tex = Some(ctx.load_texture(
            "ultracrop-image",
            color_image,
            TextureOptions::LINEAR,
        ));
    }

    fn sync_resize_fields(&mut self) {
        if let Some(img) = &self.img {
            let (w, h) = img.dimensions();
            self.resize_w = w;
            self.resize_h = h;
        }
    }

    fn set_status(&mut self, text: impl Into<String>) {
        self.status = text.into();
    }

    // Window sizing on open (UltraCrop._compute_fit_size, L758–766):
    //  • image too small for a big window → the app's normal default size;
    //  • image that fits         → the image plus its chrome, never below the
    //                              default size, so a tiny image does not
    //                              produce a tiny window;
    //  • image bigger than the screen → fullscreen, where the canvas fit
    //                              scale shows the whole image inside the app.
    fn fit_window_to_image(&mut self, ctx: &egui::Context) {
        let Some(img) = &self.img else {
            return;
        };
        let (iw, ih) = img.dimensions();
        let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) else {
            // An image passed on the command line opens before the window
            // exists, so the screen size is not known yet. Retry on the first
            // frame that has it.
            self.pending_fit = true;
            ctx.request_repaint();
            return;
        };
        let (sw, sh) = (monitor.x, monitor.y);
        self.pending_fit = false;
        let need_w = iw as f32 + SIDE_W + CHROME_W;
        let need_h = ih as f32 + CHROME_H;
        let now = ctx.input(|i| i.time);
        let target = if need_w > sw || need_h > sh {
            None
        } else {
            Some(Vec2::new(
                need_w.max(DEFAULT_WINDOW_W).min(sw),
                need_h.max(DEFAULT_WINDOW_H).min(sh),
            ))
        };
        self.fit = Some(WindowFit {
            target,
            started: now,
            deadline: now + FIT_ENFORCE_SECS,
            last_request: f64::NEG_INFINITY,
        });
        // KWin can override the first request with its own placement or with
        // a remembered/maximized state, so re-apply it for a short while.
        self.enforce_window_fit(ctx);
    }

    /// Keep asking for the size computed by `fit_window_to_image` until KWin
    /// agrees, the user starts dragging, or the grace period runs out. The
    /// Python original used a 4 s enforcement loop for exactly this reason;
    /// a single `InnerSize` request can be swallowed at map time.
    fn enforce_window_fit(&mut self, ctx: &egui::Context) {
        let Some(fit) = self.fit else {
            return;
        };
        let now = ctx.input(|i| i.time);
        // Ignore the launch click: a menu action may still hold the button
        // down when the first frame runs. After that, a drag means the user
        // is resizing the window and should win.
        let dragging = now > fit.started + 0.3 && ctx.input(|i| i.pointer.any_down());
        if dragging || now > fit.deadline {
            self.fit = None;
            return;
        }

        let vp = ctx.input(|i| i.viewport().clone());
        match fit.target {
            None => {
                if vp.fullscreen != Some(true) && now - fit.last_request >= FIT_RETRY_SECS {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                    self.fullscreen = true;
                    if let Some(f) = &mut self.fit {
                        f.last_request = now;
                    }
                }
            }
            Some(target) => {
                if vp.fullscreen == Some(true) || self.fullscreen {
                    // Leave a previous image's fullscreen before sizing.
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                    self.fullscreen = false;
                    if let Some(f) = &mut self.fit {
                        f.last_request = now;
                    }
                } else {
                    if vp.maximized == Some(true) {
                        // A maximized window ignores InnerSize.
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                    }
                    let reached = vp.inner_rect.is_some_and(|r| {
                        (r.width() - target.x).abs() <= FIT_TOLERANCE
                            && (r.height() - target.y).abs() <= FIT_TOLERANCE
                    });
                    // Do not stop at the first success: KWin applies its
                    // remembered/maximized geometry a moment later and can
                    // undo the resize. Keep watching until the grace period
                    // ends, and only re-send when the size has drifted.
                    if !reached && now - fit.last_request >= FIT_RETRY_SECS {
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target));
                        if let Some(f) = &mut self.fit {
                            f.last_request = now;
                        }
                    }
                }
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(120));
    }

    fn open_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        // UltraCrop._add_recent L801: recents are stored with
        // os.path.abspath(), so a file opened by a relative CLI argument must
        // still land in the list (and in the title) as an absolute path.
        let path = std::path::absolute(&path).unwrap_or(path);
        match image::open(&path) {
            Ok(img) => {
                // UltraCrop._open_file L1457-1460: normalise every image to
                // RGB or RGBA ("A" in mode → RGBA) before it is shown.
                let img = normalise_image(img);
                self.file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                self.path = Some(path.clone());
                self.undo.clear();
                self.redo.clear();
                self.tex = None;
                self.set_image(ctx, Arc::new(img), true);
                self.add_recent(path.clone());
                let name = file_name(&path);
                self.set_status(format!("Avatud: {name}"));
                self.last_title = format!("UltraCrop – {name}");
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.last_title.clone()));
                self.fit_window_to_image(ctx);
            }
            Err(err) => {
                self.set_status(format!("Pildi avamine ebaõnnestus: {err}"));
            }
        }
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        let mut dlg = rfd::FileDialog::new()
            .set_title("Vali pilt")
            .add_filter(
                "Pildid",
                &["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp", "ico"],
            );
        if let Some(dir) = self
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map(Path::to_path_buf)
        {
            dlg = dlg.set_directory(dir);
        } else if let Some(home) = home_dir() {
            dlg = dlg.set_directory(home);
        }
        if let Some(path) = dlg.pick_file() {
            self.open_path(ctx, path);
        }
    }

    fn save_dialog(&mut self) {
        let Some(img) = self.img.clone() else {
            self.set_status("Kõigepealt ava pilt!");
            return;
        };
        let out = self.cropped_or_full(&img);
        let ext = self
            .path
            .as_ref()
            .and_then(|p| p.extension())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "png".into());

        let mut dlg = rfd::FileDialog::new()
            .set_title("Salvesta pilt")
            // UltraCrop.save_crop: initialfile=f"karpitud{ext}"
            .set_file_name(format!("karpitud.{ext}"))
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("BMP", &["bmp"])
            .add_filter("TIFF", &["tiff", "tif"])
            .add_filter("WebP", &["webp"])
            // UltraCrop.save_crop also offers "Kõik" (*.*).
            .add_filter("Kõik", &["*"]);
        if let Some(dir) = self.path.as_ref().and_then(|p| p.parent()) {
            dlg = dlg.set_directory(dir);
        } else if let Some(home) = home_dir() {
            dlg = dlg.set_directory(home);
        }
        if let Some(path) = dlg.save_file() {
            self.write_image(&out, &path);
        }
    }

    fn quick_save(&mut self) {
        let Some(img) = self.img.clone() else {
            self.set_status("Pilt pole avatud.");
            return;
        };
        let Some(src) = self.path.clone() else {
            self.set_status("Pilt pole avatud.");
            return;
        };
        let out = self.cropped_or_full(&img);
        let base = src
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "pilt".into());
        let dir = src.parent().map(Path::to_path_buf).unwrap_or_default();
        let ext = src
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_else(|| "png".into());
        let path = quick_save_path(&dir, &base, &ext);
        self.write_image(&out, &path);
    }

    fn write_image(&mut self, img: &DynamicImage, path: &Path) {
        let fmt = ImageFormat::from_path(path).unwrap_or(ImageFormat::Png);
        let result = if matches!(fmt, ImageFormat::Jpeg) {
            img.to_rgb8().save_with_format(path, fmt)
        } else {
            img.save_with_format(path, fmt)
        };
        match result {
            Ok(_) => self.set_status(format!(
                "Salvestatud: {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
            Err(err) => self.set_status(format!("Salvestamine ebaõnnestus: {err}")),
        }
    }

    fn cropped_or_full(&self, img: &DynamicImage) -> DynamicImage {
        match self.crop_pixels(img) {
            Some((x, y, w, h)) => img.crop_imm(x, y, w, h),
            None => img.clone(),
        }
    }

    fn crop_pixels(&self, img: &DynamicImage) -> Option<(u32, u32, u32, u32)> {
        let crop = normalize(self.crop?);
        let (iw, ih) = img.dimensions();
        crop_box_pixels(crop, iw, ih)
    }

    fn apply_crop(&mut self, ctx: &egui::Context) {
        let Some(img) = self.img.clone() else {
            return;
        };
        let Some((x, y, w, h)) = self.crop_pixels(&img) else {
            self.set_status("Kõigepealt joonista kärpimisraam");
            return;
        };
        self.push_undo();
        let cropped = Arc::new(img.crop_imm(x, y, w, h));
        self.set_image(ctx, cropped, true);
        self.set_status(format!("Kärbitud → {w} × {h} px"));
    }

    fn rotate(&mut self, ctx: &egui::Context, degrees: i32) {
        let Some(img) = self.img.clone() else {
            return;
        };
        self.push_undo();
        let out = if degrees > 0 {
            img.rotate90()
        } else {
            img.rotate270()
        };
        self.set_image(ctx, Arc::new(out), true);
        self.set_status(if degrees > 0 {
            "Pööratud 90°"
        } else {
            "Pööratud -90°"
        });
    }

    fn flip(&mut self, ctx: &egui::Context, horizontal: bool) {
        let Some(img) = self.img.clone() else {
            return;
        };
        self.push_undo();
        let out = if horizontal {
            img.fliph()
        } else {
            img.flipv()
        };
        self.set_image(ctx, Arc::new(out), true);
        self.set_status(if horizontal {
            "Peegeldata horisontaalselt"
        } else {
            "Peegeldata vertikaalselt"
        });
    }

    fn apply_resize(&mut self, ctx: &egui::Context) {
        let Some(img) = self.img.clone() else {
            self.set_status("Pilt pole avatud");
            return;
        };
        let nw = self.resize_w.max(1);
        let nh = self.resize_h.max(1);
        let (iw, ih) = img.dimensions();
        if nw == iw && nh == ih {
            self.set_status("Suurus on juba sama");
            return;
        }
        self.push_undo();
        // PIL Image.resize keeps the source mode; resize_exact does the same.
        let resized = img.resize_exact(nw, nh, image::imageops::FilterType::Lanczos3);
        self.set_image(ctx, Arc::new(resized), true);
        self.set_status(format!("Suurust muudetud → {nw} × {nh} px"));
    }

    // ── Undo / redo ───────────────────────────────────────────────────
    fn push_undo(&mut self) {
        if let Some(img) = &self.img {
            self.undo.push(img.clone());
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn undo(&mut self, ctx: &egui::Context) {
        let Some(prev) = self.undo.pop() else {
            self.set_status("Tagasivõtmise ajalugu on tühi");
            return;
        };
        if let Some(cur) = self.img.clone() {
            self.redo.push(cur);
        }
        self.set_image(ctx, prev, true);
        self.set_status("Tagasi võetud");
    }

    fn redo(&mut self, ctx: &egui::Context) {
        let Some(next) = self.redo.pop() else {
            self.set_status("Uuestitegemise ajalugu on tühi");
            return;
        };
        if let Some(cur) = self.img.clone() {
            self.undo.push(cur);
        }
        self.set_image(ctx, next, true);
        self.set_status("Uuesti tehtud");
    }

    // ── Crop helpers ──────────────────────────────────────────────────
    fn image_bounds(&self) -> Rect {
        match &self.img {
            Some(img) => {
                let (w, h) = img.dimensions();
                Rect::from_min_max(Pos2::ZERO, Pos2::new(w as f32, h as f32))
            }
            None => Rect::from_min_size(Pos2::ZERO, Vec2::splat(1.0)),
        }
    }

    fn set_aspect(&mut self, name: &str, ratio: Option<f32>) {
        // CropCanvas.set_aspect only records the constraint for the next
        // draw/resize; it never reshapes the box that is already there.
        self.aspect = ratio;
        self.aspect_name = name.to_string();
    }

    fn toggle_lock(&mut self) {
        if self.lock {
            let r = self
                .crop
                .map(normalize)
                .filter(|c| c.width() >= 1.0 && c.height() >= 1.0)
                .map(|c| c.width() / c.height().max(0.001))
                .or_else(|| {
                    self.img.as_ref().map(|img| {
                        let (w, h) = img.dimensions();
                        w as f32 / h.max(1) as f32
                    })
                });
            if let Some(r) = r.filter(|r| *r > 0.0) {
                self.locked_ratio = Some(r);
                self.set_status(format!("Kuvasuhe lukustatud ({r:.3})"));
            } else {
                self.lock = false;
                self.locked_ratio = None;
                self.set_status("Pole pilti — pole midagi lukustada");
            }
        } else {
            self.locked_ratio = None;
            self.set_status("Kuvasuhe vabastatud");
        }
    }

    fn effective_ratio(&mut self, shift: bool) -> Option<f32> {
        if shift {
            Some(1.0)
        } else {
            self.aspect.or(self.locked_ratio)
        }
    }

    fn clear_crop(&mut self) {
        self.crop = None;
        self.set_status("Raam tühjendatud");
    }

    fn select_all(&mut self) {
        if self.img.is_some() {
            self.crop = Some(self.image_bounds());
            self.set_status("Kogu pilt valitud");
        }
    }

    fn nudge(&mut self, dx: f32, dy: f32) {
        // CropCanvas.nudge: translate only; the box may leave the image and
        // get_crop_box clamps it when it is used.
        if let Some(c) = self.crop {
            self.crop = Some(c.translate(Vec2::new(dx, dy)));
        }
    }

    fn copy_clipboard(&mut self) {
        let Some(img) = self.img.clone() else {
            return;
        };
        let out = self.cropped_or_full(&img).to_rgba8();
        let (w, h) = out.dimensions();
        match arboard::Clipboard::new().and_then(|mut cb| {
            cb.set_image(arboard::ImageData {
                width: w as usize,
                height: h as usize,
                bytes: out.into_raw().into(),
            })
        }) {
            Ok(_) => self.set_status("Kopeeritud lõikelauale"),
            Err(err) => self.set_status(format!("Kopeerimine ebaõnnestus: {err}")),
        }
    }

    /// New feature, not in the Python original: write the current crop to a
    /// temporary PNG and let PhotoCraft's CLI open it as a tab in the running
    /// app. The button is only live while a crop box exists.
    fn open_in_photocraft(&mut self) {
        let Some(img) = self.img.clone() else {
            self.set_status("Kõigepealt ava pilt!");
            return;
        };
        let Some((x, y, w, h)) = self.crop_pixels(&img) else {
            self.set_status("Kõigepealt joonista kärpimisraam");
            return;
        };
        let stem = self
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "pilt".into());
        let crop = img.crop_imm(x, y, w, h);
        let path = match write_photocraft_export(&crop, &stem) {
            Ok(path) => path,
            Err(err) => {
                self.set_status(format!("PhotoCrafti avamine ebaõnnestus: {err}"));
                return;
            }
        };
        match std::process::Command::new("photocraft").arg(&path).spawn() {
            Ok(mut child) => {
                // Reap the launcher so it does not linger as a zombie; the
                // temporary PNG stays put for PhotoCraft to read.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                self.set_status("Kärbe avatud PhotoCraftis");
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.set_status("PhotoCrafti ei leitud — kas see on paigaldatud?")
            }
            Err(err) => self.set_status(format!("PhotoCrafti avamine ebaõnnestus: {err}")),
        }
    }

    // ── Recent files ─────────────────────────────────────────────────
    fn add_recent(&mut self, path: PathBuf) {
        self.recent.retain(|p| p != &path);
        self.recent.insert(0, path);
        self.recent.truncate(MAX_RECENT);
        save_recent(&self.recent);
    }

    // ── Global shortcuts ─────────────────────────────────────────────
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let cmd = ctx.input(|i| i.modifiers.command);
        let shift = ctx.input(|i| i.modifiers.shift);

        if cmd {
            if ctx.input(|i| i.key_pressed(egui::Key::O)) {
                self.open_dialog(ctx);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::S)) {
                if shift {
                    self.quick_save();
                } else {
                    self.save_dialog();
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Z)) && !shift {
                self.undo(ctx);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Y)) || (shift && ctx.input(|i| i.key_pressed(egui::Key::Z))) {
                self.redo(ctx);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::X)) {
                self.apply_crop(ctx);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::C)) {
                self.copy_clipboard();
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Num0)) {
                self.zoom = 1.0;
                self.pan = Vec2::ZERO;
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Plus) || i.key_pressed(egui::Key::Equals)) {
                self.zoom = self.clamp_zoom(self.zoom * ZOOM_BUTTON_IN);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Minus)) {
                self.zoom = self.clamp_zoom(self.zoom * ZOOM_BUTTON_OUT);
            }
        } else {
            if ctx.input(|i| i.key_pressed(egui::Key::R)) {
                self.rotate(ctx, if shift { -90 } else { 90 });
            }
            if ctx.input(|i| i.key_pressed(egui::Key::H)) {
                self.flip(ctx, true);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::V)) {
                self.flip(ctx, false);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.clear_crop();
            }
            // F11: leave or re-enter fullscreen by hand. A big image opens
            // fullscreen automatically; this is the way back to a window.
            if ctx.input(|i| i.key_pressed(egui::Key::F11)) {
                // The user is in charge from here on.
                self.fit = None;
                let on = !self.fullscreen;
                self.fullscreen = on;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
                if !on {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
                        DEFAULT_WINDOW_W,
                        DEFAULT_WINDOW_H,
                    )));
                }
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                self.nudge(if shift { -10.0 } else { -1.0 }, 0.0);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                self.nudge(if shift { 10.0 } else { 1.0 }, 0.0);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
                self.nudge(0.0, if shift { -10.0 } else { -1.0 });
            }
            if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
                self.nudge(0.0, if shift { 10.0 } else { 1.0 });
            }
        }
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.screenshot_path.clone() else {
            return;
        };
        if self.screenshot_saved {
            return;
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            self.screenshot_saved = true;
            match save_screenshot(&image, &path) {
                Ok(_) => self.set_status(format!("Screenshot: {}", path.display())),
                Err(err) => self.set_status(format!("Screenshot failed: {err}")),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.screenshot_frame < 120 {
            self.screenshot_frame += 1;
        }
        if self.screenshot_frame >= 120 && !self.screenshot_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
            self.screenshot_requested = true;
        }
        ctx.request_repaint();
    }

    fn handle_dropped(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        if let Some(path) = dropped.into_iter().find(|p| is_image(p)) {
            self.open_path(ctx, path);
        } else {
            self.set_status("See fail ei ole pilt");
        }
    }

    // ── Canvas geometry ──────────────────────────────────────────────
    fn fit_scale(&self, canvas: Rect) -> f32 {
        let Some(img) = &self.img else {
            return 0.0;
        };
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return 0.0;
        }
        (canvas.width() / w as f32)
            .min(canvas.height() / h as f32)
            .min(1.0)
            .max(0.0001)
    }

    // `zoom` is a multiplier on the fit scale, while the original clamps the
    // absolute scale to 0.05…8.0; convert before clamping.
    fn clamp_zoom(&self, z: f32) -> f32 {
        let fit = if self.last_fit > 0.0001 {
            self.last_fit
        } else {
            1.0
        };
        z.clamp(ZOOM_MIN / fit, ZOOM_MAX / fit)
    }

    fn image_rect(&self, canvas: Rect) -> Rect {
        let Some(img) = &self.img else {
            return Rect::NOTHING;
        };
        let (w, h) = img.dimensions();
        let scale = self.fit_scale(canvas) * self.zoom;
        Rect::from_center_size(
            canvas.center() + self.pan,
            Vec2::new(w as f32 * scale, h as f32 * scale),
        )
    }

    fn to_screen(&self, p: Pos2, img_rect: Rect, iw: f32, ih: f32) -> Pos2 {
        img_rect.min
            + Vec2::new(
                p.x / iw.max(1.0) * img_rect.width(),
                p.y / ih.max(1.0) * img_rect.height(),
            )
    }

    fn to_image(&self, p: Pos2, img_rect: Rect, iw: f32, ih: f32) -> Pos2 {
        let x = (p.x - img_rect.min.x) / img_rect.width().max(0.0001) * iw;
        let y = (p.y - img_rect.min.y) / img_rect.height().max(0.0001) * ih;
        Pos2::new(x.clamp(0.0, iw), y.clamp(0.0, ih))
    }

    fn crop_screen(&self, img_rect: Rect, iw: f32, ih: f32) -> Option<Rect> {
        let c = self.crop?;
        let a = self.to_screen(c.min, img_rect, iw, ih);
        let b = self.to_screen(c.max, img_rect, iw, ih);
        Some(Rect::from_two_pos(a, b))
    }

    fn handle_points(&self, r: Rect) -> [Pos2; 8] {
        let c = r.center();
        [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
            Pos2::new(c.x, r.top()),
            Pos2::new(c.x, r.bottom()),
            Pos2::new(r.left(), c.y),
            Pos2::new(r.right(), c.y),
        ]
    }

    fn handle_at(&self, p: Pos2, r: Rect) -> Option<usize> {
        // CropCanvas._handle_at: square test, half-size 7 + 2 px of slop.
        self.handle_points(r)
            .iter()
            .position(|h| (p.x - h.x).abs() <= HANDLE_HIT && (p.y - h.y).abs() <= HANDLE_HIT)
    }

    fn cursor_for_handle(idx: usize) -> CursorIcon {
        match idx {
            0 | 2 => CursorIcon::ResizeNwSe,
            1 | 3 => CursorIcon::ResizeNeSw,
            4 | 5 => CursorIcon::ResizeVertical,
            _ => CursorIcon::ResizeHorizontal,
        }
    }

    // ── Canvas interaction ───────────────────────────────────────────
    fn interact_canvas(
        &mut self,
        ui: &mut Ui,
        resp: &Response,
        canvas: Rect,
        img_rect: Rect,
        iw: f32,
        ih: f32,
    ) {
        let pointer = ui.input(|i| i.pointer.latest_pos());
        let hover = resp.hovered();

        // Scroll to zoom, anchored under the cursor.
        if hover {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.01 {
                let old_zoom = self.zoom;
                // CropCanvas._on_wheel: one notch is exactly ×1.12 (or ÷1.12).
                let factor = if scroll > 0.0 { ZOOM_STEP } else { 1.0 / ZOOM_STEP };
                self.zoom = self.clamp_zoom(self.zoom * factor);
                if let Some(p) = pointer {
                    let old_scale = self.fit_scale(canvas) * old_zoom;
                    let new_scale = self.fit_scale(canvas) * self.zoom;
                    let center = canvas.center() + self.pan;
                    let img_pt = Pos2::new(
                        (p.x - center.x) / old_scale.max(0.0001),
                        (p.y - center.y) / old_scale.max(0.0001),
                    );
                    self.pan = Vec2::new(
                        p.x - canvas.center().x - img_pt.x * new_scale,
                        p.y - canvas.center().y - img_pt.y * new_scale,
                    );
                }
            }
        }

        // Middle / right drag pans the picture.
        if resp.dragged_by(PointerButton::Middle) || resp.dragged_by(PointerButton::Secondary) {
            self.pan += resp.drag_delta();
        }

        let bounds = self.image_bounds();
        let shift = ui.input(|i| i.modifiers.shift);

        if hover {
            if let Some(p) = pointer {
                self.cursor_img = Some(self.to_image(p, img_rect, iw, ih));
            }
        } else {
            self.cursor_img = None;
        }

        // Start a drag.
        if hover && ui.input(|i| i.pointer.primary_pressed()) {
            if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                let ip = self.to_image(p, img_rect, iw, ih);
                if let Some(cr) = self.crop_screen(img_rect, iw, ih) {
                    if let Some(idx) = self.handle_at(p, cr) {
                        self.drag = Some(DragState {
                            kind: DragKind::Resize(idx),
                            start_img: ip,
                            orig_crop: self.crop,
                        });
                    } else if cr.contains(p) {
                        self.drag = Some(DragState {
                            kind: DragKind::Move,
                            start_img: ip,
                            orig_crop: self.crop,
                        });
                    } else {
                        self.drag = Some(DragState {
                            kind: DragKind::New,
                            start_img: ip,
                            orig_crop: self.crop,
                        });
                        self.crop = Some(Rect::from_min_size(ip, Vec2::splat(1.0)));
                    }
                } else {
                    self.drag = Some(DragState {
                        kind: DragKind::New,
                        start_img: ip,
                        orig_crop: None,
                    });
                    self.crop = Some(Rect::from_min_size(ip, Vec2::splat(1.0)));
                }
            }
        }

        // Continue a drag.
        if let Some(drag) = self.drag {
            if ui.input(|i| i.pointer.primary_down()) {
                if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                    let ip = self.to_image(p, img_rect, iw, ih);
                    let ratio = self.effective_ratio(shift);
                    match drag.kind {
                        DragKind::New => {
                            let start = drag.start_img;
                            self.crop = Some(drag_rect(start, ip, ratio, bounds));
                        }
                        DragKind::Move => {
                            if let Some(orig) = drag.orig_crop {
                                let delta = ip - drag.start_img;
                                self.crop = Some(fit_inside(orig.translate(delta), bounds));
                            }
                        }
                        DragKind::Resize(idx) => {
                            if let Some(orig) = drag.orig_crop {
                                self.crop =
                                    Some(resize_rect(orig, idx, ip, ratio, bounds));
                            }
                        }
                    }
                }
            } else {
                // Released.
                if let Some(c) = self.crop {
                    let c = normalize(c);
                    if c.width() < MIN_BOX || c.height() < MIN_BOX {
                        self.crop = None;
                    } else {
                        self.crop = Some(fit_inside(c, bounds));
                    }
                }
                self.drag = None;
            }
        }

        // Hover cursor.
        if self.drag.is_none() && hover {
            if let (Some(p), Some(cr)) = (pointer, self.crop_screen(img_rect, iw, ih)) {
                if let Some(idx) = self.handle_at(p, cr) {
                    ui.ctx().set_cursor_icon(Self::cursor_for_handle(idx));
                } else if cr.contains(p) {
                    ui.ctx().set_cursor_icon(CursorIcon::Grab);
                }
            }
        }
    }

    // ── Canvas painting ──────────────────────────────────────────────
    fn paint_canvas(&mut self, ui: &mut Ui, resp: &Response, painter: &egui::Painter) {
        let canvas = resp.rect;
        self.last_fit = self.fit_scale(canvas);
        self.last_scale = self.last_fit * self.zoom;
        painter.rect_filled(canvas, Rounding::ZERO, CANVAS_BG);

        // Subtle dot grid so the empty canvas is not a flat void.
        let step = 28.0;
        let dot = Color32::from_rgb(0x16, 0x1a, 0x28);
        let mut x = canvas.left();
        while x < canvas.right() {
            let mut y = canvas.top();
            while y < canvas.bottom() {
                painter.circle_filled(Pos2::new(x, y), 0.8, dot);
                y += step;
            }
            x += step;
        }

        let Some(img) = self.img.clone() else {
            // CropCanvas.redraw L432-434: exactly one centred line.
            painter.text(
                canvas.center(),
                Align2::CENTER_CENTER,
                "Lohista fail siia  ·  Ctrl+O",
                FontId::proportional(14.0),
                TEXT_DIM,
            );
            return;
        };

        let (iw_u, ih_u) = img.dimensions();
        let iw = iw_u as f32;
        let ih = ih_u as f32;
        let img_rect = self.image_rect(canvas);

        // Shadow + checkerboard + image.
        painter.rect_filled(
            img_rect.translate(Vec2::new(8.0, 10.0)),
            Rounding::same(3.0),
            Color32::from_black_alpha(110),
        );
        paint_checkerboard(painter, img_rect);
        if let Some(tex) = &self.tex {
            painter.image(
                tex.id(),
                img_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        painter.rect_stroke(
            img_rect,
            Rounding::ZERO,
            Stroke::new(1.0_f32, Color32::from_white_alpha(26)),
        );

        self.interact_canvas(ui, resp, canvas, img_rect, iw, ih);

        let Some(crop) = self.crop else {
            return;
        };
        let crop = normalize(crop);
        let cr = self.crop_screen(img_rect, iw, ih).unwrap_or(Rect::NOTHING);
        if !cr.is_positive() {
            return;
        }

        // CropCanvas._draw_overlay: the black mask covers the whole canvas.
        // "gray25" stipple is ~25 % coverage, approximated as alpha 64.
        let dim = Color32::from_black_alpha(64);
        let top = Rect::from_min_max(canvas.min, Pos2::new(canvas.max.x, cr.min.y));
        let bottom = Rect::from_min_max(Pos2::new(canvas.min.x, cr.max.y), canvas.max);
        let left = Rect::from_min_max(
            Pos2::new(canvas.min.x, cr.min.y),
            Pos2::new(cr.min.x, cr.max.y),
        );
        let right = Rect::from_min_max(
            Pos2::new(cr.max.x, cr.min.y),
            Pos2::new(canvas.max.x, cr.max.y),
        );
        for r in [top, bottom, left, right] {
            if r.is_positive() {
                painter.rect_filled(r, Rounding::ZERO, dim);
            }
        }

        // CropCanvas._draw_box: accent 8/4 dashes over white 4/8 dashes.
        dashed_rect_phase(painter, cr, 8.0, 4.0, 0.0, Stroke::new(2.0_f32, ACCENT));
        dashed_rect_phase(
            painter,
            cr,
            4.0,
            8.0,
            6.0,
            Stroke::new(1.0_f32, Color32::WHITE),
        );

        // Rule-of-thirds / grid inside the crop.
        if self.show_thirds {
            let s = Stroke::new(1.0_f32, Color32::from_white_alpha(128));
            for i in 1..3 {
                let t = i as f32 / 3.0;
                let x = cr.left() + cr.width() * t;
                let y = cr.top() + cr.height() * t;
                dashed_line_phase(
                    painter,
                    Pos2::new(x, cr.top()),
                    Pos2::new(x, cr.bottom()),
                    4.0,
                    4.0,
                    0.0,
                    s,
                );
                dashed_line_phase(
                    painter,
                    Pos2::new(cr.left(), y),
                    Pos2::new(cr.right(), y),
                    4.0,
                    4.0,
                    0.0,
                    s,
                );
            }
        }
        if self.show_grid {
            let s = Stroke::new(1.0_f32, Color32::from_rgb(0x33, 0x41, 0x55));
            for i in 1..6 {
                let t = i as f32 / 6.0;
                let x = cr.left() + cr.width() * t;
                let y = cr.top() + cr.height() * t;
                painter.line_segment([Pos2::new(x, cr.top()), Pos2::new(x, cr.bottom())], s);
                painter.line_segment([Pos2::new(cr.left(), y), Pos2::new(cr.right(), y)], s);
            }
        }

        // Handles: filled accent, white rim, radius 7 (HANDLE_HALF).
        for h in self.handle_points(cr) {
            painter.circle_filled(h, HANDLE_HALF, ACCENT);
            painter.circle_stroke(h, HANDLE_HALF, Stroke::new(1.5_f32, Color32::WHITE));
        }

        // Size badge; the original truncates the box size like int().
        let bw = crop.width().trunc() as i64;
        let bh = crop.height().trunc() as i64;
        let label = format!(" {bw} × {bh} px ");
        let galley = painter.layout_no_wrap(label, FontId::proportional(11.5), Color32::WHITE);
        let mut pos = Pos2::new(cr.left(), cr.top() - 10.0);
        if pos.y - galley.size().y < canvas.top() + 4.0 {
            pos.y = cr.top() + 10.0 + galley.size().y;
        }
        let bg_rect = Rect::from_min_size(
            Pos2::new(pos.x - 6.0, pos.y - galley.size().y - 4.0),
            galley.size() + Vec2::new(12.0, 8.0),
        )
        .intersect(canvas);
        painter.rect_filled(bg_rect, Rounding::same(5.0), Color32::from_rgba_unmultiplied(10, 12, 20, 225));
        painter.galley(
            Pos2::new(bg_rect.left() + 6.0, bg_rect.top() + 4.0),
            galley,
            Color32::WHITE,
        );

        // Drop hint on top of everything.
        let hovering_files = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        if hovering_files {
            painter.rect_filled(canvas, Rounding::ZERO, Color32::from_rgba_unmultiplied(10, 12, 20, 210));
            dashed_rect(painter, canvas.shrink(24.0), 12.0, 8.0, Stroke::new(2.0_f32, ACCENT));
            painter.text(
                canvas.center(),
                Align2::CENTER_CENTER,
                "Tõsta pilt siia",
                FontId::proportional(22.0),
                TEXT,
            );
        }
    }

    // ── Toolbar ──────────────────────────────────────────────────────
    fn toolbar(&mut self, ctx: &egui::Context) {
        let has_img = self.img.is_some();
        let has_crop = self
            .img
            .as_ref()
            .map_or(false, |img| self.crop_pixels(img).is_some());
        egui::TopBottomPanel::top("toolbar")
            .frame(
                egui::Frame::none()
                    .fill(SURFACE)
                    .inner_margin(Margin {
                        left: 14.0,
                        right: 14.0,
                        top: 9.0,
                        bottom: 9.0,
                    }),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("UltraCrop")
                            .size(20.0)
                            .strong()
                            .color(TEXT),
                    );
                    let pill = egui::Frame::none()
                        .fill(Color32::from_rgb(0x24, 0x26, 0x52))
                        .rounding(Rounding::same(6.0))
                        .inner_margin(Margin {
                            left: 6.0,
                            right: 6.0,
                            top: 2.0,
                            bottom: 2.0,
                        });
                    pill.show(ui, |ui| {
                        ui.label(
                            RichText::new("RUST")
                                .size(9.5)
                                .strong()
                                .color(ACCENT_HOVER),
                        );
                    });
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("kiire · täpne · ilus")
                            .size(11.0)
                            .color(TEXT_DIM),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let name = self
                            .path
                            .as_ref()
                            .map(|p| file_name(p))
                            .unwrap_or_else(|| "Pilt pole avatud".into());
                        ui.label(RichText::new(name).size(11.5).color(TEXT_DIM));
                        if has_img {
                            ui.label(RichText::new("•").size(11.0).color(BORDER));
                            let (w, h) = self.img.as_ref().unwrap().dimensions();
                            ui.label(
                                RichText::new(format!("{w} × {h}"))
                                    .size(11.0)
                                    .color(TEXT_DIM),
                            );
                        }
                    });
                });

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    if tool_button(ui, "Ava", SURFACE2, TEXT, true)
                        .on_hover_text("Ava pilt (Ctrl+O)")
                        .clicked()
                    {
                        self.open_dialog(ctx);
                    }
                    ui.add_space(4.0);
                    if tool_button(ui, "Kärbi", AMBER, Color32::BLACK, has_img)
                        .on_hover_text("Rakenda kärbe (Ctrl+X)")
                        .clicked()
                    {
                        self.apply_crop(ctx);
                    }
                    if tool_button(ui, "Tühjenda", SURFACE2, TEXT, has_img)
                        .on_hover_text("Tühjenda raam (Esc)")
                        .clicked()
                    {
                        self.clear_crop();
                    }
                    if tool_button(ui, "Kopeeri", SURFACE2, TEXT, has_img)
                        .on_hover_text("Kopeeri lõikelauale (Ctrl+C)")
                        .clicked()
                    {
                        self.copy_clipboard();
                    }
                    if tool_button(ui, "Ava PhotoCraftis", PINK, Color32::BLACK, has_crop)
                        .on_hover_text("Ava kärbe PhotoCraftis")
                        .on_disabled_hover_text("Joonista kõigepealt kärpimisraam")
                        .clicked()
                    {
                        self.open_in_photocraft();
                    }
                    ui.add_space(6.0);
                    if tool_button(ui, "Salvesta", GREEN, Color32::BLACK, has_img)
                        .on_hover_text("Salvesta nimega (Ctrl+S)")
                        .clicked()
                    {
                        self.save_dialog();
                    }
                    if tool_button(ui, "Kiire salvestus", CYAN, Color32::BLACK, has_img)
                        .on_hover_text("Salvesta kõrvale _karp failina (Ctrl+Shift+S)")
                        .clicked()
                    {
                        self.quick_save();
                    }
                    ui.add_space(6.0);
                    if tool_button(ui, "Tagasi", SURFACE2, TEXT, !self.undo.is_empty())
                        .on_hover_text("Võta tagasi (Ctrl+Z)")
                        .clicked()
                    {
                        self.undo(ctx);
                    }
                    if tool_button(ui, "Uuesti", SURFACE2, TEXT, !self.redo.is_empty())
                        .on_hover_text("Tee uuesti (Ctrl+Y)")
                        .clicked()
                    {
                        self.redo(ctx);
                    }
                });

                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if tool_button(ui, "Pööra +90", SURFACE3, TEXT, has_img).clicked() {
                        self.rotate(ctx, 90);
                    }
                    if tool_button(ui, "Pööra -90", SURFACE3, TEXT, has_img).clicked() {
                        self.rotate(ctx, -90);
                    }
                    if tool_button(ui, "Peegelda H", SURFACE3, TEXT, has_img).clicked() {
                        self.flip(ctx, true);
                    }
                    if tool_button(ui, "Peegelda V", SURFACE3, TEXT, has_img).clicked() {
                        self.flip(ctx, false);
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new("Suum").size(11.5).color(TEXT_DIM));
                    if tool_button(ui, "-", SURFACE2, TEXT, has_img).clicked() {
                        // CropCanvas.zoom_by(0.8): absolute 0.05…8.0 clamp.
                        self.zoom = self.clamp_zoom(self.zoom * ZOOM_BUTTON_OUT);
                    }
                    let pct = self.zoom_percent();
                    ui.label(
                        RichText::new(format!("{pct}%"))
                            .size(12.5)
                            .strong()
                            .color(TEXT),
                    );
                    if tool_button(ui, "+", SURFACE2, TEXT, has_img).clicked() {
                        // CropCanvas.zoom_by(1.25): absolute 0.05…8.0 clamp.
                        self.zoom = self.clamp_zoom(self.zoom * ZOOM_BUTTON_IN);
                    }
                    if tool_button(ui, "Sobita", SURFACE2, TEXT, has_img).clicked() {
                        self.zoom = 1.0;
                        self.pan = Vec2::ZERO;
                    }
                    if tool_button(ui, "100%", SURFACE2, TEXT, has_img).clicked() {
                        self.zoom = 1.0 / self.last_fit.max(0.0001);
                        self.pan = Vec2::ZERO;
                    }
                    ui.add_space(8.0);
                    ui.checkbox(&mut self.show_thirds, "Kolmandikud");
                    ui.checkbox(&mut self.show_grid, "Ruudustik");
                });
            });
    }

    fn zoom_percent(&self) -> i32 {
        if self.img.is_none() {
            return 100;
        }
        // UltraCrop._update_zoom_label+0x002C..0x006E:
        //   f"{int(self._canvas._zoom * 100)}%"
        // Python int() truncates; it never rounds, so 112.6 % reads "112%".
        zoom_label_percent(self.last_scale)
    }

    // ── Side panel ──────────────────────────────────────────────────
    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("side")
            .exact_width(PANEL_W + PANEL_CHROME)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(BG)
                    .inner_margin(Margin {
                        left: 8.0,
                        right: 12.0,
                        top: 10.0,
                        bottom: 10.0,
                    }),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.info_card(ui);
                        ui.add_space(9.0);
                        self.preview_card(ui);
                        ui.add_space(9.0);
                        self.aspect_card(ui);
                        ui.add_space(9.0);
                        self.view_card(ui);
                        ui.add_space(9.0);
                        self.resize_card(ui, ctx);
                        ui.add_space(9.0);
                        self.recent_card(ui, ctx);
                        ui.add_space(9.0);
                        self.shortcuts_card(ui);
                        ui.add_space(12.0);
                    });
            });
    }

    fn info_card(&mut self, ui: &mut Ui) {
        card(ui, "Teave", ACCENT, |ui| {
            let name = self
                .path
                .as_ref()
                .map(|p| file_name(p))
                .unwrap_or_else(|| "–".into());
            row(ui, "Fail", &name);
            if let Some(img) = &self.img {
                let (w, h) = img.dimensions();
                row(ui, "Suurus", &format!("{w} × {h} px"));
                row(ui, "Faili suurus", &fmt_bytes(self.file_size));
                row(ui, "Värviruum", if img.color().has_alpha() { "RGBA" } else { "RGB" });
                let crop = self
                    .crop_pixels(img)
                    .map(|(x, y, w, h)| {
                        format!(
                            "({x},{y})–({},{})\n  {w} × {h} px",
                            x + w,
                            y + h
                        )
                    })
                    .unwrap_or_else(|| "–".into());
                row(ui, "Kärpimine", &crop);
            } else {
                row(ui, "Suurus", "–");
                row(ui, "Faili suurus", "–");
                row(ui, "Värviruum", "–");
                row(ui, "Kärpimine", "–");
            }
        });
    }

    fn aspect_card(&mut self, ui: &mut Ui) {
        let mut chosen: Option<(&'static str, Option<f32>)> = None;
        card(ui, "Kuvasuhe", CYAN, |ui| {
            egui::Grid::new("aspect-grid")
                .num_columns(3)
                .spacing(Vec2::new(6.0, 6.0))
                .show(ui, |ui| {
                    for (i, (name, ratio)) in ASPECTS.iter().enumerate() {
                        let selected = self.aspect_name == *name;
                        let fill = if selected { ACCENT } else { SURFACE2 };
                        let fg = if selected { Color32::WHITE } else { TEXT_DIM };
                        let b = egui::Button::new(
                            RichText::new(*name).size(11.0).strong().color(fg),
                        )
                        .fill(fill)
                        .rounding(Rounding::same(6.0))
                        .min_size(Vec2::new(88.0, 26.0));
                        if ui.add(b).clicked() {
                            chosen = Some((*name, *ratio));
                        }
                        if i % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
            ui.add_space(4.0);
            let mut lock = self.lock;
            if ui.checkbox(&mut lock, "Lukusta kuvasuhe").changed() {
                self.lock = lock;
                self.toggle_lock();
            }
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Kogu pilt").size(11.0).color(TEXT_DIM))
                            .fill(SURFACE2)
                            .rounding(Rounding::same(6.0)),
                    )
                    .clicked()
                {
                    self.select_all();
                }
                if ui
                    .add(
                        egui::Button::new(RichText::new("Tühjenda").size(11.0).color(TEXT_DIM))
                            .fill(SURFACE2)
                            .rounding(Rounding::same(6.0)),
                    )
                    .clicked()
                {
                    self.clear_crop();
                }
            });
        });
        if let Some((name, ratio)) = chosen {
            self.set_aspect(name, ratio);
        }
    }

    fn view_card(&mut self, ui: &mut Ui) {
        card(ui, "Vaade", Color32::from_rgb(0xa7, 0x8b, 0xfa), |ui| {
            ui.checkbox(&mut self.show_thirds, "Kolmandiku reegel");
            ui.checkbox(&mut self.show_grid, "Ruudustik 6 × 6");
            ui.add_space(4.0);
            ui.label(
                RichText::new("Rulliga suum · keskmise nupuga lohista")
                    .size(10.5)
                    .color(TEXT_DIM),
            );
        });
    }

    fn resize_card(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        card(ui, "Suuruse muutmine", AMBER, |ui| {
            let (iw, ih) = self
                .img
                .as_ref()
                .map(|i| i.dimensions())
                .unwrap_or((0, 0));
            let mut changed_w = false;
            let mut changed_h = false;
            ui.horizontal(|ui| {
                ui.label(RichText::new("W").size(11.0).color(TEXT_DIM));
                changed_w = ui
                    .add(
                        egui::DragValue::new(&mut self.resize_w)
                            .range(1..=40000)
                            .speed(1.0),
                    )
                    .changed();
                ui.add_space(6.0);
                ui.label(RichText::new("H").size(11.0).color(TEXT_DIM));
                changed_h = ui
                    .add(
                        egui::DragValue::new(&mut self.resize_h)
                            .range(1..=40000)
                            .speed(1.0),
                    )
                    .changed();
            });
            if self.link_resize && iw > 0 && ih > 0 {
                if changed_w {
                    self.resize_h = ((self.resize_w as f32) * ih as f32 / iw as f32)
                        .round()
                        .max(1.0) as u32;
                } else if changed_h {
                    self.resize_w = ((self.resize_h as f32) * iw as f32 / ih as f32)
                        .round()
                        .max(1.0) as u32;
                }
            }
            ui.checkbox(&mut self.link_resize, "Lingi kuvasuhe");
            ui.add_space(2.0);
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Rakenda suurus").size(11.5).strong().color(Color32::BLACK),
                    )
                    .fill(AMBER)
                    .rounding(Rounding::same(7.0))
                    .min_size(Vec2::new(140.0, 28.0)),
                )
                .clicked()
            {
                self.apply_resize(ctx);
            }
        });
    }

    fn preview_card(&mut self, ui: &mut Ui) {
        card(ui, "Eelvaade", GREEN, |ui| {
            let Some(tex) = self.tex.as_ref() else {
                ui.label(RichText::new("—").size(12.0).color(TEXT_DIM));
                return;
            };
            let Some(img) = self.img.as_ref() else {
                return;
            };
            // UltraCrop._update_preview: get_crop_box() (clamped, int) or "—".
            let Some((x, y, w, h)) = self.crop_pixels(img) else {
                ui.label(RichText::new("—").size(12.0).color(TEXT_DIM));
                return;
            };
            let (iw, ih) = img.dimensions();
            let uv = Rect::from_min_max(
                Pos2::new(x as f32 / iw.max(1) as f32, y as f32 / ih.max(1) as f32),
                Pos2::new(
                    (x + w) as f32 / iw.max(1) as f32,
                    (y + h) as f32 / ih.max(1) as f32,
                ),
            );
            let cw = w as f32;
            let ch = h as f32;
            // PIL thumbnail((PANEL_W - 56, 180)) shrinks, never enlarges.
            let max_w = PREVIEW_MAX_W;
            let max_h = PREVIEW_MAX_H;
            let scale = (max_w / cw).min(max_h / ch).min(1.0);
            let pw = (cw * scale).max(1.0);
            let ph = (ch * scale).max(1.0);
            let avail = ui.available_width().max(40.0);
            ui.horizontal(|ui| {
                ui.add_space(((avail - pw) * 0.5).max(0.0));
                ui.add(
                    egui::Image::new((tex.id(), tex.size_vec2()))
                        .uv(uv)
                        // The widget otherwise fits the *full texture's*
                        // aspect ratio before applying `uv`, which squeezes a
                        // non-original crop into the original shape. The
                        // preview box is already sized from the crop, so use
                        // that exact size and stretch only the UV window.
                        .maintain_aspect_ratio(false)
                        .fit_to_exact_size(Vec2::new(pw, ph))
                        .rounding(Rounding::same(6.0)),
                );
            });
            ui.add_space(3.0);
            ui.label(
                RichText::new(format!(
                    "{} × {} px",
                    w, h
                ))
                .size(10.5)
                .color(TEXT_DIM),
            );
        });
    }

    fn recent_card(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let mut open: Option<PathBuf> = None;
        card(ui, "Viimased failid", PINK, |ui| {
            if self.recent.is_empty() {
                ui.label(
                    RichText::new("Siin pole veel faile")
                        .size(11.0)
                        .color(TEXT_DIM),
                );
            }
            for path in self.recent.iter().take(MAX_RECENT) {
                let name = file_name(path);
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(name).size(11.0).color(TEXT),
                        )
                        .fill(SURFACE2)
                        .rounding(Rounding::same(6.0))
                        .min_size(Vec2::new(ui.available_width(), 24.0)),
                    )
                    .on_hover_text(path.display().to_string())
                    .clicked()
                {
                    open = Some(path.clone());
                }
            }
        });
        if let Some(path) = open {
            self.open_path(ctx, path);
        }
    }

    fn shortcuts_card(&mut self, ui: &mut Ui) {
        card(ui, "Kiirklahvid", PINK, |ui| {
            let keys = [
                ("Ctrl+O", "Ava pilt"),
                ("Ctrl+S", "Salvesta"),
                ("Ctrl+Shift+S", "Kiire salvestus"),
                ("Ctrl+Z / Y", "Tagasi / uuesti"),
                ("Ctrl+X", "Kärbi"),
                ("Ctrl+C", "Kopeeri"),
                ("R / Shift+R", "Pööra"),
                ("H / V", "Peegelda"),
                ("Ratas", "Suum"),
                ("Ctrl+0", "Sobita"),
                ("Nooled", "Nihuta raami"),
                ("Esc", "Tühjenda"),
                ("F11", "Täisekraan"),
            ];
            for (key, desc) in keys {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [86.0, 18.0],
                        egui::Label::new(
                            RichText::new(key)
                                .size(9.5)
                                .monospace()
                                .color(Color32::from_rgb(0xa5, 0xb4, 0xfc)),
                        ),
                    );
                    ui.label(RichText::new(desc).size(10.0).color(TEXT_DIM));
                });
            }
        });
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::none()
                    .fill(SURFACE)
                    .inner_margin(Margin {
                        left: 12.0,
                        right: 12.0,
                        top: 5.0,
                        bottom: 5.0,
                    }),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.status).size(11.0).color(TEXT_DIM));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let coords = self
                            .cursor_img
                            .map(|p| {
                                format!(
                                    "X:{}  Y:{}",
                                    p.x.max(0.0) as i64,
                                    p.y.max(0.0) as i64
                                )
                            })
                            .unwrap_or_else(|| "X:–  Y:–".into());
                        ui.label(
                            RichText::new(coords)
                                .size(10.5)
                                .monospace()
                                .color(TEXT_DIM),
                        );
                        ui.label(RichText::new("·").size(10.0).color(BORDER));
                        ui.label(
                            RichText::new(format!("{}%", self.zoom_percent()))
                                .size(10.5)
                                .monospace()
                                .color(TEXT_DIM),
                        );
                    });
                });
            });
    }
}

impl eframe::App for CropApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.pending_fit {
            self.fit_window_to_image(ctx);
        }
        self.enforce_window_fit(ctx);
        self.handle_screenshot(ctx);
        self.handle_dropped(ctx);
        self.handle_shortcuts(ctx);

        self.toolbar(ctx);
        self.status_bar(ctx);
        self.side_panel(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(CANVAS_BG).inner_margin(Margin::same(10.0)))
            .show(ctx, |ui| {
                let size = ui.available_size();
                let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
                self.paint_canvas(ui, &resp, &painter);
            });
    }
}

// ─── Geometry helpers ──────────────────────────────────────────────────
/// UltraCrop._update_zoom_label+0x002C..0x006E — `int(zoom * 100)`:
/// CPython's int() truncates toward zero, so the label never rounds up.
fn zoom_label_percent(absolute_scale: f32) -> i32 {
    (absolute_scale * 100.0).trunc() as i32
}

/// CropCanvas.get_crop_box+0x0096..0x0234 — each edge is `int()`-truncated
/// (not rounded), clamped into the image, and the box is rejected when either
/// side is under 1 px (offset 0x0232 loads that 1).
fn crop_box_pixels(crop: Rect, iw: u32, ih: u32) -> Option<(u32, u32, u32, u32)> {
    let x = crop.min.x.max(0.0).min(iw as f32) as u32;
    let y = crop.min.y.max(0.0).min(ih as f32) as u32;
    let x2 = crop.max.x.max(0.0).min(iw as f32) as u32;
    let y2 = crop.max.y.max(0.0).min(ih as f32) as u32;
    if x2 <= x || y2 <= y {
        None
    } else {
        Some((x, y, x2 - x, y2 - y))
    }
}

/// UltraCrop.quick_save+0x0108..0x0198 — `base + "_karp" + ext`, then
/// `_karp2`, `_karp3` … while the name is taken (the counter starts at 2).
fn quick_save_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut path = dir.join(format!("{stem}_karp.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}_karp{n}.{ext}"));
        n += 1;
    }
    path
}

fn normalize(r: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(r.min.x.min(r.max.x), r.min.y.min(r.max.y)),
        Pos2::new(r.min.x.max(r.max.x), r.min.y.max(r.max.y)),
    )
}

fn fit_inside(r: Rect, b: Rect) -> Rect {
    let mut r = normalize(r);
    let sw = if r.width() > b.width() && r.width() > 0.0 {
        b.width() / r.width()
    } else {
        1.0
    };
    let sh = if r.height() > b.height() && r.height() > 0.0 {
        b.height() / r.height()
    } else {
        1.0
    };
    let s = sw.min(sh);
    if s < 1.0 {
        r = Rect::from_center_size(r.center(), r.size() * s);
    }
    let dx = if r.min.x < b.min.x {
        b.min.x - r.min.x
    } else if r.max.x > b.max.x {
        b.max.x - r.max.x
    } else {
        0.0
    };
    let dy = if r.min.y < b.min.y {
        b.min.y - r.min.y
    } else if r.max.y > b.max.y {
        b.max.y - r.max.y
    } else {
        0.0
    };
    normalize(r.translate(Vec2::new(dx, dy)))
}

fn drag_rect(start: Pos2, end: Pos2, ratio: Option<f32>, bounds: Rect) -> Rect {
    let mut dx = end.x - start.x;
    let mut dy = end.y - start.y;
    if let Some(r) = ratio {
        let r = r.max(0.001);
        if dx.abs() > dy.abs() * r {
            let sign = if dy >= 0.0 { 1.0 } else { -1.0 };
            dy = dx.abs() / r * sign;
        } else {
            let sign = if dx >= 0.0 { 1.0 } else { -1.0 };
            dx = dy.abs() * r * sign;
        }
        let end2 = Pos2::new(
            (start.x + dx).clamp(bounds.min.x, bounds.max.x),
            (start.y + dy).clamp(bounds.min.y, bounds.max.y),
        );
        return fit_inside(Rect::from_two_pos(start, end2), bounds);
    }
    let end = Pos2::new(
        end.x.clamp(bounds.min.x, bounds.max.x),
        end.y.clamp(bounds.min.y, bounds.max.y),
    );
    fit_inside(Rect::from_two_pos(start, end), bounds)
}

fn resize_rect(orig: Rect, idx: usize, p: Pos2, ratio: Option<f32>, bounds: Rect) -> Rect {
    let orig = normalize(orig);
    let left = matches!(idx, 0 | 3 | 6);
    let right = matches!(idx, 1 | 2 | 7);
    let top = matches!(idx, 0 | 1 | 4);
    let bottom = matches!(idx, 2 | 3 | 5);

    if let Some(r) = ratio {
        let r = r.max(0.001);
        if matches!(idx, 0 | 1 | 2 | 3) {
            // Corner drag: the opposite corner stays put.
            let (ax, ay) = match idx {
                0 => (orig.max.x, orig.max.y),
                1 => (orig.min.x, orig.max.y),
                2 => (orig.min.x, orig.min.y),
                _ => (orig.max.x, orig.min.y),
            };
            let dx = (p.x - ax).abs();
            let dy = (p.y - ay).abs();
            let w = dx.max(dy * r).max(1.0);
            let h = (w / r).max(1.0);
            let rect = Rect::from_min_max(
                Pos2::new(
                    if left { ax - w } else { ax },
                    if top { ay - h } else { ay },
                ),
                Pos2::new(
                    if left { ax } else { ax + w },
                    if top { ay } else { ay + h },
                ),
            );
            return fit_inside(rect, bounds);
        } else if idx == 4 || idx == 5 {
            // Top/bottom edge: height drives, width stays centred.
            let h = (p.y - if idx == 4 { orig.max.y } else { orig.min.y })
                .abs()
                .max(1.0);
            let w = (h * r).max(1.0);
            let cx = orig.center().x;
            let y0 = if idx == 4 { orig.max.y - h } else { orig.min.y };
            let rect = Rect::from_min_max(
                Pos2::new(cx - w * 0.5, y0),
                Pos2::new(cx + w * 0.5, y0 + h),
            );
            return fit_inside(rect, bounds);
        } else {
            // Left/right edge: width drives, height stays centred.
            let w = (p.x - if idx == 6 { orig.max.x } else { orig.min.x })
                .abs()
                .max(1.0);
            let h = (w / r).max(1.0);
            let cy = orig.center().y;
            let x0 = if idx == 6 { orig.max.x - w } else { orig.min.x };
            let rect = Rect::from_min_max(
                Pos2::new(x0, cy - h * 0.5),
                Pos2::new(x0 + w, cy + h * 0.5),
            );
            return fit_inside(rect, bounds);
        }
    }

    let mut l = orig.min.x;
    let mut t = orig.min.y;
    let mut rr = orig.max.x;
    let mut b = orig.max.y;
    let px = p.x.clamp(bounds.min.x, bounds.max.x);
    let py = p.y.clamp(bounds.min.y, bounds.max.y);
    if left {
        l = px;
    }
    if right {
        rr = px;
    }
    if top {
        t = py;
    }
    if bottom {
        b = py;
    }
    let rect = normalize(Rect::from_min_max(Pos2::new(l, t), Pos2::new(rr, b)));
    fit_inside(rect, bounds)
}

fn dashed_rect(painter: &egui::Painter, r: Rect, dash: f32, gap: f32, stroke: Stroke) {
    dashed_rect_phase(painter, r, dash, gap, 0.0, stroke);
}

fn dashed_rect_phase(
    painter: &egui::Painter,
    r: Rect,
    dash: f32,
    gap: f32,
    phase: f32,
    stroke: Stroke,
) {
    dashed_line_phase(painter, r.left_top(), r.right_top(), dash, gap, phase, stroke);
    dashed_line_phase(painter, r.right_top(), r.right_bottom(), dash, gap, phase, stroke);
    dashed_line_phase(painter, r.right_bottom(), r.left_bottom(), dash, gap, phase, stroke);
    dashed_line_phase(painter, r.left_bottom(), r.left_top(), dash, gap, phase, stroke);
}

#[allow(clippy::too_many_arguments)]
fn dashed_line_phase(
    painter: &egui::Painter,
    a: Pos2,
    b: Pos2,
    dash: f32,
    gap: f32,
    phase: f32,
    stroke: Stroke,
) {
    let d = b - a;
    let len = d.length();
    if len < 0.5 {
        return;
    }
    let dir = d / len;
    let mut t = -phase.rem_euclid(dash + gap);
    while t < len {
        let s = t.max(0.0);
        let e = (t + dash).min(len);
        if e > s {
            painter.line_segment([a + dir * s, a + dir * e], stroke);
        }
        t += dash + gap;
    }
}

fn paint_checkerboard(painter: &egui::Painter, r: Rect) {
    painter.rect_filled(r, Rounding::ZERO, Color32::from_rgb(0x22, 0x26, 0x30));
    let step = 14.0;
    let clip = painter.with_clip_rect(r.intersect(painter.clip_rect()));
    let mut y = r.top();
    let mut row = 0;
    while y < r.bottom() {
        let mut x = r.left() + if row % 2 == 0 { 0.0 } else { step };
        while x < r.right() {
            let cell = Rect::from_min_max(
                Pos2::new(x, y),
                Pos2::new((x + step).min(r.right()), (y + step).min(r.bottom())),
            );
            clip.rect_filled(cell, Rounding::ZERO, Color32::from_rgb(0x2a, 0x2f, 0x3c));
            x += step * 2.0;
        }
        y += step;
        row += 1;
    }
}

// ─── UI helpers ────────────────────────────────────────────────────────
fn install_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = SURFACE;
    style.visuals.extreme_bg_color = SURFACE2;
    style.visuals.faint_bg_color = SURFACE2;
    style.visuals.widgets.noninteractive.bg_fill = SURFACE;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    style.visuals.widgets.inactive.bg_fill = SURFACE2;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    style.visuals.widgets.hovered.bg_fill = SURFACE3;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.active.bg_fill = ACCENT;
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT_HOVER);
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.slider_width = 140.0;
    style.text_styles.insert(
        egui::TextStyle::Body,
        FontId::proportional(12.5),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        FontId::proportional(12.5),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        FontId::proportional(10.5),
    );
    ctx.set_style(style);
}

fn tool_button(
    ui: &mut Ui,
    label: &str,
    fill: Color32,
    fg: Color32,
    enabled: bool,
) -> Response {
    let text_color = if enabled { fg } else { Color32::from_rgb(0x5b, 0x63, 0x7c) };
    let fill = if enabled { fill } else { SURFACE2 };
    let b = egui::Button::new(
        RichText::new(label)
            .size(12.0)
            .strong()
            .color(text_color),
    )
    .fill(fill)
    .rounding(Rounding::same(7.0))
    .min_size(Vec2::new(0.0, 30.0));
    ui.add_enabled(enabled, b)
}

fn card<R>(ui: &mut Ui, title: &str, accent: Color32, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::none()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, p) = ui.allocate_painter(Vec2::new(3.0, 15.0), Sense::hover());
                p.rect_filled(r.rect, Rounding::same(2.0), accent);
                ui.add_space(2.0);
                ui.label(
                    RichText::new(title)
                        .size(12.5)
                        .strong()
                        .color(accent),
                );
            });
            ui.add_space(5.0);
            add(ui)
        })
        .inner
}

fn row(ui: &mut Ui, key: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [76.0, 18.0],
            egui::Label::new(RichText::new(key).size(11.0).color(TEXT_DIM)),
        );
        ui.add(
            egui::Label::new(RichText::new(value).size(11.0).color(TEXT))
                .truncate(),
        );
    });
}

// ─── Files ─────────────────────────────────────────────────────────────
/// UltraCrop._open_file L1457-1460: "A" in mode → RGBA, else RGB.
fn normalise_image(img: DynamicImage) -> DynamicImage {
    if img.color().has_alpha() {
        DynamicImage::ImageRgba8(img.to_rgba8())
    } else {
        DynamicImage::ImageRgb8(img.to_rgb8())
    }
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "bmp" | "gif" | "tif" | "tiff" | "webp" | "ico")
    )
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn fmt_bytes(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.2} MB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{:.1} kB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn config_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".config")))?;
    Some(base.join("ultracrop"))
}

fn recent_file() -> Option<PathBuf> {
    Some(config_dir()?.join("recent.txt"))
}

fn load_recent() -> Vec<PathBuf> {
    let Some(path) = recent_file() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .take(MAX_RECENT)
        .collect()
}

fn save_recent(items: &[PathBuf]) {
    let Some(path) = recent_file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = items
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::write(path, text);
}

/// Each hand-off gets a fresh PNG: PhotoCraft then opens a new tab with the
/// crop that was just drawn, instead of reusing a stale path from last time.
fn write_photocraft_export(img: &DynamicImage, stem: &str) -> Result<PathBuf, String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join("ultracrop-photocraft");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("{stem}_karp_{stamp}_{seq}.png"));
    img.save_with_format(&path, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(path)
}

fn save_screenshot(image: &egui::ColorImage, path: &Path) -> Result<(), String> {
    let (w, h) = (image.width() as u32, image.height() as u32);
    if w == 0 || h == 0 {
        return Err("empty picture".into());
    }
    let mut bytes = Vec::with_capacity((w * h * 4) as usize);
    for pixel in &image.pixels {
        let [r, g, b, a] = pixel.to_srgba_unmultiplied();
        bytes.extend_from_slice(&[r, g, b, a]);
    }
    let rgba = image::RgbaImage::from_raw(w, h, bytes)
        .ok_or_else(|| "could not build picture".to_string())?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    rgba.save_with_format(path, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    eprintln!("saved screenshot to {}", path.display());
    Ok(())
}

/// The window/taskbar icon is embedded so it travels with the binary, even
/// when the desktop file is not installed.
fn app_icon() -> Option<Arc<egui::IconData>> {
    let img = image::load_from_memory(include_bytes!("../icon-ultracrop-256.png"))
        .ok()?
        .to_rgba8();
    let (w, h) = img.dimensions();
    Some(Arc::new(egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    }))
}

// ─── Entry point ───────────────────────────────────────────────────────
fn main() -> eframe::Result<()> {
    // eframe 0.29 uses winit 0.30, whose Wayland backend never reports file
    // drops (only X11, Windows and macOS do). On a Wayland session that has
    // XWayland available we therefore start through X11, so dragging an image
    // onto the window actually works instead of silently doing nothing.
    prefer_x11_for_file_drops();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("UltraCrop")
        .with_inner_size([1180.0, 780.0])
        .with_min_inner_size([MIN_WINDOW_W, MIN_WINDOW_H])
        .with_drag_and_drop(true);
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "UltraCrop",
        options,
        Box::new(|cc| Ok(Box::new(CropApp::new(cc)))),
    )
}

/// winit's Wayland backend has no drag-and-drop support, while its X11
/// backend does. If XWayland is available, hide the Wayland display so winit
/// falls back to X11 and file drops are delivered again.
fn prefer_x11_for_file_drops() {
    #[cfg(target_os = "linux")]
    {
        let on_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let has_x11 = std::env::var_os("DISPLAY").is_some();
        if on_wayland && has_x11 {
            std::env::remove_var("WAYLAND_DISPLAY");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 800.0))
    }

    #[test]
    fn ratio_resize_stays_square() {
        let orig = Rect::from_min_max(Pos2::new(100.0, 100.0), Pos2::new(500.0, 400.0));
        let r = resize_rect(orig, 2, Pos2::new(250.0, 230.0), Some(1.0), bounds());
        assert!((r.width() - r.height()).abs() < 0.01);
        assert!(r.min.x >= 0.0 && r.min.y >= 0.0);
        assert!(r.max.x <= 1000.0 && r.max.y <= 800.0);
    }

    #[test]
    fn ratio_drag_clamps_to_image() {
        let r = drag_rect(
            Pos2::new(900.0, 700.0),
            Pos2::new(1200.0, 900.0),
            Some(1.0),
            bounds(),
        );
        assert!((r.width() - r.height()).abs() < 0.01);
        assert!(r.max.x <= 1000.01 && r.max.y <= 800.01);
    }

    #[test]
    fn fit_inside_keeps_aspect() {
        let r = fit_inside(
            Rect::from_min_max(Pos2::new(-50.0, -50.0), Pos2::new(1500.0, 700.0)),
            bounds(),
        );
        assert!(r.min.x >= -0.01 && r.min.y >= -0.01);
        assert!(r.max.x <= 1000.01 && r.max.y <= 800.01);
        assert!((r.width() / r.height()) - (1550.0 / 750.0) < 0.01);
    }

    // ── Rules read out of the decompiled original ────────────────────
    // Each test cites the bytecode address the value came from.

    #[test]
    fn constants_match_the_decompiled_ledger() {
        // <module>+0x0138 MAX_RECENT 8, +0x013C HANDLE_HALF 7,
        // +0x0140 PANEL_W 290, +0x0144 SIDE_W 320, +0x0148 CHROME_H 175,
        // +0x014C CHROME_W 40, +0x0150 MIN_BOX 12, +0x0154 UNDO_LIMIT 10.
        assert_eq!(MAX_RECENT, 8);
        assert_eq!(HANDLE_HALF, 7.0);
        assert_eq!(HANDLE_HIT, 9.0);
        assert_eq!(PANEL_W, 290.0);
        assert_eq!(SIDE_W, 320.0);
        assert_eq!(CHROME_H, 175.0);
        assert_eq!(CHROME_W, 40.0);
        assert_eq!(MIN_BOX, 12.0);
        assert_eq!(UNDO_LIMIT, 10);
        // CropCanvas.__init__+0x006C / +0x007A: 0.05 and 8.0.
        assert_eq!(ZOOM_MIN, 0.05);
        assert_eq!(ZOOM_MAX, 8.0);
        // CropCanvas._on_wheel+0x0094: 1.12 per notch.
        assert_eq!(ZOOM_STEP, 1.12);
        // _update_preview+0x00F6 / +0x0104: PANEL_W - 56 and 180.
        assert_eq!(PREVIEW_MAX_W, 234.0);
        assert_eq!(PREVIEW_MAX_H, 180.0);
        // UltraCrop.__init__+0x009A: minsize(820, 540).
        assert_eq!(MIN_WINDOW_W, 820.0);
        assert_eq!(MIN_WINDOW_H, 540.0);
    }

    #[test]
    fn zoom_label_truncates_like_python_int() {
        // UltraCrop._update_zoom_label+0x002C: int(_zoom * 100)
        assert_eq!(zoom_label_percent(1.126), 112);
        assert_eq!(zoom_label_percent(0.999), 99);
        assert_eq!(zoom_label_percent(0.05), 5);
        assert_eq!(zoom_label_percent(8.0), 800);
    }

    #[test]
    fn crop_edges_truncate_then_clamp() {
        // CropCanvas.get_crop_box+0x0096: int() on each edge, clamp to image.
        let crop = Rect::from_min_max(Pos2::new(10.9, 20.9), Pos2::new(100.9, 200.9));
        assert_eq!(crop_box_pixels(crop, 500, 500), Some((10, 20, 90, 180)));
        // Off-image edges clamp to the image size instead of overflowing.
        let crop = Rect::from_min_max(Pos2::new(-40.0, -40.0), Pos2::new(900.0, 900.0));
        assert_eq!(crop_box_pixels(crop, 500, 400), Some((0, 0, 500, 400)));
    }

    #[test]
    fn crop_under_one_pixel_is_rejected() {
        // +0x0232 compares the side against 1 and returns None.
        let crop = Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(10.9, 40.0));
        assert_eq!(crop_box_pixels(crop, 500, 500), None);
    }

    #[test]
    fn quick_save_counts_from_two() {
        // UltraCrop.quick_save+0x0108: base + "_karp" + ext, then _karp2 …
        let dir = std::env::temp_dir().join(format!("ultracrop-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let first = quick_save_path(&dir, "photo", "png");
        assert!(first.ends_with("photo_karp.png"));
        std::fs::write(&first, b"x").unwrap();
        let second = quick_save_path(&dir, "photo", "png");
        assert!(second.ends_with("photo_karp2.png"));
        std::fs::write(&second, b"x").unwrap();
        let third = quick_save_path(&dir, "photo", "png");
        assert!(third.ends_with("photo_karp3.png"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_size_formatting_matches_fmt_bytes() {
        // UltraCrop._fmt_bytes+0x0004 / +0x002C: 1 MiB, 1 KiB thresholds.
        assert_eq!(fmt_bytes(1024 * 1024), "1.00 MB");
        assert_eq!(fmt_bytes(1536), "1.5 kB");
        assert_eq!(fmt_bytes(1023), "1023 B");
    }

    #[test]
    fn photocraft_export_writes_a_fresh_png() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            9,
            4,
            image::Rgb([12, 34, 56]),
        ));
        let first = write_photocraft_export(&img, "test").unwrap();
        let second = write_photocraft_export(&img, "test").unwrap();
        assert_ne!(first, second);
        let loaded = image::open(&first).unwrap();
        assert_eq!(loaded.dimensions(), (9, 4));
        let _ = std::fs::remove_file(first);
        let _ = std::fs::remove_file(second);
    }

    #[test]
    fn photocraft_export_keeps_the_whole_crop() {
        let img = DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            100,
            80,
            image::Rgb([12, 34, 56]),
        ));
        let crop = Rect::from_min_max(Pos2::new(20.0, 10.0), Pos2::new(70.0, 60.0));
        let (x, y, w, h) = crop_box_pixels(crop, 100, 80).unwrap();
        let path = write_photocraft_export(&img.crop_imm(x, y, w, h), "test").unwrap();
        assert_eq!(image::open(&path).unwrap().dimensions(), (50, 50));
        let _ = std::fs::remove_file(path);
    }
}
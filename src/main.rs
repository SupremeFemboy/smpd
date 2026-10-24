mod config;
mod loader;
mod mpd;
mod render;
mod ui;

use config::Config;
use loader::{AsyncImageLoader, LoaderTarget};
use mpd::{BrowserItem, MpdClient, MpdStatus, PlayState, PlaylistInfo, Track};
use render::{CardVertex, RenderState, Uniforms};
use ui::{AppMode, FontManager, render_ui_overlay};

use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use ::wayland_client::WEnum;
use ::wayland_client::protocol::wl_pointer::ButtonState;
use layershellev::keyboard::{Key, KeyCode, ModifiersState, PhysicalKey};
use layershellev::reexport::*;
use layershellev::xkb_keyboard::ElementState;
use layershellev::*;

struct SmpdApp {
    config: Config,
    font_mgr: FontManager,
    mpd: MpdClient,
    status: MpdStatus,

    mode: AppMode,
    tracks: Arc<RwLock<Vec<Track>>>,
    playlists: Arc<RwLock<Vec<PlaylistInfo>>>,
    browser_items: Arc<RwLock<Vec<BrowserItem>>>,
    browser_dir: String,

    target_index: usize,
    current_scroll: f32,
    target_scroll: f32,

    queue_index: usize,
    queue_current_scroll: f32,
    queue_target_scroll: f32,

    playlist_index: usize,
    playlist_current_scroll: f32,
    playlist_target_scroll: f32,

    browser_index: usize,
    browser_current_scroll: f32,
    browser_target_scroll: f32,

    save_input: Option<String>,
    delete_confirm: Option<String>,
    overwrite_confirm: Option<String>,
    modifiers: ModifiersState,

    render_state: Option<RenderState>,
    loader: Option<AsyncImageLoader>,

    cursor_pos: [f32; 2],
    screen_size: [f32; 2],
    overlay_dirty: bool,
    last_frame_time: Instant,
}

impl SmpdApp {
    #[allow(clippy::too_many_arguments)]
    fn new(
        tracks: Vec<Track>,
        playlists: Vec<PlaylistInfo>,
        browser_items: Vec<BrowserItem>,
        start_index: usize,
        config: Config,
        font_mgr: FontManager,
        mpd: MpdClient,
        status: MpdStatus,
    ) -> Self {
        let tracks = Arc::new(RwLock::new(tracks));
        let playlists = Arc::new(RwLock::new(playlists));
        let browser_items = Arc::new(RwLock::new(browser_items));

        Self {
            config,
            font_mgr,
            mpd,
            status,
            mode: AppMode::Queue,
            tracks,
            playlists,
            browser_items,
            browser_dir: String::new(),
            target_index: start_index,
            current_scroll: start_index as f32,
            target_scroll: start_index as f32,
            queue_index: start_index,
            queue_current_scroll: start_index as f32,
            queue_target_scroll: start_index as f32,
            playlist_index: 0,
            playlist_current_scroll: 0.0,
            playlist_target_scroll: 0.0,
            browser_index: 0,
            browser_current_scroll: 0.0,
            browser_target_scroll: 0.0,
            save_input: None,
            delete_confirm: None,
            overwrite_confirm: None,
            modifiers: ModifiersState::default(),
            render_state: None,
            loader: None,
            cursor_pos: [0.0, 0.0],
            screen_size: [0.0, 0.0],
            overlay_dirty: true,
            last_frame_time: Instant::now(),
        }
    }

    fn current_count(&self) -> usize {
        match self.mode {
            AppMode::Queue => self.tracks.read().unwrap().len(),
            AppMode::Playlists => self.playlists.read().unwrap().len(),
            AppMode::Folders => self.browser_items.read().unwrap().len(),
        }
    }

    fn switch_to_mode(&mut self, new_mode: AppMode) {
        if self.mode == new_mode {
            return;
        }

        // Save current scroll state
        match self.mode {
            AppMode::Queue => {
                self.queue_index = self.target_index;
                self.queue_current_scroll = self.current_scroll;
                self.queue_target_scroll = self.target_scroll;
            }
            AppMode::Playlists => {
                self.playlist_index = self.target_index;
                self.playlist_current_scroll = self.current_scroll;
                self.playlist_target_scroll = self.target_scroll;
            }
            AppMode::Folders => {
                self.browser_index = self.target_index;
                self.browser_current_scroll = self.current_scroll;
                self.browser_target_scroll = self.target_scroll;
            }
        }

        self.mode = new_mode;

        // Restore new scroll state
        match self.mode {
            AppMode::Queue => {
                self.refresh_status();
                let count = self.tracks.read().unwrap().len();
                self.target_index = if count > 0 {
                    self.queue_index.min(count - 1)
                } else {
                    0
                };
                self.current_scroll = self.queue_current_scroll;
                self.target_scroll = self.target_index as f32;
            }
            AppMode::Playlists => {
                self.reload_stored_playlists();
                let count = self.playlists.read().unwrap().len();
                self.target_index = if count > 0 {
                    self.playlist_index.min(count - 1)
                } else {
                    0
                };
                self.current_scroll = self.playlist_current_scroll;
                self.target_scroll = self.target_index as f32;
            }
            AppMode::Folders => {
                self.reload_browser_items();
                let count = self.browser_items.read().unwrap().len();
                self.target_index = if count > 0 {
                    self.browser_index.min(count - 1)
                } else {
                    0
                };
                self.current_scroll = self.browser_current_scroll;
                self.target_scroll = self.target_index as f32;
            }
        }

        self.overlay_dirty = true;
        self.request_adjacent_textures();
    }

    fn select_prev(&mut self) {
        if self.target_index > 0 {
            self.target_index -= 1;
            self.target_scroll = self.target_index as f32;
            self.overlay_dirty = true;
            self.request_adjacent_textures();
        }
    }

    fn select_next(&mut self) {
        let count = self.current_count();
        if self.target_index + 1 < count {
            self.target_index += 1;
            self.target_scroll = self.target_index as f32;
            self.overlay_dirty = true;
            self.request_adjacent_textures();
        }
    }

    fn request_adjacent_textures(&mut self) {
        let center = self.target_index;
        let count = self.current_count();
        let mode = self.mode;

        if let Some(loader) = &mut self.loader {
            match mode {
                AppMode::Queue => {
                    loader.request_queue(center);
                    for offset in 1..=4 {
                        if center >= offset {
                            loader.request_queue(center - offset);
                        }
                        if center + offset < count {
                            loader.request_queue(center + offset);
                        }
                    }
                }
                AppMode::Playlists => {
                    loader.request_playlist(center);
                    for offset in 1..=4 {
                        if center >= offset {
                            loader.request_playlist(center - offset);
                        }
                        if center + offset < count {
                            loader.request_playlist(center + offset);
                        }
                    }
                }
                AppMode::Folders => {
                    loader.request_browser(center);
                    for offset in 1..=4 {
                        if center >= offset {
                            loader.request_browser(center - offset);
                        }
                        if center + offset < count {
                            loader.request_browser(center + offset);
                        }
                    }
                }
            }
        }
    }

    fn confirm_selection(&mut self) {
        match self.mode {
            AppMode::Queue => {
                let track_opt = self.tracks.read().unwrap().get(self.target_index).cloned();
                if let Some(track) = track_opt {
                    let _ = self.mpd.play_id(track.id);
                    self.refresh_status();

                    if self.config.mpd.close_on_select {
                        std::process::exit(0);
                    }
                }
            }
            AppMode::Playlists => {
                let pl_opt = self
                    .playlists
                    .read()
                    .unwrap()
                    .get(self.target_index)
                    .cloned();
                if let Some(pl) = pl_opt {
                    // Clears current queue, loads new playlist, and plays from track 0
                    let _ = self.mpd.load_playlist_replace(&pl.name);

                    // Clear texture cache for old queue
                    if let Some(ref mut rs) = self.render_state {
                        rs.queue_texture_cache.clear();
                    }

                    // Reload active playlist queue from MPD
                    self.reload_playlist();

                    // Switch back to Mode 1 (Queue)
                    self.mode = AppMode::Queue;
                    self.target_index = 0;
                    self.target_scroll = 0.0;
                    self.current_scroll = 0.0;
                    self.queue_index = 0;
                    self.queue_target_scroll = 0.0;
                    self.queue_current_scroll = 0.0;

                    self.overlay_dirty = true;
                    self.request_adjacent_textures();
                }
            }
            AppMode::Folders => {
                let item_opt = self
                    .browser_items
                    .read()
                    .unwrap()
                    .get(self.target_index)
                    .cloned();
                if let Some(item) = item_opt {
                    if item.is_dir {
                        // Enter directory
                        self.browser_dir = item.path;
                        self.reload_browser_items();
                    } else {
                        // Add single file to queue
                        let _ = self.mpd.add_to_queue(&item.path);
                        self.reload_playlist();
                    }
                }
            }
        }
    }

    fn refresh_status(&mut self) {
        if let Ok(st) = self.mpd.get_status() {
            self.status = st;
            if let Some(current_id) = self.status.song_id {
                let mut tracks_guard = self.tracks.write().unwrap();
                for track in tracks_guard.iter_mut() {
                    track.is_current = track.id == current_id;
                }
            }
            self.overlay_dirty = true;
        }
    }

    fn reload_playlist(&mut self) {
        if let Ok(st) = self.mpd.get_status() {
            self.status = st;
        }
        if let Ok(tr) = self.mpd.get_playlist(self.status.song_id) {
            let count = tr.len();
            *self.tracks.write().unwrap() = tr;
            if count == 0 {
                self.queue_index = 0;
                self.queue_scroll_reset();
            } else {
                self.queue_index = self.queue_index.min(count - 1);
            }
            self.overlay_dirty = true;
        }
    }

    fn queue_scroll_reset(&mut self) {
        self.queue_target_scroll = 0.0;
        self.queue_current_scroll = 0.0;
        if self.mode == AppMode::Queue {
            self.target_index = 0;
            self.target_scroll = 0.0;
            self.current_scroll = 0.0;
        }
    }

    fn reload_stored_playlists(&mut self) {
        if let Ok(pls) = self.mpd.list_playlists() {
            let count = pls.len();
            *self.playlists.write().unwrap() = pls;
            if count == 0 {
                self.playlist_index = 0;
                self.playlist_target_scroll = 0.0;
                self.playlist_current_scroll = 0.0;
                if self.mode == AppMode::Playlists {
                    self.target_index = 0;
                    self.target_scroll = 0.0;
                    self.current_scroll = 0.0;
                }
            } else {
                self.playlist_index = self.playlist_index.min(count - 1);
                if self.mode == AppMode::Playlists {
                    self.target_index = self.playlist_index;
                    self.target_scroll = self.target_index as f32;
                }
            }
            self.overlay_dirty = true;
        }
    }

    fn reload_browser_items(&mut self) {
        if let Ok(items) = self.mpd.list_directory(&self.browser_dir) {
            let count = items.len();
            *self.browser_items.write().unwrap() = items;
            self.browser_index = 0;
            self.browser_target_scroll = 0.0;
            self.browser_current_scroll = 0.0;
            if self.mode == AppMode::Folders {
                self.target_index = 0;
                self.target_scroll = 0.0;
                self.current_scroll = 0.0;
            }
            if let Some(ref mut rs) = self.render_state {
                rs.browser_texture_cache.clear();
            }
            self.overlay_dirty = true;
            if count > 0 {
                self.request_adjacent_textures();
            }
        }
    }
}

fn print_help() {
    println!(
        r#"smpd — MPD Active Track Selector on Wayland Layer Shell

Usage: smpd [OPTIONS]

Options:
  --host <HOST>         MPD server host or unix socket path [default: 127.0.0.1 or $MPD_HOST]
  --port <PORT>         MPD server port [default: 6600 or $MPD_PORT]
  --config <PATH>       Path to custom TOML configuration file
  -h, --help            Print help information
  -v, --version         Print version information

Controls:
  [1]                   Mode 1: Active Queue management
  [2]                   Mode 2: Stored Playlists selector (replaces queue)
  [3]                   Mode 3: Library Folders browser (add files/folders)
  [Tab]                 Toggle between Queue, Playlists, and Folders mode
  [h / ←]               Previous item
  [l / →]               Next item
  [Space]               Toggle play / pause (in Queue mode)
  [Enter]               Play track / Load playlist / Open folder
  [a]                   Add focused folder or file to active queue (in Folders mode)
  [Backspace / u]       Go up one directory level (in Folders mode)
  [C / x]               Clear current active queue (in Queue mode)
  [s]                   Save current active queue as a new playlist
  [c]                   Center on currently playing track (in Queue mode)
  [n / p]               Next / previous track in MPD (in Queue mode)
  [Delete / d]          Remove focused track from current playlist (in Queue mode)
  [Home / End]          Jump to start / end of carousel
  [Mouse Click]         Click card to focus; click focused card to select
  [Mouse Wheel]         Scroll through carousel
  [Esc / q]             Quit
"#
    );
}

fn main() {
    let mut config = Config::load();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_help();
                return;
            }
            "-v" | "--version" => {
                println!("smpd {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--host" => {
                if let Some(host) = args.next() {
                    config.mpd.host = host;
                }
            }
            "--port" => {
                if let Some(port_str) = args.next()
                    && let Ok(p) = port_str.parse::<u16>()
                {
                    config.mpd.port = p;
                }
            }
            "--config" => {
                if let Some(cfg_path) = args.next()
                    && let Ok(content) = std::fs::read_to_string(&cfg_path)
                {
                    if let Ok(loaded) = toml::from_str::<Config>(&content) {
                        config = loaded;
                    } else {
                        eprintln!("[smpd] Failed to parse custom config: {cfg_path}");
                    }
                }
            }
            _ => {}
        }
    }

    let font_mgr = FontManager::new(config.ui.font_spec());

    let mut mpd = match MpdClient::connect(&config.mpd) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("smpd error: Could not connect to MPD: {e}");
            std::process::exit(1);
        }
    };

    let status = mpd.get_status().unwrap_or(MpdStatus {
        state: PlayState::Stop,
        song_pos: None,
        song_id: None,
        elapsed_secs: None,
        duration_secs: None,
        volume: None,
        playlist_length: 0,
    });

    let tracks = mpd.get_playlist(status.song_id).unwrap_or_default();
    let playlists = mpd.list_playlists().unwrap_or_default();
    let browser_items = mpd.list_directory("").unwrap_or_default();

    let start_index = if let Some(song_id) = status.song_id {
        tracks.iter().position(|t| t.id == song_id).unwrap_or(0)
    } else {
        0
    };

    let ev: WindowState<()> = WindowState::new("smpd")
        .with_use_display_handle(true)
        .with_layer(Layer::Overlay)
        .with_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right)
        .with_keyboard_interacivity(KeyboardInteractivity::Exclusive)
        .with_exclusive_zone(-1)
        .with_size((0, 0))
        .build()
        .expect("Failed to create Wayland layer shell surface");

    let mut app = SmpdApp::new(
        tracks,
        playlists,
        browser_items,
        start_index,
        config,
        font_mgr,
        mpd,
        status,
    );

    let _ = ev.running(move |event, state, unit_id| {
        if let LayerShellEvent::RequestMessages(msg) = event {
            match msg {
                DispatchMessage::RequestRefresh { width, height, .. } => {
                    let (sw, sh) = (*width as f32, *height as f32);
                    app.screen_size = [sw, sh];

                    if *width == 0 || *height == 0 {
                        return ReturnData::None;
                    }

                    // Initialize GPU render state on first refresh
                    if app.render_state.is_none() {
                        let wrapper = Arc::new(state.gen_mainwindow_wrapper());
                        let r_state =
                            pollster::block_on(RenderState::new(wrapper, *width, *height));

                        let max_thumb = app.config.cache.max_thumbnail_size;
                        let use_cache = app.config.cache.enabled;

                        let mut loader = AsyncImageLoader::new(
                            Arc::clone(&app.tracks),
                            Arc::clone(&app.playlists),
                            Arc::clone(&app.browser_items),
                            app.config.mpd.clone(),
                            max_thumb,
                            use_cache,
                        );

                        let center = app.target_index;
                        loader.request_queue(center);
                        for offset in 1..=4 {
                            if center >= offset {
                                loader.request_queue(center - offset);
                            }
                            if center + offset < app.tracks.read().unwrap().len() {
                                loader.request_queue(center + offset);
                            }
                        }

                        app.render_state = Some(r_state);
                        app.loader = Some(loader);
                        app.overlay_dirty = true;
                        app.last_frame_time = Instant::now();
                    }

                    let count = app.current_count();
                    let mode = app.mode;
                    let center = app.target_index;

                    let Some(r_state) = app.render_state.as_mut() else {
                        return ReturnData::None;
                    };

                    r_state.resize(*width, *height);

                    // Poll loader
                    if let Some(loader) = &mut app.loader {
                        while let Some(loaded) = loader.try_recv() {
                            loader.mark_completed(loaded.target);
                            if let Ok((img, _, _)) = loaded.result {
                                let tex = RenderState::create_texture_from_rgba(
                                    &r_state.device,
                                    &r_state.queue,
                                    &r_state.card_group_layout,
                                    &img,
                                    Some("card-texture"),
                                );
                                match loaded.target {
                                    LoaderTarget::Queue(idx) => {
                                        r_state.queue_texture_cache.insert(idx, tex);
                                    }
                                    LoaderTarget::Playlist(idx) => {
                                        r_state.playlist_texture_cache.insert(idx, tex);
                                    }
                                    LoaderTarget::Browser(idx) => {
                                        r_state.browser_texture_cache.insert(idx, tex);
                                    }
                                }
                            }
                        }
                    }

                    // Animation update: smooth exponential easing
                    let dt = app.last_frame_time.elapsed().as_secs_f32().min(0.1);
                    app.last_frame_time = Instant::now();

                    let diff = app.target_scroll - app.current_scroll;
                    let is_animating = diff.abs() > 0.0005;
                    if is_animating {
                        let factor = 1.0 - (-18.0 * dt).exp();
                        app.current_scroll += diff * factor;
                    } else {
                        app.current_scroll = app.target_scroll;
                    }

                    // Ensure smooth animation and continue refreshing while textures are loading
                    let has_missing_visible_textures = (center.saturating_sub(4)
                        ..=(center + 4).min(count.saturating_sub(1)))
                        .any(|i| match mode {
                            AppMode::Queue => !r_state.queue_texture_cache.contains_key(&i),
                            AppMode::Playlists => !r_state.playlist_texture_cache.contains_key(&i),
                            AppMode::Folders => !r_state.browser_texture_cache.contains_key(&i),
                        });

                    if (is_animating || has_missing_visible_textures)
                        && let Some(id) = unit_id
                    {
                        state.request_refresh(id, RefreshRequest::NextFrame);
                    }

                    // Uniforms update
                    let uniforms = Uniforms {
                        screen_size: [sw, sh],
                        _pad: [0.0, 0.0],
                        bg_color: app.config.colors.background_color.to_f32_array(),
                    };
                    r_state.queue.write_buffer(
                        &r_state.uniform_buffer,
                        0,
                        bytemuck::bytes_of(&uniforms),
                    );

                    // Update overlay UI
                    if app.overlay_dirty {
                        let current_track = match app.mode {
                            AppMode::Queue => {
                                app.tracks.read().unwrap().get(app.target_index).cloned()
                            }
                            _ => None,
                        };
                        let current_playlist = match app.mode {
                            AppMode::Playlists => {
                                app.playlists.read().unwrap().get(app.target_index).cloned()
                            }
                            _ => None,
                        };
                        let current_browser_item = match app.mode {
                            AppMode::Folders => app
                                .browser_items
                                .read()
                                .unwrap()
                                .get(app.target_index)
                                .cloned(),
                            _ => None,
                        };

                        let overlay_img = render_ui_overlay(
                            *width,
                            *height,
                            app.mode,
                            app.target_index,
                            count,
                            current_track.as_ref(),
                            current_playlist.as_ref(),
                            current_browser_item.as_ref(),
                            &app.browser_dir,
                            app.save_input.as_deref(),
                            app.delete_confirm.as_deref(),
                            app.overwrite_confirm.as_deref(),
                            app.status.state,
                            &app.config,
                            &app.font_mgr,
                        );
                        r_state.update_overlay(&overlay_img);
                        app.overlay_dirty = false;
                    }

                    // Geometry computation for square parallelogram cards (1:1 aspect ratio)
                    let cot75: f32 = 0.267_949_2;
                    let base_h = (sh * 0.45).clamp(240.0, 800.0);
                    let base_w = base_h; // 1:1 square parallelogram for album covers!
                    let gap = 48.0;
                    let step = base_w + gap;
                    let cy = sh * 0.5;

                    let mut vertices: Vec<CardVertex> = Vec::new();
                    let mut draw_commands: Vec<(usize, u32, u32)> = Vec::new();

                    for i in 0..count {
                        let t = i as f32 - app.current_scroll;
                        let cx = sw * 0.5 + t * step;

                        let bound_w = base_w + base_h * cot75;
                        if cx + bound_w * 0.6 < 0.0 || cx - bound_w * 0.6 > sw {
                            continue;
                        }

                        // Pre-request texture if not yet in cache
                        match app.mode {
                            AppMode::Queue => {
                                if let Some(loader) = &mut app.loader
                                    && !r_state.queue_texture_cache.contains_key(&i)
                                {
                                    loader.request_queue(i);
                                }
                            }
                            AppMode::Playlists => {
                                if let Some(loader) = &mut app.loader
                                    && !r_state.playlist_texture_cache.contains_key(&i)
                                {
                                    loader.request_playlist(i);
                                }
                            }
                            AppMode::Folders => {
                                if let Some(loader) = &mut app.loader
                                    && !r_state.browser_texture_cache.contains_key(&i)
                                {
                                    loader.request_browser(i);
                                }
                            }
                        }

                        let dist_center = t.abs();
                        let scale = 1.0 + 0.06 * (1.0 - dist_center.min(1.0));
                        let card_h = base_h * scale;
                        let card_w = base_w * scale;
                        let delta_x = card_h * cot75;

                        let base_unselected_opacity = app.config.colors.unselected_opacity;
                        let opacity = (1.0
                            - (1.0 - base_unselected_opacity) * (dist_center / 2.0).min(1.0))
                        .max(0.15);

                        let (border_color, border_width) = if i == app.target_index {
                            (
                                app.config.colors.selected_border_color.to_f32_array(),
                                app.config.colors.selected_border_width * scale,
                            )
                        } else {
                            // If in Queue mode, check if track is current playing track
                            let is_current = if app.mode == AppMode::Queue {
                                app.tracks
                                    .read()
                                    .unwrap()
                                    .get(i)
                                    .map(|t| t.is_current)
                                    .unwrap_or(false)
                            } else {
                                false
                            };

                            if is_current {
                                (
                                    app.config.colors.accent_color.to_f32_array(),
                                    app.config.colors.selected_border_width * 0.8,
                                )
                            } else {
                                (
                                    app.config.colors.card_border_color.to_f32_array(),
                                    app.config.colors.card_border_width,
                                )
                            }
                        };

                        let tl_local = [delta_x * 0.5 - card_w * 0.5, -card_h * 0.5];
                        let tr_local = [delta_x * 0.5 + card_w * 0.5, -card_h * 0.5];
                        let br_local = [-delta_x * 0.5 + card_w * 0.5, card_h * 0.5];
                        let bl_local = [-delta_x * 0.5 - card_w * 0.5, card_h * 0.5];

                        let tl_pos = [cx + tl_local[0], cy + tl_local[1]];
                        let tr_pos = [cx + tr_local[0], cy + tr_local[1]];
                        let br_pos = [cx + br_local[0], cy + br_local[1]];
                        let bl_pos = [cx + bl_local[0], cy + bl_local[1]];

                        let (tw, th) = match app.mode {
                            AppMode::Queue => {
                                if let Some(tex) = r_state.queue_texture_cache.get(&i) {
                                    (tex.width as f32, tex.height as f32)
                                } else {
                                    (
                                        r_state.fallback_texture.width as f32,
                                        r_state.fallback_texture.height as f32,
                                    )
                                }
                            }
                            AppMode::Playlists => {
                                if let Some(tex) = r_state.playlist_texture_cache.get(&i) {
                                    (tex.width as f32, tex.height as f32)
                                } else {
                                    (
                                        r_state.fallback_texture.width as f32,
                                        r_state.fallback_texture.height as f32,
                                    )
                                }
                            }
                            AppMode::Folders => {
                                if let Some(tex) = r_state.browser_texture_cache.get(&i) {
                                    (tex.width as f32, tex.height as f32)
                                } else {
                                    (
                                        r_state.fallback_texture.width as f32,
                                        r_state.fallback_texture.height as f32,
                                    )
                                }
                            }
                        };

                        let scale_cov = (card_h / th).max((card_w + delta_x) / tw);
                        let eff_w = tw * scale_cov;
                        let eff_h = th * scale_cov;

                        let calc_uv = |local: [f32; 2]| -> [f32; 2] {
                            [
                                (0.5 + local[0] / eff_w).clamp(0.0, 1.0),
                                (0.5 + local[1] / eff_h).clamp(0.0, 1.0),
                            ]
                        };

                        let v_tl = CardVertex {
                            pos: tl_pos,
                            uv: calc_uv(tl_local),
                            local_pos: tl_local,
                            card_size: [card_w, card_h],
                            border_color,
                            border_width,
                            opacity,
                        };
                        let v_tr = CardVertex {
                            pos: tr_pos,
                            uv: calc_uv(tr_local),
                            local_pos: tr_local,
                            card_size: [card_w, card_h],
                            border_color,
                            border_width,
                            opacity,
                        };
                        let v_br = CardVertex {
                            pos: br_pos,
                            uv: calc_uv(br_local),
                            local_pos: br_local,
                            card_size: [card_w, card_h],
                            border_color,
                            border_width,
                            opacity,
                        };
                        let v_bl = CardVertex {
                            pos: bl_pos,
                            uv: calc_uv(bl_local),
                            local_pos: bl_local,
                            card_size: [card_w, card_h],
                            border_color,
                            border_width,
                            opacity,
                        };

                        let v_start = vertices.len() as u32;
                        vertices.extend_from_slice(&[v_tl, v_tr, v_br, v_tl, v_br, v_bl]);
                        draw_commands.push((i, v_start, 6));
                    }

                    if !vertices.is_empty() {
                        r_state.queue.write_buffer(
                            &r_state.card_vertex_buffer,
                            0,
                            bytemuck::cast_slice(&vertices),
                        );
                    }

                    // Render pass execution
                    let surface_texture = match r_state.surface.get_current_texture() {
                        Ok(t) => t,
                        Err(wgpu::SurfaceError::Outdated) => {
                            r_state.surface.configure(&r_state.device, &r_state.config);
                            return ReturnData::None;
                        }
                        Err(e) => {
                            eprintln!("Surface error: {:?}", e);
                            return ReturnData::None;
                        }
                    };

                    let view = surface_texture
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());

                    let mut encoder =
                        r_state
                            .device
                            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("smpd-frame-encoder"),
                            });

                    {
                        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("smpd-render-pass"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        });

                        // 1. Draw Background vignette
                        rpass.set_pipeline(&r_state.bg_pipeline);
                        rpass.set_bind_group(0, &r_state.bg_bind_group, &[]);
                        rpass.draw(0..3, 0..1);

                        // 2. Draw Cards
                        rpass.set_pipeline(&r_state.card_pipeline);
                        rpass.set_bind_group(0, &r_state.card_global_bind_group, &[]);
                        rpass.set_vertex_buffer(0, r_state.card_vertex_buffer.slice(..));

                        for (card_idx, v_start, v_count) in &draw_commands {
                            let bind_group = match app.mode {
                                AppMode::Queue => {
                                    if let Some(tex) = r_state.queue_texture_cache.get(card_idx) {
                                        &tex.bind_group
                                    } else {
                                        &r_state.fallback_texture.bind_group
                                    }
                                }
                                AppMode::Playlists => {
                                    if let Some(tex) = r_state.playlist_texture_cache.get(card_idx)
                                    {
                                        &tex.bind_group
                                    } else {
                                        &r_state.fallback_texture.bind_group
                                    }
                                }
                                AppMode::Folders => {
                                    if let Some(tex) = r_state.browser_texture_cache.get(card_idx) {
                                        &tex.bind_group
                                    } else {
                                        &r_state.fallback_texture.bind_group
                                    }
                                }
                            };
                            rpass.set_bind_group(1, bind_group, &[]);
                            rpass.draw(*v_start..(*v_start + *v_count), 0..1);
                        }

                        // 3. Draw Overlay
                        if (app.config.ui.show_top_bar || app.config.ui.show_bottom_bar)
                            && let Some(overlay_tex) = &r_state.overlay_texture
                        {
                            rpass.set_pipeline(&r_state.overlay_pipeline);
                            rpass.set_bind_group(0, &overlay_tex.bind_group, &[]);
                            rpass.draw(0..3, 0..1);
                        }
                    }

                    r_state.queue.submit(Some(encoder.finish()));
                    surface_texture.present();
                }

                DispatchMessage::ModifiersChanged(modifier) => {
                    app.modifiers = *modifier;
                }

                DispatchMessage::KeyboardInput { event, .. } => {
                    if event.state != ElementState::Pressed {
                        return ReturnData::None;
                    }

                    // 1. Delete playlist confirmation dialog mode
                    if app.delete_confirm.is_some() {
                        match event.physical_key {
                            PhysicalKey::Code(KeyCode::KeyY)
                            | PhysicalKey::Code(KeyCode::Enter)
                            | PhysicalKey::Code(KeyCode::NumpadEnter) => {
                                if let Some(pl_name) = app.delete_confirm.take() {
                                    let _ = app.mpd.delete_playlist(&pl_name);
                                    app.reload_stored_playlists();
                                    app.overlay_dirty = true;
                                    if let Some(id) = unit_id {
                                        state.request_refresh(id, RefreshRequest::NextFrame);
                                    }
                                }
                            }
                            PhysicalKey::Code(KeyCode::KeyN)
                            | PhysicalKey::Code(KeyCode::Escape) => {
                                app.delete_confirm = None;
                                app.overlay_dirty = true;
                                if let Some(id) = unit_id {
                                    state.request_refresh(id, RefreshRequest::NextFrame);
                                }
                            }
                            _ => {}
                        }
                        return ReturnData::None;
                    }

                    // 2. Overwrite playlist confirmation dialog mode
                    if app.overwrite_confirm.is_some() {
                        match event.physical_key {
                            PhysicalKey::Code(KeyCode::KeyY)
                            | PhysicalKey::Code(KeyCode::Enter)
                            | PhysicalKey::Code(KeyCode::NumpadEnter) => {
                                if let Some(pl_name) = app.overwrite_confirm.take() {
                                    let _ = app.mpd.overwrite_playlist(&pl_name);
                                    app.reload_stored_playlists();
                                    app.overlay_dirty = true;
                                    if let Some(id) = unit_id {
                                        state.request_refresh(id, RefreshRequest::NextFrame);
                                    }
                                }
                            }
                            PhysicalKey::Code(KeyCode::KeyN)
                            | PhysicalKey::Code(KeyCode::Escape) => {
                                app.overwrite_confirm = None;
                                app.overlay_dirty = true;
                                if let Some(id) = unit_id {
                                    state.request_refresh(id, RefreshRequest::NextFrame);
                                }
                            }
                            _ => {}
                        }
                        return ReturnData::None;
                    }

                    // 3. Save playlist text input mode
                    if app.save_input.is_some() {
                        match event.physical_key {
                            PhysicalKey::Code(KeyCode::Escape) => {
                                app.save_input = None;
                                app.overlay_dirty = true;
                                if let Some(id) = unit_id {
                                    state.request_refresh(id, RefreshRequest::NextFrame);
                                }
                            }
                            PhysicalKey::Code(KeyCode::Enter)
                            | PhysicalKey::Code(KeyCode::NumpadEnter) => {
                                if let Some(buf) = app.save_input.take() {
                                    let trimmed = buf.trim();
                                    if !trimmed.is_empty() {
                                        let exists = app
                                            .playlists
                                            .read()
                                            .unwrap()
                                            .iter()
                                            .any(|p| p.name == trimmed);

                                        if exists {
                                            // Ask confirmation to overwrite existing playlist
                                            app.overwrite_confirm = Some(trimmed.to_string());
                                        } else {
                                            let _ = app.mpd.save_playlist(trimmed);
                                            app.reload_stored_playlists();
                                        }
                                    }
                                    app.overlay_dirty = true;
                                    if let Some(id) = unit_id {
                                        state.request_refresh(id, RefreshRequest::NextFrame);
                                    }
                                }
                            }
                            PhysicalKey::Code(KeyCode::Backspace) => {
                                if let Some(ref mut buf) = app.save_input {
                                    buf.pop();
                                    app.overlay_dirty = true;
                                    if let Some(id) = unit_id {
                                        state.request_refresh(id, RefreshRequest::NextFrame);
                                    }
                                }
                            }
                            _ => {
                                if let Some(ref mut buf) = app.save_input
                                    && let Key::Character(ref s) = event.logical_key
                                    && !s.chars().any(|c| c.is_control())
                                {
                                    buf.push_str(s);
                                    app.overlay_dirty = true;
                                    if let Some(id) = unit_id {
                                        state.request_refresh(id, RefreshRequest::NextFrame);
                                    }
                                }
                            }
                        }
                        return ReturnData::None;
                    }

                    // 3. Normal shortcut handling strictly by PHYSICAL KEY (scancodes)
                    let is_shift = app.modifiers.shift_key()
                        || match event.logical_key {
                            Key::Character(ref s) => s.chars().any(|c| c.is_uppercase()),
                            _ => false,
                        };

                    match event.physical_key {
                        // Mode switching:
                        PhysicalKey::Code(KeyCode::Digit1) => {
                            app.switch_to_mode(AppMode::Queue);
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }
                        PhysicalKey::Code(KeyCode::Digit2) => {
                            app.switch_to_mode(AppMode::Playlists);
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }
                        PhysicalKey::Code(KeyCode::Digit3) => {
                            app.switch_to_mode(AppMode::Folders);
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }
                        PhysicalKey::Code(KeyCode::Tab) => {
                            let next = match app.mode {
                                AppMode::Queue => AppMode::Playlists,
                                AppMode::Playlists => AppMode::Folders,
                                AppMode::Folders => AppMode::Queue,
                            };
                            app.switch_to_mode(next);
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }

                        // Navigation:
                        PhysicalKey::Code(KeyCode::ArrowLeft)
                        | PhysicalKey::Code(KeyCode::KeyH) => {
                            app.select_prev();
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }
                        PhysicalKey::Code(KeyCode::ArrowRight)
                        | PhysicalKey::Code(KeyCode::KeyL) => {
                            app.select_next();
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }
                        PhysicalKey::Code(KeyCode::Home) => {
                            app.target_index = 0;
                            app.target_scroll = 0.0;
                            app.overlay_dirty = true;
                            app.request_adjacent_textures();
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }
                        PhysicalKey::Code(KeyCode::End) => {
                            let count = app.current_count();
                            if count > 0 {
                                app.target_index = count - 1;
                                app.target_scroll = app.target_index as f32;
                                app.overlay_dirty = true;
                                app.request_adjacent_textures();
                                if let Some(id) = unit_id {
                                    state.request_refresh(id, RefreshRequest::NextFrame);
                                }
                            }
                        }

                        // Confirm / Action:
                        PhysicalKey::Code(KeyCode::Enter)
                        | PhysicalKey::Code(KeyCode::NumpadEnter) => {
                            app.confirm_selection();
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }

                        // Exit:
                        PhysicalKey::Code(KeyCode::Escape) | PhysicalKey::Code(KeyCode::KeyQ) => {
                            std::process::exit(0);
                        }

                        // Space: Toggle Play / Pause in Queue mode
                        PhysicalKey::Code(KeyCode::Space) if app.mode == AppMode::Queue => {
                            let _ = app.mpd.toggle_pause();
                            app.refresh_status();
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }

                        // S: Save current active playlist prompt (starts empty)
                        PhysicalKey::Code(KeyCode::KeyS) => {
                            app.save_input = Some(String::new());
                            app.overlay_dirty = true;
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }

                        // C: Clear queue if Shift+C, or center on playing track if no Shift
                        PhysicalKey::Code(KeyCode::KeyC) if app.mode == AppMode::Queue => {
                            if is_shift {
                                let _ = app.mpd.clear_queue();
                                app.reload_playlist();
                                if let Some(ref mut rs) = app.render_state {
                                    rs.queue_texture_cache.clear();
                                }
                                app.overlay_dirty = true;
                            } else {
                                let curr_idx =
                                    app.tracks.read().unwrap().iter().position(|t| t.is_current);
                                if let Some(idx) = curr_idx {
                                    app.target_index = idx;
                                    app.target_scroll = idx as f32;
                                    app.overlay_dirty = true;
                                    app.request_adjacent_textures();
                                }
                            }
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }

                        // X: Clear queue in Queue mode
                        PhysicalKey::Code(KeyCode::KeyX) if app.mode == AppMode::Queue => {
                            let _ = app.mpd.clear_queue();
                            app.reload_playlist();
                            if let Some(ref mut rs) = app.render_state {
                                rs.queue_texture_cache.clear();
                            }
                            app.overlay_dirty = true;
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }

                        // D or Delete:
                        // - In Queue mode: delete track from current playlist
                        // - In Playlists mode: delete stored playlist with confirmation
                        PhysicalKey::Code(KeyCode::KeyD) | PhysicalKey::Code(KeyCode::Delete) => {
                            match app.mode {
                                AppMode::Queue => {
                                    let track_opt =
                                        app.tracks.read().unwrap().get(app.target_index).cloned();
                                    if let Some(track) = track_opt {
                                        let _ = app.mpd.delete_id(track.id);
                                        app.reload_playlist();
                                        if let Some(id) = unit_id {
                                            state.request_refresh(id, RefreshRequest::NextFrame);
                                        }
                                    }
                                }
                                AppMode::Playlists => {
                                    let pl_opt = app
                                        .playlists
                                        .read()
                                        .unwrap()
                                        .get(app.target_index)
                                        .cloned();
                                    if let Some(pl) = pl_opt {
                                        app.delete_confirm = Some(pl.name);
                                        app.overlay_dirty = true;
                                        if let Some(id) = unit_id {
                                            state.request_refresh(id, RefreshRequest::NextFrame);
                                        }
                                    }
                                }
                                AppMode::Folders => {}
                            }
                        }

                        // A: In Folders mode:
                        // - If Shift (A): Add entire directory immediately
                        // - If no Shift (a): Add focused item (file or folder)
                        PhysicalKey::Code(KeyCode::KeyA) if app.mode == AppMode::Folders => {
                            if is_shift {
                                let dir_to_add = if !app.browser_dir.is_empty() {
                                    app.browser_dir.clone()
                                } else {
                                    app.browser_items
                                        .read()
                                        .unwrap()
                                        .get(app.target_index)
                                        .map(|it| it.path.clone())
                                        .unwrap_or_default()
                                };
                                if !dir_to_add.is_empty() {
                                    let _ = app.mpd.add_to_queue(&dir_to_add);
                                    app.reload_playlist();
                                }
                            } else {
                                let item_opt = app
                                    .browser_items
                                    .read()
                                    .unwrap()
                                    .get(app.target_index)
                                    .cloned();
                                if let Some(item) = item_opt {
                                    let _ = app.mpd.add_to_queue(&item.path);
                                    app.reload_playlist();
                                }
                            }
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }

                        // U or Backspace: Go up directory in Folders mode
                        PhysicalKey::Code(KeyCode::KeyU)
                        | PhysicalKey::Code(KeyCode::Backspace)
                            if app.mode == AppMode::Folders =>
                        {
                            let parent = Path::new(&app.browser_dir)
                                .parent()
                                .and_then(|p| p.to_str())
                                .unwrap_or("")
                                .to_string();
                            app.browser_dir = parent;
                            app.reload_browser_items();
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }

                        // N: Next track in Queue mode
                        PhysicalKey::Code(KeyCode::KeyN) if app.mode == AppMode::Queue => {
                            let _ = app.mpd.next();
                            app.refresh_status();
                            let curr_idx =
                                app.tracks.read().unwrap().iter().position(|t| t.is_current);

                            if let Some(idx) = curr_idx {
                                app.target_index = idx;
                                app.target_scroll = idx as f32;
                                app.overlay_dirty = true;
                                app.request_adjacent_textures();
                            }
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }

                        // P: Previous track in Queue mode
                        PhysicalKey::Code(KeyCode::KeyP) if app.mode == AppMode::Queue => {
                            let _ = app.mpd.previous();
                            app.refresh_status();
                            let curr_idx =
                                app.tracks.read().unwrap().iter().position(|t| t.is_current);

                            if let Some(idx) = curr_idx {
                                app.target_index = idx;
                                app.target_scroll = idx as f32;
                                app.overlay_dirty = true;
                                app.request_adjacent_textures();
                            }
                            if let Some(id) = unit_id {
                                state.request_refresh(id, RefreshRequest::NextFrame);
                            }
                        }

                        _ => {}
                    }
                }

                DispatchMessage::MouseMotion {
                    surface_x,
                    surface_y,
                    ..
                } => {
                    app.cursor_pos = [*surface_x as f32, *surface_y as f32];
                }

                DispatchMessage::MouseButton {
                    state: btn_state,
                    button,
                    ..
                } => {
                    if *btn_state == WEnum::Value(ButtonState::Pressed) && *button == 272 {
                        let (sw, sh) = (app.screen_size[0], app.screen_size[1]);
                        let cot75: f32 = 0.267_949_2;
                        let base_h = (sh * 0.45).clamp(240.0, 800.0);
                        let base_w = base_h; // 1:1 square
                        let gap = 48.0;
                        let step = base_w + gap;
                        let cy = sh * 0.5;

                        let [mx, my] = app.cursor_pos;
                        let mut clicked_idx = None;
                        let count = app.current_count();

                        for i in 0..count {
                            let t = i as f32 - app.current_scroll;
                            let cx = sw * 0.5 + t * step;
                            let px = mx - cx;
                            let py = my - cy;

                            let dist_horizontal = base_h * 0.5 - py.abs();
                            let u_slant = px + py * cot75;
                            let dist_slant = base_w * 0.5 - u_slant.abs();

                            if dist_horizontal >= 0.0 && dist_slant >= 0.0 {
                                clicked_idx = Some(i);
                                break;
                            }
                        }

                        if let Some(idx) = clicked_idx {
                            if idx == app.target_index {
                                app.confirm_selection();
                                if let Some(id) = unit_id {
                                    state.request_refresh(id, RefreshRequest::NextFrame);
                                }
                            } else {
                                app.target_index = idx;
                                app.target_scroll = idx as f32;
                                app.overlay_dirty = true;
                                app.request_adjacent_textures();
                                if let Some(id) = unit_id {
                                    state.request_refresh(id, RefreshRequest::NextFrame);
                                }
                            }
                        }
                    }
                }

                DispatchMessage::Axis { vertical, .. } => {
                    let scroll_y = if vertical.discrete != 0 {
                        vertical.discrete as f64
                    } else {
                        vertical.absolute
                    };
                    if scroll_y > 0.1 {
                        app.select_next();
                        if let Some(id) = unit_id {
                            state.request_refresh(id, RefreshRequest::NextFrame);
                        }
                    } else if scroll_y < -0.1 {
                        app.select_prev();
                        if let Some(id) = unit_id {
                            state.request_refresh(id, RefreshRequest::NextFrame);
                        }
                    }
                }

                DispatchMessage::Closed => {
                    std::process::exit(0);
                }

                _ => {}
            }
        }

        ReturnData::None
    });
}

use ab_glyph::{Font, FontArc, PxScale, ScaleFont, point};
use font8x8::{BASIC_FONTS, UnicodeFonts};
use image::{Rgba, RgbaImage};
use std::path::PathBuf;

use crate::config::Config;
use crate::mpd::{BrowserItem, PlayState, PlaylistInfo, Track};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppMode {
    Queue,
    Playlists,
    Folders,
}

pub struct FontManager {
    pub custom_font: Option<FontArc>,
}

pub fn resolve_font(spec: &str) -> Option<PathBuf> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. Direct path or ~ expansion
    let path = if let Some(stripped) = trimmed.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(stripped)
        } else {
            PathBuf::from(trimmed)
        }
    } else {
        PathBuf::from(trimmed)
    };

    if path.is_file() {
        return Some(path);
    }

    // 2. Query system fontconfig via fc-match (standard on Linux)
    if let Ok(output) = std::process::Command::new("fc-match")
        .args(["-f", "%{file}", trimmed])
        .output()
        && output.status.success()
    {
        let matched_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !matched_path.is_empty() {
            let p = PathBuf::from(matched_path);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    // 3. Fallback scan in common font directories
    let mut search_dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        let h = PathBuf::from(home);
        search_dirs.push(h.join(".local/share/fonts"));
        search_dirs.push(h.join(".fonts"));
    }
    search_dirs.push(PathBuf::from("/usr/share/fonts"));
    search_dirs.push(PathBuf::from("/usr/local/share/fonts"));

    let lower_spec = trimmed.to_ascii_lowercase().replace(' ', "");
    for dir in search_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file()
                    && let Some(ext) = p.extension().and_then(|e| e.to_str())
                {
                    let lower_ext = ext.to_ascii_lowercase();
                    if (lower_ext == "ttf" || lower_ext == "otf")
                        && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
                    {
                        let clean = stem.to_ascii_lowercase().replace(['-', '_', ' '], "");
                        if clean.contains(&lower_spec) {
                            return Some(p);
                        }
                    }
                }
            }
        }
    }

    None
}

impl FontManager {
    pub fn new(font_spec: Option<&str>) -> Self {
        let requested = font_spec
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("monospace");

        let load_font = |spec: &str| -> Option<FontArc> {
            let path = resolve_font(spec)?;
            match std::fs::read(&path) {
                Ok(bytes) => match FontArc::try_from_vec(bytes) {
                    Ok(font) => Some(font),
                    Err(e) => {
                        eprintln!(
                            "smpd: warning: failed to parse font at {}: {}",
                            path.display(),
                            e
                        );
                        None
                    }
                },
                Err(e) => {
                    eprintln!(
                        "smpd: warning: could not read font at {}: {}",
                        path.display(),
                        e
                    );
                    None
                }
            }
        };

        let custom_font = load_font(requested).or_else(|| {
            if requested != "monospace" {
                load_font("monospace")
            } else {
                None
            }
        });

        Self { custom_font }
    }
}

pub fn render_text(
    img: &mut RgbaImage,
    font_mgr: &FontManager,
    text: &str,
    mut x: f32,
    center_y: f32,
    font_size: f32,
    color: [u8; 4],
) -> f32 {
    if let Some(ref font) = font_mgr.custom_font {
        let scale = PxScale::from(font_size);
        let scaled_font = font.as_scaled(scale);
        let baseline_y = center_y + (scaled_font.ascent() + scaled_font.descent()) * 0.5;
        let mut last_glyph_id = None;

        for ch in text.chars() {
            if ch.is_control() {
                continue;
            }
            let glyph_id = scaled_font.glyph_id(ch);
            if let Some(last) = last_glyph_id {
                x += scaled_font.kern(last, glyph_id);
            }
            last_glyph_id = Some(glyph_id);

            let mut glyph = scaled_font.scaled_glyph(ch);
            glyph.position = point(x, baseline_y);

            if let Some(outlined) = font.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, coverage| {
                    let px = (bounds.min.x as i32 + gx as i32) as u32;
                    let py = (bounds.min.y as i32 + gy as i32) as u32;
                    if px < img.width() && py < img.height() {
                        let old = img.get_pixel(px, py).0;
                        let a_src = (color[3] as f32 / 255.0) * coverage;
                        let a_dst = old[3] as f32 / 255.0;
                        let a_out = a_src + a_dst * (1.0 - a_src);
                        if a_out > 0.0 {
                            let blend = |c_s: u8, c_d: u8| -> u8 {
                                let s = c_s as f32 / 255.0;
                                let d = c_d as f32 / 255.0;
                                let res = (s * a_src + d * a_dst * (1.0 - a_src)) / a_out;
                                (res * 255.0).clamp(0.0, 255.0) as u8
                            };
                            let new_color = [
                                blend(color[0], old[0]),
                                blend(color[1], old[1]),
                                blend(color[2], old[2]),
                                (a_out * 255.0) as u8,
                            ];
                            img.put_pixel(px, py, Rgba(new_color));
                        }
                    }
                });
            }
            x += scaled_font.h_advance(glyph_id);
        }
        x
    } else {
        // Fallback to bitmap font
        let scale = (font_size / 8.0).round().max(1.0) as u32;
        let mut cur_x = x as i32;
        let start_y = (center_y - (8.0 * scale as f32) * 0.5).round() as i32;

        for ch in text.chars() {
            if ch == '\n' {
                continue;
            }
            if let Some(glyph) = BASIC_FONTS.get(ch) {
                for (gy, byte) in glyph.iter().enumerate() {
                    for gx in 0..8 {
                        if (byte & (1 << gx)) != 0 {
                            for sy in 0..scale {
                                for sx in 0..scale {
                                    let px = cur_x + gx * (scale as i32) + (sx as i32);
                                    let py = start_y + (gy as i32) * (scale as i32) + (sy as i32);
                                    if px >= 0
                                        && px < img.width() as i32
                                        && py >= 0
                                        && py < img.height() as i32
                                    {
                                        let old = img.get_pixel(px as u32, py as u32).0;
                                        let a_src = color[3] as f32 / 255.0;
                                        let a_dst = old[3] as f32 / 255.0;
                                        let a_out = a_src + a_dst * (1.0 - a_src);
                                        if a_out > 0.0 {
                                            let blend = |c_s: u8, c_d: u8| -> u8 {
                                                let s = c_s as f32 / 255.0;
                                                let d = c_d as f32 / 255.0;
                                                let res =
                                                    (s * a_src + d * a_dst * (1.0 - a_src)) / a_out;
                                                (res * 255.0).clamp(0.0, 255.0) as u8
                                            };
                                            let new_color = [
                                                blend(color[0], old[0]),
                                                blend(color[1], old[1]),
                                                blend(color[2], old[2]),
                                                (a_out * 255.0) as u8,
                                            ];
                                            img.put_pixel(px as u32, py as u32, Rgba(new_color));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            cur_x += (8 * scale) as i32;
        }
        cur_x as f32
    }
}

pub fn get_text_dimensions(font_mgr: &FontManager, text: &str, font_size: f32) -> (f32, f32) {
    if let Some(ref font) = font_mgr.custom_font {
        let scale = PxScale::from(font_size);
        let scaled_font = font.as_scaled(scale);
        let mut width = 0.0;
        let mut last_glyph_id = None;

        for ch in text.chars() {
            if ch.is_control() {
                continue;
            }
            let glyph_id = scaled_font.glyph_id(ch);
            if let Some(last) = last_glyph_id {
                width += scaled_font.kern(last, glyph_id);
            }
            last_glyph_id = Some(glyph_id);
            width += scaled_font.h_advance(glyph_id);
        }
        let height = scaled_font.height();
        (width, height)
    } else {
        let scale = (font_size / 8.0).round().max(1.0) as u32;
        let width = (text.chars().count() as u32 * 8 * scale) as f32;
        let height = (8 * scale) as f32;
        (width, height)
    }
}

pub fn get_icon<'a>(font: Option<&FontArc>, preferred: &'a str, fallback: &'a str) -> &'a str {
    if let Some(f) = font
        && preferred.chars().all(|c| f.glyph_id(c).0 != 0)
    {
        return preferred;
    }
    fallback
}

#[allow(clippy::too_many_arguments)]
pub fn draw_slanted_pill(
    img: &mut RgbaImage,
    center_x: i32,
    center_y: i32,
    width: i32,
    height: i32,
    bg_color: [u8; 4],
    border_color: [u8; 4],
    border_width: f32,
) {
    let cot75: f32 = 0.267_949_2;
    let sin75: f32 = 0.965_925_8;
    let delta_x = (height as f32) * cot75;

    let half_w = width as f32 * 0.5;
    let half_h = height as f32 * 0.5;

    let min_x = (center_x as f32 - half_w - delta_x * 0.5 - 2.0).floor() as i32;
    let max_x = (center_x as f32 + half_w + delta_x * 0.5 + 2.0).ceil() as i32;
    let min_y = (center_y as f32 - half_h - 2.0).floor() as i32;
    let max_y = (center_y as f32 + half_h + 2.0).ceil() as i32;

    for y in min_y..=max_y {
        if y < 0 || y >= img.height() as i32 {
            continue;
        }
        let py = y as f32 - center_y as f32;
        let dist_y = half_h - py.abs();
        if dist_y < -1.0 {
            continue;
        }

        for x in min_x..=max_x {
            if x < 0 || x >= img.width() as i32 {
                continue;
            }
            let px = x as f32 - center_x as f32;
            let u_slant = px + py * cot75;
            let dist_slant = (half_w - u_slant.abs()) * sin75;

            let dist = dist_y.min(dist_slant);
            if dist < -0.5 {
                continue;
            }

            let edge_factor = (dist + 0.5).clamp(0.0, 1.0);
            let border_factor = if border_width > 0.0 {
                (1.0 - (dist - (border_width - 0.5)).clamp(0.0, 1.0)).max(0.0)
            } else {
                0.0
            };

            let bf = border_factor * (border_color[3] as f32 / 255.0);
            let r = (bg_color[0] as f32 * (1.0 - bf) + border_color[0] as f32 * bf) as u8;
            let g = (bg_color[1] as f32 * (1.0 - bf) + border_color[1] as f32 * bf) as u8;
            let b = (bg_color[2] as f32 * (1.0 - bf) + border_color[2] as f32 * bf) as u8;
            let a = ((bg_color[3] as f32 * (1.0 - bf) + border_color[3] as f32 * bf) * edge_factor)
                as u8;

            img.put_pixel(x as u32, y as u32, Rgba([r, g, b, a]));
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render_ui_overlay(
    screen_w: u32,
    screen_h: u32,
    mode: AppMode,
    current_index: usize,
    total_count: usize,
    current_track: Option<&Track>,
    current_playlist: Option<&PlaylistInfo>,
    current_browser_item: Option<&BrowserItem>,
    browser_dir: &str,
    save_input: Option<&str>,
    delete_confirm: Option<&str>,
    overwrite_confirm: Option<&str>,
    notification: Option<&str>,
    state: PlayState,
    config: &Config,
    font_mgr: &FontManager,
) -> RgbaImage {
    let mut img = RgbaImage::new(screen_w, screen_h);

    if screen_w < 100 || screen_h < 100 {
        return img;
    }

    let pill_bg = config.colors.panel_bg_color.to_rgba8();
    let pill_border = config.colors.panel_border_color.to_rgba8();
    let text_white = config.colors.text_color.to_rgba8();
    let text_accent = config.colors.accent_color.to_rgba8();
    let text_dim = config.colors.dim_text_color.to_rgba8();

    let font_size = config.ui.font_size;
    let font_ref = font_mgr.custom_font.as_ref();

    let ic_save = get_icon(font_ref, "\u{f0c7}", "[Save]");
    let ic_trash = get_icon(font_ref, "\u{f014}", "[Del]");
    let ic_play = get_icon(font_ref, "\u{f04b}", "▶");
    let ic_pause = get_icon(font_ref, "\u{f04c}", "||");
    let ic_stop = get_icon(font_ref, "\u{f04d}", "■");
    let ic_music = get_icon(font_ref, "\u{f001}", "•");
    let ic_playlist = get_icon(font_ref, "\u{f025}", "•");
    let ic_folder = get_icon(font_ref, "\u{f07b}", "/");
    let ic_refresh = get_icon(font_ref, "\u{f021}", "[Update]");
    let ic_prev = get_icon(font_ref, "\u{f048}", "<-");
    let ic_next = get_icon(font_ref, "\u{f051}", "->");
    let ic_add = get_icon(font_ref, "\u{f067}", "+");
    let ic_close = get_icon(font_ref, "\u{f00d}", "Esc");

    // 1. Top Header: [ Tabs / Modes   Active Item Information ] OR [ Save Prompt ] OR [ Delete Prompt ]
    if config.ui.show_top_bar {
        if let Some(del_name) = delete_confirm {
            // Interactive Delete Confirmation prompt
            let prompt_text = format!("{}  Delete playlist '", ic_trash);
            let name_text = del_name;
            let hint_text = "'?   [y/Enter: Confirm]  [n/Esc: Cancel]";
            let full_line = format!("{}{}{}", prompt_text, name_text, hint_text);
            let (text_w, text_h) = get_text_dimensions(font_mgr, &full_line, font_size);

            let top_y = 52i32;
            let pill_w = (text_w as i32 + 64).max(480);
            let pill_h = (text_h as i32 + 24).max(46);
            let top_center_x = (screen_w / 2) as i32;

            draw_slanted_pill(
                &mut img,
                top_center_x,
                top_y,
                pill_w,
                pill_h,
                pill_bg,
                [255, 100, 100, 220],
                2.0,
            );

            let start_x = top_center_x as f32 - (text_w * 0.5);

            let x1 = render_text(
                &mut img,
                font_mgr,
                &prompt_text,
                start_x,
                top_y as f32,
                font_size,
                [255, 110, 110, 255],
            );
            let x2 = render_text(
                &mut img,
                font_mgr,
                name_text,
                x1,
                top_y as f32,
                font_size,
                text_white,
            );
            render_text(
                &mut img,
                font_mgr,
                hint_text,
                x2,
                top_y as f32,
                font_size,
                text_accent,
            );
        } else if let Some(ovw_name) = overwrite_confirm {
            // Interactive Overwrite Confirmation prompt
            let prompt_text = format!("{}  Playlist '", ic_save);
            let name_text = ovw_name;
            let hint_text = "' already exists. Overwrite?   [y/Enter: Confirm]  [n/Esc: Cancel]";
            let full_line = format!("{}{}{}", prompt_text, name_text, hint_text);
            let (text_w, text_h) = get_text_dimensions(font_mgr, &full_line, font_size);

            let top_y = 52i32;
            let pill_w = (text_w as i32 + 64).max(520);
            let pill_h = (text_h as i32 + 24).max(46);
            let top_center_x = (screen_w / 2) as i32;

            draw_slanted_pill(
                &mut img,
                top_center_x,
                top_y,
                pill_w,
                pill_h,
                pill_bg,
                [255, 175, 50, 220],
                2.0,
            );

            let start_x = top_center_x as f32 - (text_w * 0.5);

            let x1 = render_text(
                &mut img,
                font_mgr,
                &prompt_text,
                start_x,
                top_y as f32,
                font_size,
                [255, 190, 70, 255],
            );
            let x2 = render_text(
                &mut img,
                font_mgr,
                name_text,
                x1,
                top_y as f32,
                font_size,
                text_white,
            );
            render_text(
                &mut img,
                font_mgr,
                hint_text,
                x2,
                top_y as f32,
                font_size,
                text_accent,
            );
        } else if let Some(input_buf) = save_input {
            // Interactive Save Playlist Input prompt
            let prompt_text = format!("{}  Save Playlist As: ", ic_save);
            let display_input = format!("{}_", input_buf);
            let hint_text = "   [Enter: Save, Esc: Cancel]";
            let full_line = format!("{}{}{}", prompt_text, display_input, hint_text);
            let (text_w, text_h) = get_text_dimensions(font_mgr, &full_line, font_size);

            let top_y = 52i32;
            let pill_w = (text_w as i32 + 64).max(480);
            let pill_h = (text_h as i32 + 24).max(46);
            let top_center_x = (screen_w / 2) as i32;

            draw_slanted_pill(
                &mut img,
                top_center_x,
                top_y,
                pill_w,
                pill_h,
                pill_bg,
                pill_border,
                2.0,
            );

            let start_x = top_center_x as f32 - (text_w * 0.5);

            let x1 = render_text(
                &mut img,
                font_mgr,
                &prompt_text,
                start_x,
                top_y as f32,
                font_size,
                text_accent,
            );
            let x2 = render_text(
                &mut img,
                font_mgr,
                &display_input,
                x1,
                top_y as f32,
                font_size,
                text_white,
            );
            render_text(
                &mut img,
                font_mgr,
                hint_text,
                x2,
                top_y as f32,
                font_size,
                text_dim,
            );
        } else if let Some(note) = notification {
            // Notification toast pill
            let prompt_text = format!("{}  ", ic_refresh);
            let full_line = format!("{}{}", prompt_text, note);
            let (text_w, text_h) = get_text_dimensions(font_mgr, &full_line, font_size);

            let top_y = 52i32;
            let pill_w = (text_w as i32 + 64).max(380);
            let pill_h = (text_h as i32 + 24).max(46);
            let top_center_x = (screen_w / 2) as i32;

            draw_slanted_pill(
                &mut img,
                top_center_x,
                top_y,
                pill_w,
                pill_h,
                pill_bg,
                pill_border,
                2.0,
            );

            let start_x = top_center_x as f32 - (text_w * 0.5);

            let x1 = render_text(
                &mut img,
                font_mgr,
                &prompt_text,
                start_x,
                top_y as f32,
                font_size,
                text_accent,
            );
            render_text(
                &mut img,
                font_mgr,
                note,
                x1,
                top_y as f32,
                font_size,
                text_white,
            );
        } else {
            let (tab1_str, tab1_act, tab2_str, tab2_act, tab3_str, tab3_act, icon, display_str) =
                match mode {
                    AppMode::Queue => {
                        let t1 = if total_count > 0 {
                            format!("[ 1: Queue ({}/{}) ]", current_index + 1, total_count)
                        } else {
                            "[ 1: Queue (0/0) ]".to_string()
                        };
                        let t2 = "  2: Playlists".to_string();
                        let t3 = "  3: Folders".to_string();

                        let (ic, display) = if let Some(track) = current_track {
                            let st_ic = if track.is_current {
                                match state {
                                    PlayState::Play => ic_play,
                                    PlayState::Pause => ic_pause,
                                    PlayState::Stop => ic_stop,
                                }
                            } else {
                                ic_music
                            };
                            (st_ic, format!("{} - {}", track.artist, track.title))
                        } else {
                            (ic_music, "No tracks in MPD playlist".to_string())
                        };

                        (t1, true, t2, false, t3, false, ic, display)
                    }
                    AppMode::Playlists => {
                        let t1 = "1: Queue".to_string();
                        let t2 = if total_count > 0 {
                            format!("  [ 2: Playlists ({}/{}) ]", current_index + 1, total_count)
                        } else {
                            "  [ 2: Playlists (0/0) ]".to_string()
                        };
                        let t3 = "  3: Folders".to_string();

                        let (ic, display) = if let Some(pl) = current_playlist {
                            (
                                ic_playlist,
                                format!("{} ({} tracks)", pl.name, pl.track_count),
                            )
                        } else {
                            (ic_playlist, "No saved playlists".to_string())
                        };

                        (t1, false, t2, true, t3, false, ic, display)
                    }
                    AppMode::Folders => {
                        let t1 = "1: Queue".to_string();
                        let t2 = "  2: Playlists".to_string();
                        let dir_label = if browser_dir.is_empty() {
                            "root"
                        } else {
                            browser_dir
                        };
                        let t3 = if total_count > 0 {
                            format!(
                                "  [ 3: Folders: {} ({}/{}) ]",
                                dir_label,
                                current_index + 1,
                                total_count
                            )
                        } else {
                            format!("  [ 3: Folders: {} (0/0) ]", dir_label)
                        };

                        let (ic, display) = if let Some(item) = current_browser_item {
                            if item.is_dir {
                                (
                                    ic_folder,
                                    format!("{} ({} items)", item.name, item.item_count),
                                )
                            } else {
                                (ic_music, item.name.clone())
                            }
                        } else {
                            (ic_folder, "Empty folder".to_string())
                        };

                        (t1, false, t2, false, t3, true, ic, display)
                    }
                };

            let full_top_line = format!(
                "{} {} {}   {}  {}",
                tab1_str, tab2_str, tab3_str, icon, display_str
            );
            let (text_w, text_h) = get_text_dimensions(font_mgr, &full_top_line, font_size);

            let top_y = 52i32;
            let pill_w = (text_w as i32 + 64).max(420);
            let pill_h = (text_h as i32 + 24).max(46);
            let top_center_x = (screen_w / 2) as i32;

            draw_slanted_pill(
                &mut img,
                top_center_x,
                top_y,
                pill_w,
                pill_h,
                pill_bg,
                pill_border,
                2.0,
            );

            let start_x = top_center_x as f32 - (text_w * 0.5);

            let t1_color = if tab1_act { text_accent } else { text_dim };
            let t2_color = if tab2_act { text_accent } else { text_dim };
            let t3_color = if tab3_act { text_accent } else { text_dim };

            let x1 = render_text(
                &mut img,
                font_mgr,
                &tab1_str,
                start_x,
                top_y as f32,
                font_size,
                t1_color,
            );
            let x2 = render_text(
                &mut img,
                font_mgr,
                &tab2_str,
                x1,
                top_y as f32,
                font_size,
                t2_color,
            );
            let x3 = render_text(
                &mut img,
                font_mgr,
                &tab3_str,
                x2,
                top_y as f32,
                font_size,
                t3_color,
            );
            let x4 = render_text(
                &mut img,
                font_mgr,
                &format!("   {}  ", icon),
                x3,
                top_y as f32,
                font_size,
                text_accent,
            );
            render_text(
                &mut img,
                font_mgr,
                &display_str,
                x4,
                top_y as f32,
                font_size,
                text_white,
            );
        }
    }

    // 2. Bottom Footer: Navigation hints with Nerd Font icons
    if config.ui.show_bottom_bar {
        let footer_text = if delete_confirm.is_some() {
            "Confirm deletion:   y / Enter : Delete playlist   n / Esc : Cancel".to_string()
        } else if overwrite_confirm.is_some() {
            "Confirm overwrite:   y / Enter : Overwrite playlist   n / Esc : Cancel".to_string()
        } else if save_input.is_some() {
            "Type playlist name...   Enter : Confirm Save   Esc : Cancel".to_string()
        } else {
            match mode {
                AppMode::Queue => {
                    format!(
                        "{ic_prev} h/<- Prev   {ic_next} l/-> Next   {ic_pause} Space Pause   {ic_play} Enter Play   {ic_trash} C/x Clear   {ic_save} s Save   {ic_refresh} u Update   1/2/3 Mode   {ic_close} Esc Exit"
                    )
                }
                AppMode::Playlists => {
                    format!(
                        "{ic_prev} h/<- Prev   {ic_next} l/-> Next   {ic_play} Enter Load   {ic_trash} d Delete   {ic_save} s Save   {ic_refresh} u Update   1/2/3 Mode   {ic_close} Esc Exit"
                    )
                }
                AppMode::Folders => {
                    format!(
                        "{ic_prev} h/<- Prev   {ic_next} l/-> Next   Enter Open   {ic_add} a Add   {ic_add} A Add Dir   Backspace Up   {ic_refresh} u Update   1/2/3 Mode   {ic_close} Esc Exit"
                    )
                }
            }
        };

        let (footer_w, footer_h) = get_text_dimensions(font_mgr, &footer_text, font_size * 0.85);
        let bot_y = (screen_h as i32 - 52).max(120);
        let bot_pill_w = footer_w as i32 + 64;
        let bot_pill_h = (footer_h as i32 + 20).max(42);
        let bot_center_x = (screen_w / 2) as i32;

        draw_slanted_pill(
            &mut img,
            bot_center_x,
            bot_y,
            bot_pill_w,
            bot_pill_h,
            pill_bg,
            [60, 75, 95, 180],
            1.5,
        );

        let bot_start_x = bot_center_x as f32 - (footer_w * 0.5);

        render_text(
            &mut img,
            font_mgr,
            &footer_text,
            bot_start_x,
            bot_y as f32,
            font_size * 0.85,
            text_dim,
        );
    }

    img
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_overlay_creation() {
        let config = Config::default();
        let font_mgr = FontManager::new(None);
        let track = Track {
            id: 1,
            pos: 0,
            file: "song.mp3".to_string(),
            artist: "Artist".to_string(),
            title: "Song Title".to_string(),
            album: "Album".to_string(),
            duration_secs: 180,
            is_current: true,
        };
        let overlay = render_ui_overlay(
            1920,
            1080,
            AppMode::Queue,
            0,
            5,
            Some(&track),
            None,
            None,
            "",
            None,
            None,
            None,
            None,
            PlayState::Play,
            &config,
            &font_mgr,
        );
        assert_eq!(overlay.width(), 1920);
        assert_eq!(overlay.height(), 1080);
    }

    #[test]
    fn test_font_icons_fallback() {
        let font_mgr = FontManager::new(None);
        let font = font_mgr.custom_font.as_ref().unwrap();

        let test_chars = [
            ('▶', "play triangle U+25B6"),
            ('■', "black square U+25A0"),
            ('•', "bullet U+2022"),
            ('/', "slash"),
            ('|', "bar"),
        ];

        for (ch, desc) in test_chars {
            let gid = font.glyph_id(ch);
            println!("{desc} '{ch}': gid = {:?}, valid = {}", gid, gid.0 != 0);
            assert!(gid.0 != 0);
        }
    }
}

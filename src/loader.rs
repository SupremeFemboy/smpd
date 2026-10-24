use image::RgbaImage;
use image::imageops::FilterType;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, RwLock};
use std::thread;

use crate::config::MpdConfig;
use crate::mpd::{BrowserItem, MpdClient, PlaylistInfo, Track};

const CACHE_MAGIC: &[u8; 4] = b"SWTH";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoaderTarget {
    Queue(usize),
    Playlist(usize),
    Browser(usize),
}

pub struct LoadedImage {
    pub target: LoaderTarget,
    pub result: Result<(RgbaImage, u32, u32), String>,
}

fn get_cache_dir() -> Option<PathBuf> {
    let dir = if let Ok(cache) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(cache).join("smpd/thumbnails")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".cache/smpd/thumbnails")
    } else {
        return None;
    };
    let _ = fs::create_dir_all(&dir);
    Some(dir)
}

fn compute_cache_key(path_str: &str, max_dim: u32) -> String {
    let mut h = 0xcbf29ce484222325u64;
    for &b in path_str.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    for &b in &max_dim.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}.thumb", h)
}

pub fn save_thumbnail_cache(
    cache_path: &Path,
    img: &RgbaImage,
    orig_w: u32,
    orig_h: u32,
) -> std::io::Result<()> {
    let tmp_path = cache_path.with_extension("tmp");
    let mut file = File::create(&tmp_path)?;
    file.write_all(CACHE_MAGIC)?;
    file.write_all(&img.width().to_le_bytes())?;
    file.write_all(&img.height().to_le_bytes())?;
    file.write_all(&orig_w.to_le_bytes())?;
    file.write_all(&orig_h.to_le_bytes())?;
    file.write_all(img.as_raw())?;
    file.flush()?;
    fs::rename(tmp_path, cache_path)?;
    Ok(())
}

pub fn load_thumbnail_cache(cache_path: &Path) -> std::io::Result<(RgbaImage, u32, u32)> {
    let mut file = File::open(cache_path)?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    if &magic != CACHE_MAGIC {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid cache magic",
        ));
    }
    let mut num_buf = [0u8; 4];

    file.read_exact(&mut num_buf)?;
    let w = u32::from_le_bytes(num_buf);

    file.read_exact(&mut num_buf)?;
    let h = u32::from_le_bytes(num_buf);

    file.read_exact(&mut num_buf)?;
    let orig_w = u32::from_le_bytes(num_buf);

    file.read_exact(&mut num_buf)?;
    let orig_h = u32::from_le_bytes(num_buf);

    let expected_len = (w as usize) * (h as usize) * 4;
    let mut data = vec![0u8; expected_len];
    file.read_exact(&mut data)?;

    let img = RgbaImage::from_raw(w, h, data).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Corrupt raw image in cache",
        )
    })?;

    Ok((img, orig_w, orig_h))
}

pub struct AsyncImageLoader {
    req_tx: Sender<LoaderTarget>,
    res_rx: Receiver<LoadedImage>,
    pending: HashSet<LoaderTarget>,
}

impl AsyncImageLoader {
    pub fn new(
        tracks: Arc<RwLock<Vec<Track>>>,
        playlists: Arc<RwLock<Vec<PlaylistInfo>>>,
        browser_items: Arc<RwLock<Vec<BrowserItem>>>,
        mpd_config: MpdConfig,
        max_dim: u32,
        use_cache: bool,
    ) -> Self {
        let (req_tx, req_rx) = channel::<LoaderTarget>();
        let (res_tx, res_rx) = channel::<LoadedImage>();

        thread::Builder::new()
            .name("smpd-loader".to_string())
            .spawn(move || {
                let cache_dir = get_cache_dir();
                let mut mpd_client: Option<MpdClient> = None;

                while let Ok(target) = req_rx.recv() {
                    let (file_opt, fallback_artist, fallback_title) = match target {
                        LoaderTarget::Queue(index) => {
                            let tracks_guard = tracks.read().unwrap();
                            match tracks_guard.get(index) {
                                Some(t) => {
                                    (Some(t.file.clone()), t.artist.clone(), t.title.clone())
                                }
                                None => continue,
                            }
                        }
                        LoaderTarget::Playlist(index) => {
                            let playlists_guard = playlists.read().unwrap();
                            match playlists_guard.get(index) {
                                Some(p) => (
                                    p.first_track_file.clone(),
                                    "Playlist".to_string(),
                                    p.name.clone(),
                                ),
                                None => continue,
                            }
                        }
                        LoaderTarget::Browser(index) => {
                            let browser_guard = browser_items.read().unwrap();
                            match browser_guard.get(index) {
                                Some(item) => {
                                    if item.is_dir {
                                        (
                                            item.sample_file.clone(),
                                            "Folder".to_string(),
                                            item.name.clone(),
                                        )
                                    } else {
                                        (
                                            Some(item.path.clone()),
                                            "Track".to_string(),
                                            item.name.clone(),
                                        )
                                    }
                                }
                                None => continue,
                            }
                        }
                    };

                    let cache_file = if use_cache {
                        file_opt.as_ref().and_then(|f| {
                            cache_dir.as_ref().map(|dir| {
                                let key = compute_cache_key(f, max_dim);
                                dir.join(key)
                            })
                        })
                    } else {
                        None
                    };

                    // Check disk cache first
                    if let Some(ref path) = cache_file
                        && path.exists()
                        && let Ok((img, orig_w, orig_h)) = load_thumbnail_cache(path)
                    {
                        let _ = res_tx.send(LoadedImage {
                            target,
                            result: Ok((img, orig_w, orig_h)),
                        });
                        continue;
                    }

                    // Cache miss: connect MPD if needed and fetch cover
                    if mpd_client.is_none() {
                        mpd_client = MpdClient::connect(&mpd_config).ok();
                    }

                    let mut cover_bytes = None;
                    if let Some(ref file) = file_opt {
                        if let Some(ref mut cl) = mpd_client {
                            cover_bytes = cl.fetch_cover(file);
                        }

                        // Retry connection once if needed
                        if cover_bytes.is_none()
                            && mpd_client.is_none()
                            && let Ok(mut new_client) = MpdClient::connect(&mpd_config)
                        {
                            cover_bytes = new_client.fetch_cover(file);
                            mpd_client = Some(new_client);
                        }
                    }

                    let mut is_real_cover = false;
                    let (img, orig_w, orig_h) = if let Some(bytes) = cover_bytes
                        && let Ok(decoded) = image::load_from_memory(&bytes)
                    {
                        is_real_cover = true;
                        let (orig_w, orig_h) = (decoded.width(), decoded.height());
                        let thumb = if orig_w > max_dim || orig_h > max_dim {
                            decoded.resize(max_dim, max_dim, FilterType::Triangle)
                        } else {
                            decoded
                        };
                        (thumb.to_rgba8(), orig_w, orig_h)
                    } else {
                        // Generate procedural vinyl album cover
                        let procedural =
                            generate_procedural_cover(&fallback_artist, &fallback_title, 512);
                        (procedural, 512, 512)
                    };

                    if is_real_cover && let Some(ref path) = cache_file {
                        let _ = save_thumbnail_cache(path, &img, orig_w, orig_h);
                    }

                    let _ = res_tx.send(LoadedImage {
                        target,
                        result: Ok((img, orig_w, orig_h)),
                    });
                }
            })
            .expect("Failed to spawn loader thread");

        Self {
            req_tx,
            res_rx,
            pending: HashSet::new(),
        }
    }

    pub fn request_queue(&mut self, index: usize) {
        let target = LoaderTarget::Queue(index);
        if self.pending.insert(target) {
            let _ = self.req_tx.send(target);
        }
    }

    pub fn request_playlist(&mut self, index: usize) {
        let target = LoaderTarget::Playlist(index);
        if self.pending.insert(target) {
            let _ = self.req_tx.send(target);
        }
    }

    pub fn request_browser(&mut self, index: usize) {
        let target = LoaderTarget::Browser(index);
        if self.pending.insert(target) {
            let _ = self.req_tx.send(target);
        }
    }

    pub fn mark_completed(&mut self, target: LoaderTarget) {
        self.pending.remove(&target);
    }

    pub fn try_recv(&self) -> Option<LoadedImage> {
        self.res_rx.try_recv().ok()
    }
}

pub fn generate_procedural_cover(artist: &str, title: &str, size: u32) -> RgbaImage {
    let mut img = RgbaImage::new(size, size);
    let mut h = 0xcbf29ce484222325u64;
    for &b in artist.as_bytes().iter().chain(title.as_bytes().iter()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    let seed = h;

    let hue_base = ((seed & 0xFF) as f32 / 255.0) * 360.0;
    let (r1, g1, b1) = hsl_to_rgb(hue_base, 0.70, 0.16);
    let (r2, g2, b2) = hsl_to_rgb((hue_base + 60.0) % 360.0, 0.75, 0.32);
    let (lr, lg, lb) = hsl_to_rgb((hue_base + 180.0) % 360.0, 0.85, 0.55);

    let center = size as f32 * 0.5;
    let vinyl_radius = size as f32 * 0.44;
    let label_radius = size as f32 * 0.18;
    let spindle_radius = size as f32 * 0.035;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();

            let pixel = if dist > vinyl_radius {
                let t = (x as f32 + y as f32) / (size as f32 * 2.0);
                let r = (r1 as f32 * (1.0 - t) + r2 as f32 * t) as u8;
                let g = (g1 as f32 * (1.0 - t) + g2 as f32 * t) as u8;
                let b = (b1 as f32 * (1.0 - t) + b2 as f32 * t) as u8;
                image::Rgba([r, g, b, 255])
            } else if dist > label_radius {
                let groove = ((dist * 1.5).sin() * 12.0) as i32;
                let base_gray = 24 + (groove.clamp(-8, 8));
                let angle = dy.atan2(dx);
                let sheen = ((angle * 2.0).sin().abs() * 20.0) as u8;
                let val = (base_gray as u8).saturating_add(sheen);
                image::Rgba([val, val, val + 2, 255])
            } else if dist > spindle_radius {
                let t = dist / label_radius;
                let r = (lr as f32 * (0.8 + 0.2 * t)).min(255.0) as u8;
                let g = (lg as f32 * (0.8 + 0.2 * t)).min(255.0) as u8;
                let b = (lb as f32 * (0.8 + 0.2 * t)).min(255.0) as u8;
                image::Rgba([r, g, b, 255])
            } else {
                image::Rgba([15, 17, 24, 255])
            };

            img.put_pixel(x, y, pixel);
        }
    }

    img
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let m = l - c * 0.5;

    let (r, g, b) = if hp < 1.0 {
        (c, x, 0.0)
    } else if hp < 2.0 {
        (x, c, 0.0)
    } else if hp < 3.0 {
        (0.0, c, x)
    } else if hp < 4.0 {
        (0.0, x, c)
    } else if hp < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_header_and_roundtrip() {
        let dir = std::env::temp_dir().join("smpd_test_cache2");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.thumb");

        let mut img = RgbaImage::new(16, 16);
        for pixel in img.pixels_mut() {
            *pixel = image::Rgba([255, 0, 128, 255]);
        }

        save_thumbnail_cache(&path, &img, 64, 64).unwrap();
        let (read_img, ow, oh) = load_thumbnail_cache(&path).unwrap();
        assert_eq!(read_img.width(), 16);
        assert_eq!(read_img.height(), 16);
        assert_eq!(ow, 64);
        assert_eq!(oh, 64);

        let _ = fs::remove_file(path);
        let _ = fs::remove_dir(dir);
    }
}

use crate::config::MpdConfig;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayState {
    Play,
    Pause,
    Stop,
}

impl PlayState {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            PlayState::Play => "PLAYING",
            PlayState::Pause => "PAUSED",
            PlayState::Stop => "STOPPED",
        }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct MpdStatus {
    pub state: PlayState,
    pub song_pos: Option<usize>,
    pub song_id: Option<u32>,
    pub elapsed_secs: Option<f32>,
    pub duration_secs: Option<f32>,
    pub volume: Option<u32>,
    pub playlist_length: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    pub id: u32,
    pub pos: usize,
    pub file: String,
    pub artist: String,
    pub title: String,
    pub album: String,
    pub duration_secs: u32,
    pub is_current: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlaylistInfo {
    pub name: String,
    pub track_count: usize,
    pub first_track_file: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BrowserItem {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub item_count: usize,
    pub sample_file: Option<String>,
}

impl Track {
    #[allow(dead_code)]
    pub fn duration_formatted(&self) -> String {
        let mins = self.duration_secs / 60;
        let secs = self.duration_secs % 60;
        format!("{}:{:02}", mins, secs)
    }
}

pub enum MpdStream {
    Tcp(TcpStream),
    Unix(UnixStream),
}

impl Read for MpdStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            MpdStream::Tcp(s) => s.read(buf),
            MpdStream::Unix(s) => s.read(buf),
        }
    }
}

impl Write for MpdStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            MpdStream::Tcp(s) => s.write(buf),
            MpdStream::Unix(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            MpdStream::Tcp(s) => s.flush(),
            MpdStream::Unix(s) => s.flush(),
        }
    }
}

impl MpdStream {
    pub fn try_clone(&self) -> std::io::Result<Self> {
        match self {
            MpdStream::Tcp(s) => s.try_clone().map(MpdStream::Tcp),
            MpdStream::Unix(s) => s.try_clone().map(MpdStream::Unix),
        }
    }

    pub fn set_read_timeout(&self, dur: Option<Duration>) -> std::io::Result<()> {
        match self {
            MpdStream::Tcp(s) => s.set_read_timeout(dur),
            MpdStream::Unix(s) => s.set_read_timeout(dur),
        }
    }

    pub fn set_write_timeout(&self, dur: Option<Duration>) -> std::io::Result<()> {
        match self {
            MpdStream::Tcp(s) => s.set_write_timeout(dur),
            MpdStream::Unix(s) => s.set_write_timeout(dur),
        }
    }
}

pub struct MpdClient {
    stream: MpdStream,
    reader: BufReader<MpdStream>,
    music_directory: Option<PathBuf>,
}

impl MpdClient {
    pub fn connect(config: &MpdConfig) -> Result<Self, String> {
        let timeout = Duration::from_millis(config.timeout_ms);
        let stream = Self::open_stream(&config.host, config.port, timeout)?;

        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| format!("Failed to set read timeout: {e}"))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|e| format!("Failed to set write timeout: {e}"))?;

        let read_stream = stream
            .try_clone()
            .map_err(|e| format!("Failed to clone stream: {e}"))?;
        let mut reader = BufReader::new(read_stream);

        // Read banner
        let mut banner = String::new();
        reader
            .read_line(&mut banner)
            .map_err(|e| format!("Failed to read MPD banner: {e}"))?;

        if !banner.starts_with("OK MPD") {
            return Err(format!("Unexpected MPD banner: {}", banner.trim()));
        }

        let mut client = Self {
            stream,
            reader,
            music_directory: config.music_directory.clone(),
        };

        if let Some(ref pwd) = config.password {
            client.send_command(&format!("password \"{}\"", escape_param(pwd)))?;
        }

        Ok(client)
    }

    fn open_stream(host: &str, port: u16, timeout: Duration) -> Result<MpdStream, String> {
        // Check for Unix socket
        if host.starts_with('/') || host.starts_with("unix:") {
            let socket_path = host.strip_prefix("unix:").unwrap_or(host);
            let stream = UnixStream::connect(socket_path).map_err(|e| {
                format!("Failed to connect to MPD unix socket '{socket_path}': {e}")
            })?;
            return Ok(MpdStream::Unix(stream));
        }

        // Check if $XDG_RUNTIME_DIR/mpd/socket exists as convenience if host is default
        if (host == "127.0.0.1" || host == "localhost")
            && port == 6600
            && let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR")
        {
            let socket = PathBuf::from(runtime_dir).join("mpd/socket");
            if socket.exists()
                && let Ok(s) = UnixStream::connect(&socket)
            {
                return Ok(MpdStream::Unix(s));
            }
        }

        // Connect TCP
        use std::net::ToSocketAddrs;
        let addr_str = format!("{host}:{port}");
        let addrs = addr_str
            .to_socket_addrs()
            .map_err(|e| format!("Failed to resolve '{addr_str}': {e}"))?;

        for addr in addrs {
            if let Ok(s) = TcpStream::connect_timeout(&addr, timeout) {
                let _ = s.set_nodelay(true);
                return Ok(MpdStream::Tcp(s));
            }
        }

        Err(format!("Could not connect to MPD at {addr_str}"))
    }

    pub fn send_command(&mut self, cmd: &str) -> Result<Vec<String>, String> {
        let req = format!("{cmd}\n");
        self.stream
            .write_all(req.as_bytes())
            .map_err(|e| format!("Failed to send '{cmd}': {e}"))?;
        self.stream
            .flush()
            .map_err(|e| format!("Failed to flush stream: {e}"))?;

        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            let bytes_read = self
                .reader
                .read_line(&mut line)
                .map_err(|e| format!("Failed to read response for '{cmd}': {e}"))?;

            if bytes_read == 0 {
                return Err("Connection closed unexpectedly by MPD server".to_string());
            }

            let trimmed = line.trim();
            if trimmed == "OK" {
                break;
            }
            if trimmed.starts_with("ACK ") {
                return Err(format!("MPD error: {trimmed}"));
            }
            lines.push(trimmed.to_string());
        }

        Ok(lines)
    }

    pub fn get_status(&mut self) -> Result<MpdStatus, String> {
        let lines = self.send_command("status")?;

        let mut state = PlayState::Stop;
        let mut song_pos = None;
        let mut song_id = None;
        let mut elapsed_secs = None;
        let mut duration_secs = None;
        let mut volume = None;
        let mut playlist_length = 0;

        for line in lines {
            if let Some((k, v)) = line.split_once(": ") {
                match k {
                    "state" => match v {
                        "play" => state = PlayState::Play,
                        "pause" => state = PlayState::Pause,
                        _ => state = PlayState::Stop,
                    },
                    "song" => song_pos = v.parse::<usize>().ok(),
                    "songid" => song_id = v.parse::<u32>().ok(),
                    "elapsed" => elapsed_secs = v.parse::<f32>().ok(),
                    "duration" => duration_secs = v.parse::<f32>().ok(),
                    "volume" => volume = v.parse::<u32>().ok(),
                    "playlistlength" => playlist_length = v.parse::<usize>().unwrap_or(0),
                    _ => {}
                }
            }
        }

        Ok(MpdStatus {
            state,
            song_pos,
            song_id,
            elapsed_secs,
            duration_secs,
            volume,
            playlist_length,
        })
    }

    pub fn get_playlist(&mut self, current_song_id: Option<u32>) -> Result<Vec<Track>, String> {
        let lines = self.send_command("playlistinfo")?;
        let mut tracks = Vec::new();

        let mut curr_file: Option<String> = None;
        let mut curr_artist: Option<String> = None;
        let mut curr_title: Option<String> = None;
        let mut curr_album: Option<String> = None;
        let mut curr_duration: u32 = 0;
        let mut curr_pos: usize = 0;
        let mut curr_id: u32 = 0;

        let commit_track = |tracks: &mut Vec<Track>,
                            file: Option<String>,
                            artist: Option<String>,
                            title: Option<String>,
                            album: Option<String>,
                            duration: u32,
                            pos: usize,
                            id: u32| {
            if let Some(f) = file {
                let default_title = Path::new(&f)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Unknown Track")
                    .to_string();

                let is_current = current_song_id.map(|cid| cid == id).unwrap_or(false);

                tracks.push(Track {
                    id,
                    pos,
                    file: f,
                    artist: artist.unwrap_or_else(|| "Unknown Artist".to_string()),
                    title: title.unwrap_or(default_title),
                    album: album.unwrap_or_else(|| "Single".to_string()),
                    duration_secs: duration,
                    is_current,
                });
            }
        };

        for line in lines {
            if let Some((k, v)) = line.split_once(": ") {
                match k {
                    "file" => {
                        commit_track(
                            &mut tracks,
                            curr_file.take(),
                            curr_artist.take(),
                            curr_title.take(),
                            curr_album.take(),
                            curr_duration,
                            curr_pos,
                            curr_id,
                        );
                        curr_file = Some(v.to_string());
                        curr_artist = None;
                        curr_title = None;
                        curr_album = None;
                        curr_duration = 0;
                        curr_pos = 0;
                        curr_id = 0;
                    }
                    "Artist" => curr_artist = Some(v.to_string()),
                    "Title" => curr_title = Some(v.to_string()),
                    "Album" => curr_album = Some(v.to_string()),
                    "Time" => {
                        if let Ok(sec) = v.parse::<u32>() {
                            curr_duration = sec;
                        }
                    }
                    "duration" => {
                        if let Ok(sec) = v.parse::<f32>() {
                            curr_duration = sec.round() as u32;
                        }
                    }
                    "Pos" => {
                        if let Ok(p) = v.parse::<usize>() {
                            curr_pos = p;
                        }
                    }
                    "Id" => {
                        if let Ok(i) = v.parse::<u32>() {
                            curr_id = i;
                        }
                    }
                    _ => {}
                }
            }
        }

        // Commit final track
        commit_track(
            &mut tracks,
            curr_file,
            curr_artist,
            curr_title,
            curr_album,
            curr_duration,
            curr_pos,
            curr_id,
        );

        Ok(tracks)
    }

    pub fn play_id(&mut self, id: u32) -> Result<(), String> {
        self.send_command(&format!("playid {id}"))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn play_pos(&mut self, pos: usize) -> Result<(), String> {
        self.send_command(&format!("play {pos}"))?;
        Ok(())
    }

    pub fn toggle_pause(&mut self) -> Result<(), String> {
        self.send_command("pause")?;
        Ok(())
    }

    pub fn next(&mut self) -> Result<(), String> {
        self.send_command("next")?;
        Ok(())
    }

    pub fn previous(&mut self) -> Result<(), String> {
        self.send_command("previous")?;
        Ok(())
    }

    pub fn delete_id(&mut self, id: u32) -> Result<(), String> {
        self.send_command(&format!("deleteid {id}"))?;
        Ok(())
    }

    pub fn list_playlists(&mut self) -> Result<Vec<PlaylistInfo>, String> {
        let lines = self.send_command("listplaylists")?;
        let mut playlist_names = Vec::new();

        for line in lines {
            if let Some(name) = line.strip_prefix("playlist: ") {
                playlist_names.push(name.to_string());
            }
        }

        let mut playlists = Vec::new();
        for name in playlist_names {
            let escaped = escape_param(&name);
            let mut track_count = 0;
            let mut first_file = None;

            if let Ok(info_lines) = self.send_command(&format!("listplaylistinfo \"{escaped}\"")) {
                for l in info_lines {
                    if let Some(f) = l.strip_prefix("file: ") {
                        track_count += 1;
                        if first_file.is_none() {
                            first_file = Some(f.to_string());
                        }
                    }
                }
            }

            playlists.push(PlaylistInfo {
                name,
                track_count,
                first_track_file: first_file,
            });
        }

        Ok(playlists)
    }

    /// Clears the active queue, loads the specified playlist, and starts playing from track 0.
    pub fn load_playlist_replace(&mut self, name: &str) -> Result<(), String> {
        let escaped = escape_param(name);
        let _ = self.send_command("clear");
        self.send_command(&format!("load \"{escaped}\""))?;
        let _ = self.send_command("play 0");
        Ok(())
    }

    /// Clears the current active queue.
    pub fn clear_queue(&mut self) -> Result<(), String> {
        self.send_command("clear")?;
        Ok(())
    }

    /// Saves the current active queue as a new playlist.
    pub fn save_playlist(&mut self, name: &str) -> Result<(), String> {
        let escaped = escape_param(name);
        self.send_command(&format!("save \"{escaped}\""))?;
        Ok(())
    }

    /// Overwrites an existing playlist by removing it first, then saving the current queue.
    pub fn overwrite_playlist(&mut self, name: &str) -> Result<(), String> {
        let escaped = escape_param(name);
        let _ = self.send_command(&format!("rm \"{escaped}\""));
        self.send_command(&format!("save \"{escaped}\""))?;
        Ok(())
    }

    /// Deletes a stored playlist from MPD storage.
    pub fn delete_playlist(&mut self, name: &str) -> Result<(), String> {
        let escaped = escape_param(name);
        self.send_command(&format!("rm \"{escaped}\""))?;
        Ok(())
    }

    /// Adds a file or directory to the end of the active queue.
    pub fn add_to_queue(&mut self, uri_or_dir: &str) -> Result<(), String> {
        let escaped = escape_param(uri_or_dir);
        self.send_command(&format!("add \"{escaped}\""))?;
        Ok(())
    }

    /// Triggers an MPD database update (`mpc update`).
    pub fn update_database(&mut self, uri: Option<&str>) -> Result<u32, String> {
        let cmd = match uri {
            Some(u) => format!("update \"{}\"", escape_param(u)),
            None => "update".to_string(),
        };
        let lines = self.send_command(&cmd)?;
        let mut job_id = 0;
        for l in lines {
            if let Some(id_str) = l.strip_prefix("updating_db: ")
                && let Ok(id) = id_str.parse::<u32>()
            {
                job_id = id;
            }
        }
        Ok(job_id)
    }

    /// Lists subdirectories and audio files in a given directory in the MPD music library.
    pub fn list_directory(&mut self, dir: &str) -> Result<Vec<BrowserItem>, String> {
        let escaped = escape_param(dir);
        let cmd = if dir.is_empty() {
            "lsinfo \"\"".to_string()
        } else {
            format!("lsinfo \"{escaped}\"")
        };
        let lines = self.send_command(&cmd)?;

        let mut items = Vec::new();
        for line in lines {
            if let Some(sub_dir) = line.strip_prefix("directory: ") {
                let name = Path::new(sub_dir)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(sub_dir)
                    .to_string();
                items.push(BrowserItem {
                    name,
                    path: sub_dir.to_string(),
                    is_dir: true,
                    item_count: 0,
                    sample_file: None,
                });
            } else if let Some(file_path) = line.strip_prefix("file: ") {
                let name = Path::new(file_path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(file_path)
                    .to_string();
                items.push(BrowserItem {
                    name,
                    path: file_path.to_string(),
                    is_dir: false,
                    item_count: 0,
                    sample_file: Some(file_path.to_string()),
                });
            }
        }

        // For subdirectories, inspect sample file / track count
        for item in items.iter_mut().filter(|it| it.is_dir) {
            let esc = escape_param(&item.path);
            if let Ok(info_lines) = self.send_command(&format!("lsinfo \"{esc}\"")) {
                let mut cnt = 0;
                for l in info_lines {
                    if let Some(f) = l.strip_prefix("file: ") {
                        cnt += 1;
                        if item.sample_file.is_none() {
                            item.sample_file = Some(f.to_string());
                        }
                    } else if l.starts_with("directory: ") {
                        cnt += 1;
                    }
                }
                item.item_count = cnt;
            }
        }

        Ok(items)
    }

    /// Extract cover artwork for track URI.
    /// 1. First tries local filesystem extraction (embedded ID3 APIC tag or folder artwork).
    /// 2. If not found or remote, queries MPD protocol (`readpicture`, then `albumart`).
    pub fn fetch_cover(&mut self, uri: &str) -> Option<Vec<u8>> {
        // 1. Try local extraction (fastest: ~0.5ms vs 200+ network roundtrips)
        if let Some(bytes) = self.fetch_local_cover(uri) {
            return Some(bytes);
        }

        // 2. Try MPD protocol commands
        for cmd in &["readpicture", "albumart"] {
            if let Some(bytes) = self.fetch_mpd_binary(cmd, uri)
                && !bytes.is_empty()
            {
                return Some(bytes);
            }
        }

        None
    }

    fn fetch_mpd_binary(&mut self, cmd: &str, uri: &str) -> Option<Vec<u8>> {
        let mut offset = 0usize;
        let mut total_size = 0usize;
        let mut image_bytes = Vec::new();
        let escaped_uri = escape_param(uri);

        loop {
            let request = format!("{cmd} \"{escaped_uri}\" {offset}\n");
            self.stream.write_all(request.as_bytes()).ok()?;
            self.stream.flush().ok()?;

            let mut binary_chunk_len = None;

            loop {
                let mut line = String::new();
                self.reader.read_line(&mut line).ok()?;
                let trimmed = line.trim();

                if trimmed.starts_with("ACK") {
                    return None;
                }
                if trimmed == "OK" {
                    break;
                }
                if let Some(val) = trimmed.strip_prefix("size: ")
                    && let Ok(s) = val.parse::<usize>()
                {
                    total_size = s;
                }
                if let Some(val) = trimmed.strip_prefix("binary: ")
                    && let Ok(b) = val.parse::<usize>()
                {
                    binary_chunk_len = Some(b);
                    break;
                }
            }

            if let Some(b_len) = binary_chunk_len {
                let mut chunk = vec![0u8; b_len];
                self.reader.read_exact(&mut chunk).ok()?;
                image_bytes.extend_from_slice(&chunk);
                offset += b_len;

                // Read trailing newline
                let mut nl = [0u8; 1];
                self.reader.read_exact(&mut nl).ok()?;

                // Read trailing text lines until OK
                loop {
                    let mut ok_line = String::new();
                    self.reader.read_line(&mut ok_line).ok()?;
                    let t = ok_line.trim();
                    if t == "OK" || t.is_empty() {
                        break;
                    }
                }
            } else {
                break;
            }

            if offset >= total_size || total_size == 0 {
                break;
            }
        }

        if image_bytes.is_empty() {
            None
        } else {
            Some(image_bytes)
        }
    }

    fn fetch_local_cover(&self, uri: &str) -> Option<Vec<u8>> {
        let base_dirs = if let Some(ref dir) = self.music_directory {
            vec![dir.clone()]
        } else {
            let mut dirs = Vec::new();
            if let Ok(home) = std::env::var("HOME") {
                dirs.push(PathBuf::from(&home).join("Music"));
                dirs.push(PathBuf::from(&home).join("music"));
            }
            dirs
        };

        for base in &base_dirs {
            let mut candidate_file = base.join(uri);

            // If direct path doesn't exist, search for matching filename in music directory
            if !candidate_file.is_file()
                && let Some(file_name) = Path::new(uri).file_name()
                && let Ok(entries) = std::fs::read_dir(base)
            {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        let sub_candidate = p.join(file_name);
                        if sub_candidate.is_file() {
                            candidate_file = sub_candidate;
                            break;
                        }
                    }
                }
            }

            if candidate_file.is_file() {
                // Check for embedded ID3 APIC tag in audio file
                if let Some(apic_bytes) = extract_embedded_art(&candidate_file) {
                    return Some(apic_bytes);
                }

                // Check directory for cover image files
                if let Some(parent) = candidate_file.parent() {
                    for cover_name in &[
                        "cover.jpg",
                        "cover.png",
                        "cover.webp",
                        "folder.jpg",
                        "folder.png",
                        "front.jpg",
                        "front.png",
                        "AlbumArt.jpg",
                    ] {
                        let candidate = parent.join(cover_name);
                        if candidate.is_file()
                            && let Ok(bytes) = std::fs::read(&candidate)
                        {
                            return Some(bytes);
                        }
                    }
                }
            }
        }

        None
    }
}

pub fn extract_embedded_art(file_path: &Path) -> Option<Vec<u8>> {
    let mut file = std::fs::File::open(file_path).ok()?;
    let mut header = [0u8; 10];
    file.read_exact(&mut header).ok()?;

    if &header[0..3] == b"ID3" {
        let tag_size = (((header[6] & 0x7F) as usize) << 21)
            | (((header[7] & 0x7F) as usize) << 14)
            | (((header[8] & 0x7F) as usize) << 7)
            | ((header[9] & 0x7F) as usize);

        let mut data = vec![0u8; tag_size];
        file.read_exact(&mut data).ok()?;

        let mut pos = 0;
        while pos + 10 < data.len() {
            if &data[pos..pos + 4] == b"APIC" {
                let frame_size = u32::from_be_bytes([
                    data[pos + 4],
                    data[pos + 5],
                    data[pos + 6],
                    data[pos + 7],
                ]) as usize;

                let end = (pos + 10 + frame_size).min(data.len());
                let body = &data[pos + 10..end];

                // Search for magic bytes of JPEG, PNG, or WEBP in the APIC frame body
                for i in 0..body.len().saturating_sub(4) {
                    if body[i..].starts_with(&[0xFF, 0xD8, 0xFF])
                        || body[i..].starts_with(b"\x89PNG")
                        || body[i..].starts_with(b"RIFF")
                    {
                        return Some(body[i..].to_vec());
                    }
                }
            }
            pos += 1;
        }
    }

    None
}

fn escape_param(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_param() {
        assert_eq!(escape_param("hello \"world\""), "hello \\\"world\\\"");
        assert_eq!(escape_param("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_list_playlists_mock() {
        let p = PlaylistInfo {
            name: "Favorites".to_string(),
            track_count: 10,
            first_track_file: Some("rock/song.mp3".to_string()),
        };
        assert_eq!(p.name, "Favorites");
        assert_eq!(p.track_count, 10);
    }

    #[test]
    fn test_track_duration_format() {
        let track = Track {
            id: 1,
            pos: 0,
            file: "song.mp3".to_string(),
            artist: "Artist".to_string(),
            title: "Title".to_string(),
            album: "Album".to_string(),
            duration_secs: 247,
            is_current: false,
        };
        assert_eq!(track.duration_formatted(), "4:07");
    }

    #[test]
    fn test_extract_embedded_art_if_exists() {
        let candidate = PathBuf::from("/home/user/Music/Not Calm/MATORO - Z-ISPUGALSA.mp3");
        if candidate.is_file() {
            let art = extract_embedded_art(&candidate);
            assert!(art.is_some());
            let bytes = art.unwrap();
            assert!(!bytes.is_empty());
        }
    }
}

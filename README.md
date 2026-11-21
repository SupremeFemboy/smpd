# smpd

> ⚠️ **100% AI-generated slop:** This code was entirely architected, written, and refactored by AI agents under the supervision of a Supreme Femboy. Use at your own risk.

---

## English

A fast, GPU-accelerated Wayland MPD (Music Player Daemon) track and playlist selector written in Rust with `wgpu` and `layershellev`. It opens directly as a native Wayland Layer Shell surface (`zwlr_layer_shell_v1`) on the `Overlay` layer with exclusive keyboard grab. Each album cover preview is rendered as a square 1:1 parallelogram with sides tilted at a 75° angle, complete with sub-pixel edge anti-aliasing, subtle active card zoom, upright cover texture projection, and real-time MPD status indicators.

Tracks, stored playlists, and library directories are discovered automatically from MPD. Album art is extracted in real time directly from local files (ID3 APIC tags and folder images) or streamed over the native MPD protocol (`readpicture` and `albumart`), with automatic procedural vinyl fallback generation for missing artwork. Controls use hardware scancodes (`physical_key`) to work reliably across all keyboard layouts (including Russian and English). Features a deterministic high-speed disk thumbnail cache for instantaneous startups, a 3-mode switcher (Queue, Playlists, Folders), in-app playlist saving with overwrite confirmation, and a customizable TOML configuration with automatic system font resolution via Fontconfig.

### Features

- **Native Wayland Layer Shell.** Binds directly to `zwlr_layer_shell_v1` on `Layer::Overlay`. Sits above all windows, workspaces, and status bars without being managed as an ordinary XDG client window (no window rules or tiling hacks needed).
- **Square Parallelograms with 75° Slanted Sides (1:1).** Each card is tailored for album covers with an exact 1:1 aspect ratio ($W = H$) and sides slanted at 75° ($\Delta X = H \cdot \cot(75^\circ) \approx 0.268 \times H$).
- **Upright Texture Projection & Anti-Aliasing.** Shaders map album covers in upright cover mode without geometric shear or distortion, while calculating exact perpendicular edge distances for razor-sharp, anti-aliased borders.
- **High-Speed Dual-Tier Cover Art Extraction.** Extracts embedded artwork directly from local audio files (ID3v2 APIC frames and folder images) in < 0.5 ms, or streams over MPD protocol binary chunks (`readpicture` / `albumart`). Missing covers procedurally synthesize unique vinyl record gradient art.
- **Deterministic Disk Thumbnail Cache.** Previews are scaled to 960×960 and cached in binary format under `$XDG_CACHE_HOME/smpd/thumbnails/`. Cached loads take less than 0.5 ms per image. Cache keys use stable 64-bit FNV-1a hashing, eliminating redundant disk or network decoding.
- **Three Dedicated Operating Modes:**
  - **Mode 1 (`1`): Active Queue.** Browse, reorder, play, pause, seek, and delete tracks from the active MPD queue.
  - **Mode 2 (`2`): Stored Playlists.** View all saved playlists with thumbnail previews. Pressing `Enter` clears the current queue, loads the playlist, and immediately begins playback. Includes interactive deletion with confirmation (`d`).
  - **Mode 3 (`3`): Library Folders.** Explore music directories and files. Press `a` to add single items, or `A` to recursively queue the entire directory immediately.
- **Scancode-Based Navigation (Layout-Agnostic).** Keybindings are registered via hardware scancodes (`PhysicalKey::Code`). Shortcuts (`h`, `l`, `c`, `s`, `d`, `a`, `u`, etc.) work identically on Russian, English, or any other keyboard layout.
- **In-App Playlist Creation & Overwrite Protection.** Press `s` in any mode to open an interactive command bar and save the current queue to a new playlist. If a playlist with that name already exists, an overwrite confirmation dialog prompts before replacing files.
- **Database Synchronization.** Press `u` anywhere to trigger a database update in MPD (`mpc update`) with immediate visual feedback in the status bar.
- **Fluid Keyboard & Mouse Navigation.** Full support for Left/Right arrows, Vim motions (`h`/`l`), mouse scroll wheel, and direct mouse click selection (clicking the focused card confirms selection). Smooth exponential spring animations on scroll.
- **TOML Configuration.** Customize colors (in `#RRGGBB`, `#RRGGBBAA`, or `[r, g, b, a]` formats), border widths, card opacity, panel toggles, font size, and thumbnail dimensions in `~/.config/smpd/config.toml`.
- **System Font Resolution via Fontconfig.** Set `font` to any family name (default `"monospace"`, `"Noto Sans Mono"`, `"JetBrains Mono"`, `"DepartureMono Nerd Font"`) or a direct file path; resolved automatically via `fc-match`. Dynamically adapts between Nerd Font glyphs and clean Unicode symbols without broken boxes (`.notdef`). Seamlessly falls back to a built-in bitmap font if needed.

### Architecture

```
src/
├── main.rs          # layer-shell event loop (layershellev), scancode input dispatch, mode management
├── config.rs        # TOML config loader, flexible Color deserializer, default template
├── mpd.rs           # MPD client protocol, queue/playlist/folder inspection, ID3 APIC extraction
├── loader.rs        # async thumbnail loader, FNV-1a deterministic disk cache, procedural vinyl covers
├── render.rs        # wgpu pipelines, WGSL shaders (square parallelograms, vignette, UI quad)
└── ui.rs            # fontconfig resolver (fc-match), ab_glyph/font8x8 text renderer, dynamic glyph fallback
```

1. **Windowing & Event Loop:** Uses `layershellev::WindowState` configured with `.with_use_display_handle(true)`, `.with_layer(Layer::Overlay)`, `.with_anchor(Top | Bottom | Left | Right)`, and `.with_keyboard_interacivity(KeyboardInteractivity::Exclusive)`.
2. **GPU Rendering:** Passes `layershellev::WindowWrapper` to `wgpu::Instance::create_surface` over Vulkan (`Backends::PRIMARY`). Three rendering passes: background radial vignette, instanced square parallelogram card mesh, and UI text overlay.
3. **Threading:** Image decoding, MPD binary streaming, and disk cache generation run asynchronously in a background worker thread, streaming thumbnails over channels to keep the UI running at monitor refresh rate.

### Configuration

Paths adhere to the XDG Base Directory specification:

| File | Path |
|---|---|
| `config.toml` | `$XDG_CONFIG_HOME/smpd/config.toml` (default `~/.config/smpd/config.toml`) |
| `thumbnails/` | `$XDG_CACHE_HOME/smpd/thumbnails/` (default `~/.cache/smpd/thumbnails/`) |

If `config.toml` does not exist on startup, it is automatically created with default settings:

```toml
# smpd configuration file

[ui]
# Show the top panel with track index and title
show_top_bar = true

# Show the bottom panel with keyboard controls
show_bottom_bar = true

# Font name/family (e.g. "monospace", "Noto Sans Mono", "JetBrains Mono")
# or direct file path to a .ttf/.otf font file.
# If empty or not found, system "monospace" font (or built-in bitmap font) is used.
font = "monospace"

# Font size in pixels
font_size = 20.0

[colors]
# Colors can be specified as hex string ("#0b0e14", "#0b0e14e1") or array ([r, g, b, a])
background_color = "#0b0e14e1"
panel_bg_color = "#10141adc"
panel_border_color = "#00c8ffb4"

card_border_color = "#ffffff40"
selected_border_color = "#00e1ffff"

card_border_width = 1.5
selected_border_width = 4.5
unselected_opacity = 0.65

text_color = "#f5f8ffff"
accent_color = "#00e1ffff"
dim_text_color = "#8c9baff0"

[cache]
# Enable disk caching of downscaled thumbnails for instant loading
enabled = true

# Maximum thumbnail dimension in pixels (960x960 for square previews)
max_thumbnail_size = 960

[mpd]
# MPD server host (IP, hostname, or unix socket)
host = "127.0.0.1"
port = 6600
# password = "secret"
# music_directory = "/home/user/Music"
timeout_ms = 3000
# When true, selecting a track with Enter switches song and exits smpd
close_on_select = false
```

### Controls & Keybindings

| Key / Input | Mode | Action |
|---|---|---|
| `1` | Global | Switch to **Mode 1: Active Queue** |
| `2` | Global | Switch to **Mode 2: Stored Playlists** |
| `3` | Global | Switch to **Mode 3: Library Folders** |
| `Tab` | Global | Cycle modes: `Queue → Playlists → Folders → Queue` |
| `Left` / `h` | Global | Move selection to previous item |
| `Right` / `l` | Global | Move selection to next item |
| `Mouse Wheel` | Global | Scroll left / right through carousel ribbon |
| `Left Click` | Global | Focus clicked card (clicking already focused card confirms) |
| `Enter` / `Numpad Enter` | Context | Mode 1: Play track. Mode 2: Clear & load playlist. Mode 3: Open folder / add file |
| `Space` | Mode 1 | Toggle playback (play / pause) |
| `c` | Mode 1 | Center carousel on currently playing track |
| `C` / `x` | Mode 1 | Clear current active queue (`clear`) |
| `n` / `p` | Mode 1 | Next / previous track in MPD |
| `d` / `Delete` | Mode 1 | Remove focused track from active queue |
| `d` / `Delete` | Mode 2 | Delete saved playlist with confirmation dialog (`y` / `Enter`, `n` / `Esc`) |
| `a` | Mode 3 | Add focused item (folder or file) to active queue |
| `A` (Shift+A) | Mode 3 | Recursively add entire current directory to active queue |
| `Backspace` | Mode 3 | Navigate up one directory level (`..`) |
| `s` | Global | Save current active queue as a new playlist (with overwrite prompt) |
| `u` | Global | Update MPD database (`mpc update`) |
| `Home` / `End` | Global | Jump to first / last item in carousel |
| `Esc` / `q` | Global | Close selector, cancel text input, or dismiss dialog |

### CLI Usage & Integration Examples

#### Launching on Wayland:
```bash
smpd
```

#### Custom MPD host and port:
```bash
smpd --host 192.168.1.100 --port 6600
```

#### Using UNIX domain socket:
```bash
smpd --host /run/mpd/socket
```

#### Custom configuration file:
```bash
smpd --config ~/.config/smpd/custom.toml
```

#### Hyprland keybind example (`hyprland.conf`):
```ini
bind = $mainMod, M, exec, smpd
```

#### Sway keybind example (`~/.config/sway/config`):
```ini
bindsym $mod+m exec smpd
```

### Build & run

```bash
git clone https://github.com/RustBlyat/smpd
cd smpd
cargo build --release
./target/release/smpd
```

### Installing from source (Arch Linux)

An AUR-style `PKGBUILD` is included and builds straight from the local repository:

```bash
git clone https://github.com/RustBlyat/smpd
cd smpd
makepkg -si
```

The package installs the binary to `/usr/bin/smpd`. Runtime dependencies are `gcc-libs`, `vulkan-driver`, and `fontconfig`.

### Notes & limitations

- Requires a Wayland compositor implementing the `zwlr_layer_shell_v1` protocol (e.g. Hyprland, Sway, River, Wayfire, Labwc). Does not run on X11 or Mutter/GNOME (which intentionally omit layer-shell support).
- Exclusive keyboard interactivity is grabbed while open, preventing background window inputs until dismissed with `Esc`.
- Thumbnails are stored uncompressed in `$XDG_CACHE_HOME/smpd/thumbnails/` for sub-millisecond loading.

---

## Русский

Быстрый GPU-ускоренный селектор треков и плейлистов для MPD (Music Player Daemon) под Wayland, написанный на Rust с использованием `wgpu` и `layershellev`. Открывается как нативный оверлей Wayland Layer Shell (`zwlr_layer_shell_v1`) на слое `Overlay` с монопольным захватом клавиатурного фокуса. Каждое превью обложки альбома отображается в виде квадратного параллелограмма с соотношением сторон 1:1 и скосом боковых граней 75°, субпиксельным сглаживанием, динамическим зумом активной карточки, вертикальной «cover»-проекцией текстуры без искажений и индикаторами статуса MPD в реальном времени.

Треки, сохраненные плейлисты и папки библиотеки автоматически считываются из MPD. Обложки извлекаются напрямую из локальных файлов (теги ID3 APIC и изображения в папках) или передаются бинарными блоками по протоколу MPD (`readpicture` и `albumart`), а для треков без обложки процедурно генерируется стилизованный виниловый арт. Горячие клавиши привязаны к физическим сканкодам (`physical_key`), благодаря чему управление работает стабильно при любой раскладке клавиатуры (включая русскую и английскую). Поддерживается детерминированный высокоскоростной дисковый кэш превью, переключение между тремя режимами (Очередь, Плейлисты, Папки), интерактивное сохранение плейлистов с подтверждением перезаписи и TOML-конфиг с автоматическим поиском шрифтов через Fontconfig.

### Возможности

- **Нативный Wayland Layer Shell.** Работает напрямую с протоколом `zwlr_layer_shell_v1` на слое `Layer::Overlay`. Располагается поверх всех окон, рабочих столов и панелей, не отображаясь в списке обычных окон XDG (не требует правил `windowrule` или настройки плавающего режима).
- **Квадратные параллелограммы 1:1 со скосом граней 75°.** Карточки оптимизированы под обложки музыкальных альбомов и имеют соотношение сторон 1:1 ($W = H$) со скосом $75^\circ$ ($\Delta X = H \cdot \cot(75^\circ) \approx 0.268 \times H$).
- **Вертикальная проекция текстуры и сглаживание.** Шейдеры проецируют обложки в режиме «cover» строго вертикально без геометрических перекосов самого изображения, рассчитывая точное расстояние до скошенных границ для субпиксельного антиалиасинга.
- **Двухуровневое извлечение обложек альбомов.** Считывает встроенные обложки напрямую из локальных аудиофайлов (кадры ID3v2 APIC и картинки в папках) за < 0.5 мс либо запрашивает их через протокол MPD (`readpicture` / `albumart`). Для треков без обложки процедурно генерируется виниловый диск.
- **Детерминированный дисковый кэш превью.** Превью уменьшаются до 960×960 и сохраняются в бинарном формате в `$XDG_CACHE_HOME/smpd/thumbnails/`. Загрузка из кэша занимает менее 0.5 мс. Хэш FNV-1a стабилен между перезапусками процесса, исключая лишнюю нагрузку на диск и CPU.
- **Три специализированных режима работы:**
  - **Режим 1 (`1`): Активная очередь.** Просмотр, переключение, пауза и удаление треков из текущей очереди MPD.
  - **Режим 2 (`2`): Сохраненные плейлисты.** Каталог плейлистов с обложками. Клавиша `Enter` очищает текущую очередь, загружает выбранный плейлист и сразу запускает воспроизведение. Поддерживается удаление плейлистов с подтверждением (`d`).
  - **Режим 3 (`3`): Папки библиотеки.** Проводник по каталогам и файлам. Клавиша `a` добавляет выбранный элемент в очередь, а `A` — рекурсивно добавляет всю текущую папку целиком.
- **Управление по сканкодам (не зависит от раскладки).** Все горячие клавиши привязаны к физическим клавишам (`PhysicalKey::Code`). Хоткеи (`h`, `l`, `c`, `s`, `d`, `a`, `u` и др.) работают одинаково при русской, английской или любой другой раскладке.
- **Сохранение плейлистов с защитой от перезаписи.** Нажатие `s` в любом режиме открывает строку ввода для сохранения очереди в плейлист. Если плейлист с таким именем уже есть в MPD, запрашивается подтверждение перезаписи.
- **Обновление базы данных.** Нажатие `u` запускает обновление музыкальной базы MPD (`mpc update`) с мгновенным выводом уведомления в верхней плашке.
- **Управление клавиатурой и мышью.** Поддерживаются стрелки влево/вправо, Vim-навигация (`h`/`l`), колесо мыши и выбор кликом (повторный клик на активную карточку подтверждает выбор). Плавная экспоненциальная интерполяция прокрутки.
- **Конфигурация TOML.** Настройка цветов (в форматах `#RRGGBB`, `#RRGGBBAA` или `[r, g, b, a]`), толщины рамок, прозрачности, скрытия панелей, размера шрифта и кэша в `~/.config/smpd/config.toml`.
- **Автоматический резолвинг шрифтов через Fontconfig.** В параметре `font` по умолчанию используется `"monospace"` (либо `"Noto Sans Mono"`, `"JetBrains Mono"`, `"DepartureMono Nerd Font"` или прямой путь к `.ttf`/`.otf`). Функция динамического подбора символов адаптирует иконки между Nerd Fonts и стандартным Юникодом без пустых рамок (`.notdef`). При отсутствии шрифта используется встроенный растровый шрифт.

### Архитектура

```
src/
├── main.rs          # цикл событий layer-shell (layershellev), обработка сканкодов, управление режимами
├── config.rs        # загрузка TOML-конфига, десериализатор Color, шаблон по умолчанию
├── mpd.rs           # протокол MPD, инспекция очереди/плейлистов/папок, извлечение тегов ID3 APIC
├── loader.rs        # асинхронный загрузчик превью, дисковый кэш FNV-1a, процедурный виниловый арт
├── render.rs        # конвейеры wgpu, WGSL-шейдеры (квадратные параллелограммы, виньетка, оверлей)
└── ui.rs            # резолвер шрифтов (fc-match), рендерер ab_glyph/font8x8, адаптивные символы
```

1. **Окна и события:** Использует `layershellev::WindowState` с параметрами `.with_use_display_handle(true)`, `.with_layer(Layer::Overlay)`, `.with_anchor(Top | Bottom | Left | Right)` и `.with_keyboard_interacivity(KeyboardInteractivity::Exclusive)`.
2. **GPU-рендеринг:** Отрисовка в поверхность `wgpu` через Vulkan (`Backends::PRIMARY`). Три прохода: радиальная виньетка затемнения фона, полигональная сетка квадратных параллелограммов и интерфейсный оверлей.
3. **Многопоточность:** Декодирование файлов, чтение MPD и генерация кэша выполняются асинхронно в фоновом потоке с передачей готовых текстур через каналы.

### Конфигурация

Пути соответствуют спецификации XDG Base Directory:

| Файл | Путь |
|---|---|
| `config.toml` | `$XDG_CONFIG_HOME/smpd/config.toml` (по умолчанию `~/.config/smpd/config.toml`) |
| `thumbnails/` | `$XDG_CACHE_HOME/smpd/thumbnails/` (по умолчанию `~/.cache/smpd/thumbnails/`) |

При первом запуске файл конфигурации создаётся автоматически:

```toml
# smpd configuration file

[ui]
# Показывать верхнюю панель с номером и названием файла
show_top_bar = true

# Показывать нижнюю панель с подсказками управления
show_bottom_bar = true

# Имя гарнитуры (например, "monospace", "Noto Sans Mono", "JetBrains Mono")
# или прямой путь к файлу .ttf/.otf.
# Если пустая строка, используется системный monospace или встроенный растровый шрифт.
font = "monospace"

# Размер шрифта в пикселях
font_size = 20.0

[colors]
# Цвета можно указывать HEX строкой ("#0b0e14", "#0b0e14e1") или массивом ([r, g, b, a])
background_color = "#0b0e14e1"
panel_bg_color = "#10141adc"
panel_border_color = "#00c8ffb4"

card_border_color = "#ffffff40"
selected_border_color = "#00e1ffff"

card_border_width = 1.5
selected_border_width = 4.5
unselected_opacity = 0.65

text_color = "#f5f8ffff"
accent_color = "#00e1ffff"
dim_text_color = "#8c9baff0"

[cache]
# Включить дисковый кэш для моментальной подгрузки
enabled = true

# Максимальный размер превью (960px дает четкую картинку 960x960)
max_thumbnail_size = 960

[mpd]
# Хост сервера MPD (IP, имя хоста или путь к unix socket)
host = "127.0.0.1"
port = 6600
# password = "secret"
# music_directory = "/home/user/Music"
timeout_ms = 3000
# Закрывать ли smpd после выбора трека по Enter (по умолчанию false)
close_on_select = false
```

### Управление

| Клавиша / Ввод | Режим | Действие |
|---|---|---|
| `1` | Глобально | Переключить в **Режим 1: Очередь треков** |
| `2` | Глобально | Переключить в **Режим 2: Сохраненные плейлисты** |
| `3` | Глобально | Переключить в **Режим 3: Папки библиотеки** |
| `Tab` | Глобально | Циклическое переключение: `Очередь → Плейлисты → Папки → Очередь` |
| `Left` / `h` | Глобально | Предыдущий элемент |
| `Right` / `l` | Глобально | Следующий элемент |
| `Колёсико мыши` | Глобально | Прокрутка ленты влево / вправо |
| `Клик ЛКМ` | Глобально | Выбор карточки (клик на уже активную подтверждает выбор) |
| `Enter` / `Numpad Enter` | Контекст | Режим 1: Включить трек. Режим 2: Загрузить плейлист. Режим 3: Открыть папку / добавить файл |
| `Space` | Режим 1 | Пауза / возобновление воспроизведения в MPD |
| `c` | Режим 1 | Центрировать карусель на играющем треке |
| `C` / `x` | Режим 1 | Очистить текущую очередь (`clear`) |
| `n` / `p` | Режим 1 | Следующий / предыдущий трек в MPD |
| `d` / `Delete` | Режим 1 | Удалить выбранный трек из очереди |
| `d` / `Delete` | Режим 2 | Удалить сохраненный плейлист с подтверждением (`y` / `Enter`, `n` / `Esc`) |
| `a` | Режим 3 | Добавить выбранный элемент (папку/файл) в очередь |
| `A` (Shift+A) | Режим 3 | Добавить всю текущую папку целиком в очередь |
| `Backspace` | Режим 3 | Подняться на уровень вверх по каталогам (`..`) |
| `s` | Глобально | Сохранить текущую очередь в новый плейлист (с защитой от перезаписи) |
| `u` | Глобально | Обновить базу данных MPD (`mpc update`) |
| `Home` / `End` | Глобально | Перейти к началу / концу карусели |
| `Esc` / `q` | Глобально | Закрыть программу, отменить ввод или закрыть диалог |

### Примеры интеграции

#### Запуск под Wayland:
```bash
smpd
```

#### Подключение к удаленному MPD:
```bash
smpd --host 192.168.1.100 --port 6600
```

#### Использование через UNIX-сокет:
```bash
smpd --host /run/mpd/socket
```

#### Использование своего файла конфигурации:
```bash
smpd --config ~/.config/smpd/custom.toml
```

#### Пример привязки клавиши в Hyprland (`hyprland.conf`):
```ini
bind = $mainMod, M, exec, smpd
```

#### Пример привязки клавиши в Sway (`~/.config/sway/config`):
```ini
bindsym $mod+m exec smpd
```

### Сборка и запуск

```bash
git clone https://github.com/RustBlyat/smpd
cd smpd
cargo build --release
./target/release/smpd
```

### Установка из исходников (Arch Linux)

В репозитории подготовлен `PKGBUILD` в стиле AUR для локальной сборки:

```bash
git clone https://github.com/RustBlyat/smpd
cd smpd
makepkg -si
```

Бинарник устанавливается в `/usr/bin/smpd`. Для работы требуются `gcc-libs`, `vulkan-driver` и `fontconfig`.

### Примечания и ограничения

- Требуется композитор Wayland с поддержкой протокола `zwlr_layer_shell_v1` (Hyprland, Sway, River, Wayfire, Labwc). Программа не работает в X11 или Mutter/GNOME (где намеренно отсутствует поддержка layer-shell).
- На время открытия захватывается монопольный фокус клавиатуры.
- Превью кэшируются без сжатия в `$XDG_CACHE_HOME/smpd/thumbnails/` для предельной скорости чтения с диска.

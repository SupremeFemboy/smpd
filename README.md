# smpd — MPD Active Track Selector

<p align="center">
  <b>Hardware-accelerated MPD track selector with album art carousel (Wayland Layer Shell + wgpu)</b><br>
  <i>Аппаратно-ускоренный селектор активных треков MPD на Wayland Layer Shell с каруселью обложек (wgpu)</i>
</p>

---

## English

### Overview

**`smpd`** is a high-performance, GPU-accelerated desktop track selector for [MPD (Music Player Daemon)](https://www.musicpd.org/). Built on **Wayland Layer Shell (`layershellev`)** and **`wgpu`** (Vulkan), it displays your active MPD playlist as an interactive horizontal carousel of 1:1 square slanted parallelogram cards featuring album covers extracted in real time from MPD.

### Key Features

- **Wayland Native Layer Shell (`Layer::Overlay`)**: Full-screen exclusive overlay surface via `layershellev`, integrating directly into compositor desktops (Hyprland, Sway, River, Wayfire, Labwc).
- **1:1 Square Parallelogram Geometry**: Cards are shaped as square 1:1 parallelograms with a 75° slant, perfectly tailored for music album covers without cropping essential artwork.
- **Embedded DepartureMono Nerd Font**: Built directly into the binary (`include_bytes!`), providing crisp, stylish retro-monospace typography and Nerd Font icons out of the box without requiring external font installations.
- **MPD Protocol Cover Art Extraction**: Automatically retrieves embedded and folder artwork using MPD's native `albumart` and `readpicture` binary protocols, with local directory fallback (`cover.jpg`, `folder.png`, etc.).
- **Procedural Vinyl Cover Generation**: For tracks without artwork, generates deterministic vinyl gradient covers so the carousel remains visually striking.
- **Instant Startup (< 0.5 ms)**: Pre-scaled RGBA8 disk thumbnail cache (`.thumb` format with `SWTH` binary header).
- **Hardware Vignette Background & SDF Borders**: Smoothstep vignette shader, signed distance field edge AA, and glowing focus borders.
- **Smooth Exponential Easing Animation**: Horizontal scroll carousel with delta-time smoothing:
  $$\Delta t = \min(\text{elapsed}, 0.1)$$
  $$\text{current\_scroll} \mathrel{+}= (\text{target\_scroll} - \text{current\_scroll}) \times (1 - e^{-18 \cdot \Delta t})$$
- **Declarative TOML Configuration**: Strongly typed config located at `~/.config/smpd/config.toml` matching the `swall` schema.

---

### Controls & Keybindings

| Key / Input | Mode | Action |
|---|---|---|
| <kbd>1</kbd> | Global | Switch to **Mode 1**: Active Queue management |
| <kbd>2</kbd> | Global | Switch to **Mode 2**: Saved Playlists selector |
| <kbd>3</kbd> | Global | Switch to **Mode 3**: Library Folders browser |
| <kbd>Tab</kbd> | Global | Cycle modes: `Queue → Playlists → Folders → Queue` |
| <kbd>s</kbd> | Global | **Save current active queue as a new playlist** |
| <kbd>Enter</kbd> | Context | Mode 1: Play track. Mode 2: Clear & load playlist. Mode 3: Open folder / add file |
| <kbd>C</kbd> / <kbd>x</kbd> | Mode 1 | **Clear current active queue** (`clear`) |
| <kbd>Space</kbd> | Mode 1 | Toggle play / pause in MPD |
| <kbd>c</kbd> | Mode 1 | Center carousel on currently playing track |
| <kbd>n</kbd> / <kbd>p</kbd> | Mode 1 | Next / previous track in MPD |
| <kbd>d</kbd> / <kbd>Delete</kbd> | Mode 1 | Remove focused track from active queue |
| <kbd>d</kbd> / <kbd>Delete</kbd> | Mode 2 | **Delete saved playlist with confirmation** (<kbd>y</kbd>/<kbd>Enter</kbd>, <kbd>n</kbd>/<kbd>Esc</kbd>) |
| <kbd>a</kbd> | Mode 3 | Add focused item (folder or file) to active queue |
| <kbd>A</kbd> (Shift+A) | Mode 3 | **Add entire current directory to active queue** |
| <kbd>Backspace</kbd> | Mode 3 | Navigate up one directory level (`..`) |
| <kbd>u</kbd> | Global | **Update MPD database** (`mpc update`) |
| <kbd>←</kbd> / <kbd>→</kbd> or <kbd>h</kbd> / <kbd>l</kbd> | Global | Navigate carousel left / right |
| <kbd>Home</kbd> / <kbd>End</kbd> | Global | Jump to first / last item in carousel |
| <kbd>Mouse Scroll</kbd> | Global | Smooth scroll carousel horizontally |
| <kbd>Mouse Click</kbd> | Global | Click card to focus; click focused card to select |
| <kbd>Esc</kbd> / <kbd>q</kbd> | Global | Close `smpd` |

---

### Configuration (`~/.config/smpd/config.toml`)

```toml
[ui]
show_top_bar = true
show_bottom_bar = true
font = ""
font_size = 20.0

[colors]
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
enabled = true
max_thumbnail_size = 960

[mpd]
host = "127.0.0.1"
port = 6600
# password = "secret"
# music_directory = "/home/user/Music"
timeout_ms = 3000
close_on_select = false
```

---

## Русский

### Описание

**`smpd`** — это высокопроизводительный селектор активных треков для [MPD (Music Player Daemon)](https://www.musicpd.org/) на базе **Wayland Layer Shell (`layershellev`)** и **`wgpu`** (Vulkan). Программа отображает текущий плейлист в виде интерактивной карусели наклонных квадратных параллелограммов (1:1) с обложками альбомов, извлекаемыми напрямую из MPD.

### Основные возможности

- **Переключение режимов по цифрам `1` и `2`**:
  - `1`: Управление и просмотр активного плейлиста (очереди).
  - `2`: Выбор сохраненных плейлистов MPD. При выборе плейлиста текущая очередь очищается, новый плейлист загружается и сразу начинает воспроизводиться, а интерфейс автоматически переключается в режим `1`.
- **Wayland Layer Shell (`Layer::Overlay`)**: Полноэкранный эксклюзивный оверлей на базе `zwlr_layer_shell_v1` (Hyprland, Sway, River, Wayfire, Labwc).
- **Квадратный параллелограмм (1:1)**: Форм-фактор карточек оптимизирован под обложки музыкальных треков (соотношение сторон 1:1 с наклоном 75°).
- **Встроенный шрифт DepartureMono Nerd Font**: Вшит прямо в бинарник (`include_bytes!`), обеспечивая пиксельную эстетику и полный набор иконок без внешних зависимостей.
- **Извлечение обложек из MPD**: Получение обложек по протоколам `albumart` и `readpicture`, а также локальный поиск (`cover.jpg`, `folder.png` и т.д.).
- **Генерация процедурных обложек**: Для треков без обложки создается виниловый диск со стилизованным градиентом.
- **Мгновенный запуск (< 0.5 мс)**: Бинарный дисковый кэш миниатюр (`.thumb` с заголовком `SWTH`).
- **SDF-шейдеры и виньетка фона**: Плавное субпиксельное сглаживание границ (AA) и мягкая виньетка фона.
- **Плавный скролл**: Экспоненциальная анимация на любой частоте обновления (60/144/240 Гц).
- **Конфигурация**: Конфиг `~/.config/smpd/config.toml` с полной поддержкой секций `[ui]`, `[colors]`, `[cache]` и `[mpd]`.

---

## Управление

| Клавиша | Режим | Действие |
|---|---|---|
| <kbd>1</kbd> | Глобально | Режим 1: Управление текущей очередью треков |
| <kbd>2</kbd> | Глобально | Режим 2: Выбор сохраненного плейлиста |
| <kbd>3</kbd> | Глобально | Режим 3: Проводник по папкам библиотеки |
| <kbd>Tab</kbd> | Глобально | Переключение вкладок `1 → 2 → 3 → 1` |
| <kbd>s</kbd> | Глобально | **Сохранить текущую очередь в новый плейлист** |
| <kbd>Enter</kbd> | Контекст | Включить трек / загрузить плейлист / войти в папку |
| <kbd>C</kbd> / <kbd>x</kbd> | Режим 1 | **Очистить текущую очередь** (`clear`) |
| <kbd>Space</kbd> | Режим 1 | Пауза / воспроизведение в MPD |
| <kbd>c</kbd> | Режим 1 | Центрировать на играющем треке |
| <kbd>n</kbd> / <kbd>p</kbd> | Режим 1 | Следующий / предыдущий трек в MPD |
| <kbd>Delete</kbd> / <kbd>d</kbd> | Режим 1 | Удалить выбранный трек из очереди |
| <kbd>d</kbd> / <kbd>Delete</kbd> | Режим 2 | **Удалить плейлист с подтверждением** (<kbd>y</kbd>/<kbd>Enter</kbd>, <kbd>n</kbd>/<kbd>Esc</kbd>) |
| <kbd>a</kbd> | Режим 3 | Добавить выбранный элемент (папку/файл) в очередь |
| <kbd>A</kbd> (Shift+A) | Режим 3 | **Добавить всю текущую директорию в очередь сразу** |
| <kbd>Backspace</kbd> | Режим 3 | Подняться на уровень вверх по каталогам |
| <kbd>u</kbd> | Глобально | **Обновить базу данных MPD** (`mpc update`) |
| <kbd>←</kbd> / <kbd>→</kbd> или <kbd>h</kbd> / <kbd>l</kbd> | Глобально | Навигация по карусели |
| <kbd>Home</kbd> / <kbd>End</kbd> | Глобально | Перейти к началу / концу карусели |
| <kbd>Колесо мыши</kbd> | Глобально | Прокрутка карусели |
| <kbd>Клик мыши</kbd> | Глобально | Клик по карточке — сфокусироваться; повторный клик — выбрать |
| <kbd>Esc</kbd> / <kbd>q</kbd> | Глобально | Выход из программы |

---

## Сборка и запуск

```bash
# Тесты
cargo test --release

# Релизная сборка
cargo build --release

# Запуск
./target/release/smpd
```

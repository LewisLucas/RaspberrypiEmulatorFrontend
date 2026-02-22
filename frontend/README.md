Raspberry Pi Emulator Frontend (prototype)

This is a minimal runnable prototype written in Rust + SDL2. It shows a fullscreen grid of ROM files
and supports controller + keyboard navigation. Press A / Enter to launch the selected ROM using a
command template, and the frontend waits for the emulator process to exit and then returns.

Quick start
- Install dependencies (on Arch Linux):
  - sudo pacman -S --needed base-devel sdl2 sdl2_ttf
- Build:
  - cd frontend
  - cargo build --release
- Run (point to a ROMs directory):
  - ./target/release/rpi_emulator_frontend /path/to/roms

Environment
- EMULATOR_CMD: command template used to launch a ROM. Use "{rom}" where the ROM path should go.
  - Example: EMULATOR_CMD="mgba {rom}" ./target/release/rpi_emulator_frontend ./roms

Notes about testing on Wayland vs X11
- This prototype uses SDL2 for windowing and controller input. SDL2 will pick the appropriate
  backend on your development machine (Wayland on your Arch system). That is fine for development
  and testing — input and rendering will behave similarly.
- On the Raspberry Pi we plan to run under X11 for emulator compatibility. SDL2 supports X11; when
  you run the same binary on the Pi (with X11) SDL2 will use the X11 backend. Some behaviours
  (fullscreen handling, controller device names) may differ slightly between backends; final tests
  should be done on the Pi.

Next steps
- Add text rendering (SDL_ttf) to show game names and status.
- Add thumbnail loading and caching.
- Implement a controller remapping UI and persistent config.

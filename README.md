# rwasio

Rusty Wine ASIO is an ASIO driver for Wine that wires directly to JACK or PipeWire, written in Rust.

Windows DAWs running in Wine see it as a regular ASIO driver. Under the hood it bypasses Windows entirely and talks to the Linux audio server natively, with no emulation overhead.

## Why

WineASIO is broken on recent Wine versions and hasn't seen meaningful updates in years. rwasio is a clean rewrite in Rust with a few things it never had:
- Native PipeWire support (not just JACK)
- 64-bit only (no 32-bit baggage, Wine's WoW64 covers it)

## Status

Functional. FL Studio loads the driver, enumerates devices, plays and records audio through PipeWire. A GTK4/libadwaita control panel lets you pick input/output devices and buffer size at runtime, with a live diagnostics page.

<img width="813" height="942" alt="image" src="https://github.com/user-attachments/assets/b3c5b9ea-78ee-45f4-b02d-8153edea229a" />

## Requirements

- **Rust** (stable, recent) with `cargo`. The crate uses edition 2024.
- **Wine**, 64-bit, including its developer tools `winebuild` and `winegcc`. These ship with Wine but some distros split them into a separate package (e.g. `wine`, `wine-devel`, `wine-staging`, or `wine-tools`). Names vary.
- **pkg-config**.
- Development headers for **GTK4**, **libadwaita**, and **PipeWire**. Package names differ per distro, for example:
  - GTK4: `gtk4-devel` / `libgtk-4-dev`
  - libadwaita: `libadwaita-devel` / `libadwaita-1-dev`
  - PipeWire: `pipewire-devel` / `libpipewire-0.3-dev`
- A running **PipeWire** session (PipeWire is the only backend for now; JACK is planned).
- The **Wine prefix** your DAW runs in, and the **matching Wine build**. This can be plain `~/.wine`, a [Bottles](https://usebottles.com) bottle, a Proton/Steam prefix, Lutris, etc. Whatever launches your DAW is what you must register against.

## Build

```sh
cargo build --release

# Native code half (what Wine actually runs)
winegcc -m64 -shared rwasio.dll.spec target/release/librwasio.a \
    $(pkg-config --libs gtk4 libadwaita-1 libpipewire-0.3) -o rwasio.dll.so

# Fake PE module half (what Wine loads by name)
winebuild --dll --fake-module -E rwasio.dll.spec -o rwasio.dll
```

This produces two files, and you need both:

- `rwasio.dll`. A fake Windows PE module. It contains no real code; it just lets Wine resolve `rwasio.dll` by name.
- `rwasio.dll.so`. The actual native ELF library. Wine's builtin-DLL loader `dlopen`s this and runs it.

## Install

A Wine builtin DLL has two halves that go in two **different** places.

### 1. The PE stub → the prefix's system32

Copy `rwasio.dll` into your Wine prefix:

```
<prefix>/drive_c/windows/system32/rwasio.dll
```

How to find `<prefix>` (your `WINEPREFIX`):
- **Plain Wine:** defaults to `~/.wine`.
- **Bottles:** `~/.var/app/com.usebottles.bottles/data/bottles/bottles/<BottleName>`.
- **Proton / Steam:** `~/.steam/steam/steamapps/compatdata/<appid>/pfx` (or your launcher's per-game prefix).

### 2. The native `.so` → Wine's builtin search path

Wine finds builtin `.so` libraries via `WINEDLLPATH` and its own library directory. Two options:

- **Recommended (version-proof):** keep `rwasio.dll.so` anywhere and tell Wine where it is with `WINEDLLPATH=/abs/path/to/dir`. Set it both when registering and when launching the DAW.
- **Or** copy it into your Wine build's unix DLL directory, ending in `.../lib/wine/x86_64-unix/`. To locate it for system Wine:
  ```sh
  ls /usr/lib*/wine/x86_64-unix 2>/dev/null
  # or, more thoroughly:
  find / -type d -name x86_64-unix -path '*wine*' 2>/dev/null
  ```
  Bottles and Proton each ship their own Wine, so this directory lives under that specific runner/Proton install, not the system one.

> **Do not** put `rwasio.dll.so` in `system32`. Wine ignores `.so` files there.
>
> **Watch for stale copies.** The builtin directory is searched before the prefix, so an old `rwasio.dll.so` left there will silently shadow a new build. Overwrite it when you rebuild.
>
> **Each Wine build/runner has its own builtin directory.** If you switch or upgrade your runner, install the `.so` into the new one again (rebuild it with that runner if it fails to load). Switching back to the old runner keeps working because its copy is still there.

## Register

Register with the **same Wine build** your DAW uses, against the right prefix, with the `.so` reachable:

```sh
WINEPREFIX=/path/to/prefix \
WINEDLLPATH=/dir/with/rwasio.dll.so \
WINEDLLOVERRIDES='rwasio=b' \
wine regsvr32 rwasio.dll
```

- `rwasio=b` forces the builtin (our native `.so`) over the fake PE stub.
- This writes the ASIO registry entry (`HKLM\Software\ASIO\Rusty Wine ASIO`) and the COM CLSID.
- Unregister with `wine regsvr32 /u rwasio.dll`.

Matching the Wine build matters:
- **Bottles:** run the bottle's own runner wine, e.g. via `flatpak run --command=bash com.usebottles.bottles` then call that runner's `wine` with the bottle's `WINEPREFIX`.
- **Proton:** use the Proton build's `wine` (and set `WINEDLLPATH` to the dir holding the `.so`).

Registering with a different Wine than the one that runs the DAW will not work.

## Use

In your DAW's audio/ASIO settings, select **Rusty Wine ASIO**. Open the driver's control panel to pick input/output devices (or leave them on "Follow default") and the buffer size.

## Uninstall

```sh
WINEPREFIX=/path/to/prefix wine regsvr32 /u rwasio.dll
rm <prefix>/drive_c/windows/system32/rwasio.dll
rm <builtin-dir>/rwasio.dll.so      # or simply stop setting WINEDLLPATH
```

## Todo

- [x] PoC: call Linux natively from a fake Windows DLL loaded by Wine
- [x] ASIO type definitions (C ABI), IASIO/IUnknown vtables, `Asio`/`AsioClass` traits
- [x] Generic COM scaffolding (refcounted object, class factory, DLL exports macro)
- [x] ASIO interface implementation (concrete driver)
- [x] DllRegisterServer / DllUnregisterServer (Wine registry)
- [x] Detected and opened by FL Studio at 44100Hz, Float32LSB
- [x] PipeWire backend (output + capture)
- [x] Fire buffer_switch from a Wine-aware thread
- [x] Actual audio output
- [x] Audio device selection GUI (GTK4/libadwaita control panel)
- [ ] JACK backend

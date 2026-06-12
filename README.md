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

# rwasio

Rusty Wine ASIO is an ASIO driver for Wine that wires directly to JACK or PipeWire, written in Rust.

Windows DAWs running in Wine see it as a regular ASIO driver. Under the hood it bypasses Windows entirely and talks to the Linux audio server natively, with no emulation overhead.

## Why

WineASIO is broken on recent Wine versions and hasn't seen meaningful updates in years. rwasio is a clean rewrite in Rust with a few things it never had:
- Native PipeWire support (not just JACK)
- 64-bit only (no 32-bit baggage, Wine's WoW64 covers it)

## Status

Early stages. Currently, DAW such as FL Studio can load it and detected everything correctly, i can also load the control panel too. Now just need to wire the pipewire/jack thing..

<img width="677" height="564" alt="image" src="https://github.com/user-attachments/assets/4ec06129-cbde-4b11-a716-7f1759c71885" />

## Todo

- [x] PoC: call Linux natively from a fake Windows DLL loaded by Wine
- [x] ASIO type definitions (C ABI), IASIO/IUnknown vtables, `Asio`/`AsioClass` traits
- [x] Generic COM scaffolding (refcounted object, class factory, DLL exports macro)
- [x] ASIO interface implementation (concrete driver)
- [x] DllRegisterServer / DllUnregisterServer (Wine registry)
- [x] Detected and opened by FL Studio at 44100Hz, Float32LSB
- [ ] JACK backend
- [ ] PipeWire backend
- [ ] Fire buffer_switch from a Wine-aware thread
- [ ] Actual audio output

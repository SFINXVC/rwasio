# rwasio

Rusty Wine ASIO is an ASIO driver for Wine that wires directly to JACK or PipeWire, written in Rust.

Windows DAWs running in Wine see it as a regular ASIO driver. Under the hood it bypasses Windows entirely and talks to the Linux audio server natively, with no emulation overhead.

## Why

WineASIO is broken on recent Wine versions and hasn't seen meaningful updates in years. rwasio is a clean rewrite in Rust with a few things it never had:
- Native PipeWire support (not just JACK)
- 64-bit only (no 32-bit baggage, Wine's WoW64 covers it)

## Status

Early stages. Currently a proof of concept.

## Todo

- [x] PoC: call Linux natively from a fake Windows DLL loaded by Wine
- [x] ASIO type definitions (C ABI), IASIO/IUnknown vtables, `Asio`/`AsioClass` traits
- [x] Generic COM scaffolding (refcounted object, class factory, DLL exports macro)
- [ ] ASIO interface implementation (concrete driver)
- [ ] DllRegisterServer / DllUnregisterServer (Wine registry)
- [ ] JACK backend
- [ ] PipeWire backend
- [ ] Buffer management and audio routing
- [ ] Test with a real DAW / Audio Software (FL Studio, etc)
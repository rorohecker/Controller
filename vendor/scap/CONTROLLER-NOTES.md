Vendored from the published `scap 0.1.0-beta.1` crate (MIT). Original license and VCS metadata are retained.

CONTROLLER uses its public engine API only on macOS/Linux, with cancellable receive timeouts, four frames per second, low-resolution video, and audio capture disabled. Windows retains the existing GDI sampler.

The unused Windows CPAL dependency is aligned to 0.18.2 because Cargo resolves native `links` dependencies across targets; the upstream 0.15.3 dependency conflicts with CONTROLLER's ALSA bindings even on non-Windows builds. This fork is not used as a Windows capture backend.

Keep upstream changes and these adaptations separate when updating the dependency. Desktop-portal consent and actual Mac/Linux capture still require testing on those systems.

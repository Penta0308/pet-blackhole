# Pet Blackhole

A desktop pet that looks like a faint LCD pressure mark.

It is not meant to be a space black hole with rings or particles. The idea is closer to the dull brown smudge you see when an LCD panel is pressed: translucent, a little uneven, and slightly alive.

You can drag it around. When it gets close to the edge of the screen, its outline is deformed by a small soft-body simulation so it squashes instead of staying circular. After a while it can wander on its own.

## Running

Install Rust and the Vulkan SDK. `glslc` must be available on `PATH`.

```sh
cargo run
```

Right-click the pet to quit.

## Status

This is an early prototype. It currently targets a native desktop app with Vulkan rendering and no web view.

## Copyright

Copyright (c) 2026 Penta0308. All rights reserved.

Provided as-is, without warranty. Use at your own risk.

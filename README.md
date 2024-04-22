Old text editor project that was in the early stages. Mostly just groundwork.

Wanted to explore writing a low-latency GPU-accelerated text editor, using compute shaders for rendering (only supporting monospaced fonts, as a simplification, inspired by `zutty`) and an input system that would easily support different keyboard layouts (by using scancodes instead of keycodes for shortcuts, as video games tend to do).
In practice this iteration focused on the rendering side and testing the viability of writing every component near enough from the ground up, as at this time `winit` didn't feel flexible enough (especially the input API) and `wgpu` was still immature.

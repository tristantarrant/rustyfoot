# VST3 Support for Rustyfoot

## Goal

Make VST3 plugins appear as first-class LV2 pedals in the rustyfoot pedalboard UI, with auto-generated modgui web controls.

## Approach

Use [Carla's backend](https://github.com/falkTX/Carla) as the DSP bridge between LV2 and VST3. Carla's backend has no external dependencies (no Qt/PyQt) and natively supports loading VST3 plugins. [Ildaeil](https://github.com/DISTRHO/Ildaeil) (Carla backend + [DPF](https://github.com/DISTRHO/DPF)) was explored as a ready-made solution but can't be used as-is — it exposes 0 LV2 parameters (hosted plugin params are internal to Carla's XML state, not visible to the LV2 host). Instead, we build a custom LV2 wrapper that uses Carla's backend API for VST3 lifecycle management while exposing real LV2 control ports.

Rather than shipping a single generic host plugin, we generate a dedicated LV2 bundle per VST3 plugin, so each appears as its own pedal with its own modgui.

## Bundle Architecture

### Generated Bundle Layout (per wrapped VST3)

```
springs.lv2/
├── manifest.ttl                    # URI, lv2:binary, rdfs:seeAlso
├── springs.ttl                     # Port definitions (audio I/O + control ports)
├── wrapper.json                    # VST3 path, class ID, parameter mapping
├── modgui.ttl                      # modgui metadata
├── modgui/                         # Web UI assets (icon, CSS, screenshots, knobs)
└── vst3-wrapper.so -> /usr/lib/rustyfoot/vst3-wrapper.so
```

### Shared Wrapper Binary (`vst3-wrapper.so`)

A single C++ shared library, built once, symlinked into each generated `.lv2` bundle.

**Key mechanism: `lv2_lib_descriptor(bundle_path, features)`** — the LV2 core API that receives the bundle path at load time. This lets a single `.so` serve different URIs per bundle by reading a per-bundle `wrapper.json`. Verified supported by the system's lilv (`liblilv-0.so` exports both `lv2_descriptor` and `lv2_lib_descriptor`).

The standard `lv2_descriptor(index)` does NOT receive the bundle path, so a single .so can't know which URI to return. With `lv2_lib_descriptor()`, each bundle's symlink to the same .so gets a different `bundle_path` argument, solving this cleanly.

**Runtime flow:**
1. lilv reads `manifest.ttl`, finds URI and `lv2:binary`
2. Loads `vst3-wrapper.so` and calls `lv2_lib_descriptor(bundle_path, features)`
3. Wrapper reads `wrapper.json` from `bundle_path` — gets URI, VST3 path, class ID, parameter mapping
4. Returns `LV2_Lib_Descriptor` whose `get_plugin()` returns an `LV2_Descriptor` with the correct URI
5. `instantiate()`: loads the VST3 via Carla backend API (`carla_get_native_rack_plugin()`, same pattern as Ildaeil)
6. `connect_port()`: stores pointers for audio buffers and control port values
7. `run()`: syncs changed control port values to VST3 parameters (LV2 denormalized → VST3 normalized via `(value - min) / (max - min)`), calls Carla `process()`
8. `LV2_State_Interface`: saves/restores VST3 state as LV2 state properties (chunk data + individual parameter values)

**Uses Carla's backend** (not raw VST3 SDK) because:
- Carla handles VST3 lifecycle (COM factory, IComponent/IEditController separation, bus activation)
- Threading and RT-safety are proven (Ildaeil uses the same path in JACK audio thread)
- Parameter normalization, MIDI event translation, state serialization are handled
- Supports CLAP/LADSPA/DSSI too if needed in the future

### `wrapper.json` Format

```json
{
  "vst3_path": "/var/lib/rustyfoot/vst3/springs.vst3",
  "vst3_class_id": "59696d52616b68656553707269000000",
  "uri": "urn:rustyfoot:vst3:springs",
  "name": "Springs",
  "audio_inputs": 2,
  "audio_outputs": 2,
  "parameters": [
    {
      "lv2_index": 0,
      "lv2_symbol": "drive",
      "lv2_name": "Drive",
      "vst3_param_id": 0,
      "min": 0.0,
      "max": 24.0,
      "default": 0.0,
      "unit": "dB"
    },
    {
      "lv2_index": 1,
      "lv2_symbol": "predelay",
      "lv2_name": "Pre-delay",
      "vst3_param_id": 1,
      "min": 0.0,
      "max": 100.0,
      "default": 10.0,
      "unit": "ms"
    }
  ]
}
```

### Ildaeil Plugin Variants

Ildaeil ships three LV2 plugin variants, each with different I/O configurations:

| Variant | Audio In | Audio Out | MIDI In | MIDI Out | LV2 Category | Use case |
|---------|----------|-----------|---------|----------|--------------|----------|
| **FX** | 2 | 2 | 0 | 0 | UtilityPlugin | Stereo audio effects (reverb, delay, distortion) |
| **MIDI** | 0 | 0 | 1 | 1 | MIDIPlugin | Pure MIDI processors (no audio) |
| **Synth** | 0 | 2 | 1 | 0 | InstrumentPlugin | Instruments (receive MIDI, produce audio) |

The wrapper generator selects the correct variant based on the VST3's declared categories and I/O layout:
- `Fx|Stereo` → FX variant
- `Instrument` / synth with MIDI input → Synth variant
- MIDI-only (no audio I/O) → MIDI variant

Most guitar pedal VST3s (like Springs) are audio effects and use the **FX** variant.

### Headless Build (No GUI)

Ildaeil's UI is built with Dear ImGui via DPF's DGL, requiring OpenGL and X11. Since rustyfoot uses web-based modgui instead, the UI is disabled via a build patch that sets `DISTRHO_PLUGIN_HAS_UI` to `0` in each variant's `DistrhoPluginInfo.h`. This drops all GUI dependencies — no OpenGL, X11, Xcursor, Xext, or Xrandr needed on the Pi.

The patch is applied automatically by the rustyfoot-build plugin builder (`plugins/ildaeil/disable-ui.patch`).

### No Native GUI Rendering

The native VST3 GUI is not used. Instead, modgui provides web-based controls (sliders, knobs, toggles) mapped to parameters — the same approach used for all other pedals in rustyfoot. This avoids the complexity of server-side bitmap rendering + browser canvas streaming.

If native GUI rendering is ever desired, the approach would be: render to offscreen framebuffer on the server, stream compressed frames via WebSocket to a `<canvas>`, and map browser input events back to the plugin's UI event handler (essentially VNC-for-plugins).

## Scanner Tool (`rustyfoot-vst3-scan`)

CLI tool that introspects a `.vst3` bundle and generates the complete `.lv2` wrapper bundle.

**Input:** path to `.vst3` bundle + output directory
**Output:** complete `.lv2` bundle ready for installation

**Process:**
1. Loads VST3 via Carla's API, enumerates parameters (`carla_get_parameter_count()`, `carla_get_parameter_info()`, `carla_get_parameter_ranges()`)
2. Reads audio I/O bus configuration, VST3 class ID, subcategories
3. Generates:
   - `wrapper.json` — config for the wrapper .so
   - `manifest.ttl` — URI, binary reference, rdfs:seeAlso
   - `<plugin>.ttl` — full port definitions with ranges, units, categories
   - `modgui.ttl` + `modgui/` — web UI assets using the template system from `rustyfoot-build/build.sh:generate_modgui()` (same pedal backgrounds, knob sprites, Mustache HTML templates, screenshot compositing)
4. Creates symlink to `vst3-wrapper.so`

**URI scheme:** `urn:rustyfoot:vst3:<lowercase_plugin_name>` — deterministic, stable across regeneration.

## Integration with Rustyfoot Plugin Builder

VST3 support integrates into the existing `rustyfoot-build` pipeline. VST3 plugins get a **descriptor** that marks them as needing special handling. The builder compiles the VST3, then runs `rustyfoot-vst3-scan` to generate the wrapper `.lv2` bundle.

### Plugin Builder Flow

For a VST3 plugin like Springs, the descriptor (`plugins/springs/descriptor.yaml`) builds the VST3 then runs the scanner:

```yaml
install:
  - mkdir -p ${PREFIX_DIR}/lib/vst3
  - cp -rv target/bundled/springs.vst3 ${PREFIX_DIR}/lib/vst3/
  - rustyfoot-vst3-scan ${PREFIX_DIR}/lib/vst3/springs.vst3 ${LV2_DIR}/
bundles:
  - springs.lv2
```

### Rustyfoot Store (future)

In `src/web/handlers/store.rs`, the install flow would detect `.vst3` bundles in extracted archives, move them to `$MOD_DATA_DIR/vst3/`, run the scanner, and proceed with the generated `.lv2` bundle through the normal `bundle_add` path. The `plugin_cache.rs` filesystem watcher auto-detects new `.lv2` bundles.

### Rescan / Rebuild

If the wrapper binary is updated (new Carla backend version), all generated LV2 bundles need regeneration. The scanner can re-walk `$MOD_DATA_DIR/vst3/` and regenerate the `.lv2` wrappers. The existing `plugin_cache` filesystem watcher detects the new/changed `.lv2` bundles and refreshes automatically.

## Build

### Components

- **`vst3-wrapper.so`** — C++ shared library, links against Carla's backend (static or bundled)
- **`rustyfoot-vst3-scan`** — C++ CLI tool, uses Carla's API for VST3 introspection + generates TTL/modgui

Both built from a new `vst3-wrapper/` directory in rustyfoot or rustyfoot-build.

### Dependencies

- Carla backend (C/C++, no external deps — built from Ildaeil submodule)
- LV2 headers (already a dependency)
- No DPF needed — the wrapper implements the LV2 plugin API directly

### Installed Files

```
/usr/lib/rustyfoot/vst3-wrapper.so      # Shared wrapper binary
/usr/bin/rustyfoot-vst3-scan            # Scanner/generator tool

/var/lib/rustyfoot/vst3/                # Installed VST3 bundles (source)
/var/lib/rustyfoot/plugins/             # Generated LV2 wrappers (alongside native LV2 plugins)
```

## Reference Plugin: Springs

[Springs](https://codeberg.org/yimrakhee/springs) — a spring reverb emulator by yimrakhee. Rust/nih-plug, GPLv3, VST3 + CLAP (no LV2). Good test case because it's simple, well-defined, and open source.

### Characteristics

- **I/O**: Stereo in, stereo out (also supports mono)
- **Parameters** (6 + bypass):
  - Drive — soft-clipper saturation, 0–24 dB
  - Pre-delay — 0–100 ms delay before reverb onset
  - Tension — dispersion intensity, 0–1
  - Tone — low-pass filter in feedback loop, 500–12000 Hz (skewed)
  - Dwell — internal feedback / reverb tail length, 0–0.98
  - Mix — dry/wet balance, 0–1
  - Bypass — bool
- **VST3 class ID**: `YimRakheeSprings` (16 bytes)
- **VST3 subcategories**: Fx, Reverb
- **GUI**: egui-based (disabled in build via patch — replaced by generated modgui)
- **Build**: `cargo build --release` → `libsprings.so` → bundled as `springs.vst3`
- **Builder descriptor**: `rustyfoot-build/plugins/springs/` with `disable-gui.patch`

### Expected Wrapper Output

```
springs.lv2/
├── manifest.ttl          # urn:rustyfoot:vst3:springs, lv2:binary
├── springs.ttl           # 2 audio in, 2 audio out, 6 control ports + bypass
├── wrapper.json          # VST3 path, class ID, parameter mapping
├── modgui.ttl            # brand, label, knobs
├── modgui/
│   ├── icon-springs.html
│   ├── stylesheet-springs.css
│   ├── screenshot-springs.png
│   ├── thumbnail-springs.png
│   ├── pedals/boxy/...
│   └── knobs/boxy/...
└── vst3-wrapper.so -> /usr/lib/rustyfoot/vst3-wrapper.so
```

## Implementation Phases

### Phase 1: Wrapper Binary (`vst3-wrapper.so`)
- C++ source in `vst3-wrapper/`
- Implements `lv2_lib_descriptor()`, `instantiate()`, `connect_port()`, `run()`, `activate()`, `deactivate()`, `cleanup()`
- Reads `wrapper.json` at load time for per-bundle configuration
- Loads VST3 via Carla backend, maps LV2 ports to VST3 parameters
- Implements `LV2_State_Interface` for state persistence
- Test with a manually-crafted Springs `.lv2` bundle

### Phase 2: Scanner Tool (`rustyfoot-vst3-scan`)
- C++ CLI using Carla's API for VST3 introspection
- TTL generation (manifest + plugin description + modgui)
- modgui asset generation (reuse template system from build.sh or port to C++)
- Test: `rustyfoot-vst3-scan springs.vst3 /tmp/out/` → verify bundle loads in mod-host

### Phase 3: Build Integration
- Add `vst3-wrapper/` build to Makefile
- Update Springs descriptor to run scanner after build
- Package wrapper .so and scanner in debian rules

### Phase 4: Store Integration (Rust, future)
- Detect `.vst3` in downloaded archives
- Shell out to `rustyfoot-vst3-scan`
- Rescan endpoint for wrapper binary updates

## Open Questions

- How to handle VST3 plugins with variable I/O channel counts (some support mono, stereo, or surround)?
- MIDI: VST3 uses a different event model than LV2 atom MIDI — Carla handles this internally, but the wrapper's LV2 TTL needs to declare MIDI ports for instrument/synth variants.
- Latency reporting: VST3 plugins may report processing latency. The wrapper should expose an LV2 latency output port if the VST3 reports non-zero latency.
- Plugin categories: Map VST3 subcategories to LV2 classes for proper categorization in the UI.
- Store integration: Which stores would carry VST3 plugins? Patchstorage already supports multiple platforms — could add a VST3/Linux target. Or a new store backend for VST3-specific sources.

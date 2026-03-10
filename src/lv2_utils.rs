// LV2 plugin utilities FFI wrapper, ported from modtools/utils.py
// Wraps libmod_utils.so for LV2 plugin introspection, pedalboard info, and JACK operations.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_float, c_int, c_uint};
use std::sync::OnceLock;

use serde_json::{json, Value};

// ---- FFI types matching the C library (from utils.h) ----

#[repr(C)]
struct PluginAuthor {
    name: *const c_char,
    homepage: *const c_char,
    email: *const c_char,
}

#[repr(C)]
struct PluginGUIPort {
    valid: bool,
    index: c_uint,
    name: *const c_char,
    symbol: *const c_char,
}

#[repr(C)]
struct PluginGUI {
    resources_directory: *const c_char,
    icon_template: *const c_char,
    settings_template: *const c_char,
    javascript: *const c_char,
    stylesheet: *const c_char,
    screenshot: *const c_char,
    thumbnail: *const c_char,
    discussion_url: *const c_char,
    documentation: *const c_char,
    brand: *const c_char,
    label: *const c_char,
    model: *const c_char,
    panel: *const c_char,
    color: *const c_char,
    knob: *const c_char,
    ports: *const PluginGUIPort,
    monitored_outputs: *const *const c_char,
}

#[repr(C)]
struct PluginGUIMini {
    resources_directory: *const c_char,
    screenshot: *const c_char,
    thumbnail: *const c_char,
}

#[repr(C)]
struct PluginPortRanges {
    min: c_float,
    max: c_float,
    def: c_float,
}

#[repr(C)]
struct PluginPortUnits {
    label: *const c_char,
    render: *const c_char,
    symbol: *const c_char,
    _custom: bool,
}

#[repr(C)]
struct PluginPortScalePoint {
    valid: bool,
    value: c_float,
    label: *const c_char,
}

#[repr(C)]
struct PluginPort {
    valid: bool,
    index: c_uint,
    name: *const c_char,
    symbol: *const c_char,
    ranges: PluginPortRanges,
    units: PluginPortUnits,
    comment: *const c_char,
    designation: *const c_char,
    properties: *const *const c_char,
    range_steps: c_int,
    scale_points: *const PluginPortScalePoint,
    short_name: *const c_char,
}

#[repr(C)]
struct PluginPortsI {
    input: *const PluginPort,
    output: *const PluginPort,
}

#[repr(C)]
struct PluginPorts {
    audio: PluginPortsI,
    control: PluginPortsI,
    cv: PluginPortsI,
    midi: PluginPortsI,
}

#[repr(C)]
struct PluginLongParameterRanges {
    min: i64,
    max: i64,
    def: i64,
}

// Union inside PluginParameterRanges — we use the largest variant size
#[repr(C)]
union PluginParameterRangesUnion {
    f: std::mem::ManuallyDrop<PluginPortRanges>,
    l: std::mem::ManuallyDrop<PluginLongParameterRanges>,
    s: *const c_char,
}

#[repr(C)]
struct PluginParameterRanges {
    type_: c_char,
    u: PluginParameterRangesUnion,
}

#[repr(C)]
struct PluginParameter {
    valid: bool,
    readable: bool,
    writable: bool,
    uri: *const c_char,
    label: *const c_char,
    type_: *const c_char,
    ranges: PluginParameterRanges,
    units: PluginPortUnits,
    comment: *const c_char,
    short_name: *const c_char,
    file_types: *const *const c_char,
    supported_extensions: *const *const c_char,
}

#[repr(C)]
struct PluginPreset {
    valid: bool,
    uri: *const c_char,
    label: *const c_char,
    path: *const c_char,
}

#[repr(C)]
struct PluginInfo {
    valid: bool,
    uri: *const c_char,
    name: *const c_char,
    binary: *const c_char,
    brand: *const c_char,
    label: *const c_char,
    license: *const c_char,
    comment: *const c_char,
    build_environment: *const c_char,
    category: *const *const c_char,
    micro_version: c_int,
    minor_version: c_int,
    release: c_int,
    builder: c_int,
    licensed: c_int,
    iotype: c_int,
    has_external_ui: bool,
    version: *const c_char,
    stability: *const c_char,
    author: PluginAuthor,
    bundles: *const *const c_char,
    gui: PluginGUI,
    ports: PluginPorts,
    parameters: *const PluginParameter,
    presets: *const PluginPreset,
}

#[repr(C)]
struct NonCachedPluginInfo {
    licensed: c_int,
    presets: *const PluginPreset,
}

#[repr(C)]
struct PluginInfoMini {
    uri: *const c_char,
    name: *const c_char,
    brand: *const c_char,
    label: *const c_char,
    comment: *const c_char,
    build_environment: *const c_char,
    category: *const *const c_char,
    micro_version: c_int,
    minor_version: c_int,
    release: c_int,
    builder: c_int,
    licensed: c_int,
    iotype: c_int,
    gui: PluginGUIMini,
}

#[repr(C)]
struct PedalboardInfoMini {
    broken: bool,
    factory: bool,
    has_trial_plugins: bool,
    uri: *const c_char,
    bundle: *const c_char,
    title: *const c_char,
    version: c_uint,
}

#[repr(C)]
struct PedalboardMidiControl {
    channel: i8,
    control: u8,
    has_ranges: bool,
    minimum: c_float,
    maximum: c_float,
}

#[repr(C)]
struct PedalboardPluginPort {
    valid: bool,
    symbol: *const c_char,
    value: c_float,
    midi_cc: PedalboardMidiControl,
}

#[repr(C)]
struct PedalboardPlugin {
    valid: bool,
    bypassed: bool,
    instance_number: c_int,
    instance: *const c_char,
    uri: *const c_char,
    bypass_cc: PedalboardMidiControl,
    x: c_float,
    y: c_float,
    ports: *const PedalboardPluginPort,
    preset: *const c_char,
}

#[repr(C)]
struct PedalboardConnection {
    valid: bool,
    source: *const c_char,
    target: *const c_char,
}

#[repr(C)]
struct PedalboardHardwareMidiPort {
    valid: bool,
    symbol: *const c_char,
    name: *const c_char,
}

#[repr(C)]
struct PedalboardHardware {
    audio_ins: c_uint,
    audio_outs: c_uint,
    cv_ins: c_uint,
    cv_outs: c_uint,
    midi_ins: *const PedalboardHardwareMidiPort,
    midi_outs: *const PedalboardHardwareMidiPort,
    serial_midi_in: bool,
    serial_midi_out: bool,
    midi_merger_out: bool,
    midi_broadcaster_in: bool,
}

#[repr(C)]
struct PedalboardTimeInfo {
    available: c_uint,
    bpb: c_float,
    bpb_cc: PedalboardMidiControl,
    bpm: c_float,
    bpm_cc: PedalboardMidiControl,
    rolling: bool,
    rolling_cc: PedalboardMidiControl,
}

#[repr(C)]
struct PedalboardInfoFull {
    title: *const c_char,
    width: c_int,
    height: c_int,
    factory: bool,
    midi_separated_mode: bool,
    midi_loopback: bool,
    plugins: *const PedalboardPlugin,
    connections: *const PedalboardConnection,
    hardware: PedalboardHardware,
    time_info: PedalboardTimeInfo,
    version: c_uint,
}

#[repr(C)]
struct JackData {
    cpu_load: c_float,
    xruns: c_uint,
    rolling: bool,
    bpb: f64,
    bpm: f64,
}

// Callback types
type JackBufSizeChangedCb = extern "C" fn(c_uint);
type JackPortAppearedCb = extern "C" fn(*const c_char, bool);
type JackPortDeletedCb = extern "C" fn(*const c_char);
type TrueBypassStateChangedCb = extern "C" fn(bool, bool);

// ---- Library handle ----

static LIB: OnceLock<Option<libloading::Library>> = OnceLock::new();

fn get_lib() -> Option<&'static libloading::Library> {
    LIB.get_or_init(|| {
        let paths = [
            "libmod_utils.so",
            "/usr/lib/libmod_utils.so",
            "./lib/libmod_utils.so",
        ];
        for path in &paths {
            if let Ok(lib) = unsafe { libloading::Library::new(path) } {
                return Some(lib);
            }
        }
        tracing::warn!("libmod_utils.so not found, LV2 plugin operations will be unavailable");
        None
    })
    .as_ref()
}

// ---- FFI marshalling helpers ----

unsafe fn c_str_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        unsafe {
            CStr::from_ptr(ptr)
                .to_string_lossy()
                .into_owned()
        }
    }
}

unsafe fn c_str_array_to_vec(ptr: *const *const c_char) -> Vec<String> {
    if ptr.is_null() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut i = 0;
    loop {
        let s = unsafe { *ptr.add(i) };
        if s.is_null() {
            break;
        }
        result.push(unsafe { c_str_to_string(s) });
        i += 1;
    }
    result
}

/// Convert a valid-terminated array of PluginPortScalePoint to JSON
unsafe fn scale_points_to_json(ptr: *const PluginPortScalePoint) -> Vec<Value> { unsafe {
    if ptr.is_null() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut i = 0;
    loop {
        let sp = &*ptr.add(i);
        if !sp.valid {
            break;
        }
        result.push(json!({
            "valid": sp.valid,
            "value": sp.value,
            "label": c_str_to_string(sp.label),
        }));
        i += 1;
    }
    result
}}

/// Convert PluginPortUnits to JSON
unsafe fn units_to_json(units: &PluginPortUnits) -> Value { unsafe {
    json!({
        "label": c_str_to_string(units.label),
        "render": c_str_to_string(units.render),
        "symbol": c_str_to_string(units.symbol),
        "_custom": units._custom,
    })
}}

/// Convert PluginPortRanges to JSON
unsafe fn port_ranges_to_json(ranges: &PluginPortRanges) -> Value {
    json!({
        "minimum": ranges.min,
        "maximum": ranges.max,
        "default": ranges.def,
    })
}

/// Convert a valid-terminated array of PluginPort to JSON
unsafe fn ports_to_json(ptr: *const PluginPort) -> Vec<Value> { unsafe {
    if ptr.is_null() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut i = 0;
    loop {
        let port = &*ptr.add(i);
        if !port.valid {
            break;
        }
        result.push(json!({
            "valid": port.valid,
            "index": port.index,
            "name": c_str_to_string(port.name),
            "symbol": c_str_to_string(port.symbol),
            "ranges": port_ranges_to_json(&port.ranges),
            "units": units_to_json(&port.units),
            "comment": c_str_to_string(port.comment),
            "designation": c_str_to_string(port.designation),
            "properties": c_str_array_to_vec(port.properties),
            "rangeSteps": port.range_steps,
            "scalePoints": scale_points_to_json(port.scale_points),
            "shortName": c_str_to_string(port.short_name),
        }));
        i += 1;
    }
    result
}}

/// Convert PluginPortsI to JSON
unsafe fn ports_i_to_json(pi: &PluginPortsI) -> Value { unsafe {
    json!({
        "input": ports_to_json(pi.input),
        "output": ports_to_json(pi.output),
    })
}}

/// Convert a valid-terminated array of PluginGUIPort to JSON
unsafe fn gui_ports_to_json(ptr: *const PluginGUIPort) -> Vec<Value> { unsafe {
    if ptr.is_null() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut i = 0;
    loop {
        let gp = &*ptr.add(i);
        if !gp.valid {
            break;
        }
        result.push(json!({
            "valid": gp.valid,
            "index": gp.index,
            "name": c_str_to_string(gp.name),
            "symbol": c_str_to_string(gp.symbol),
        }));
        i += 1;
    }
    result
}}

/// Convert PluginGUI to JSON
unsafe fn gui_to_json(gui: &PluginGUI) -> Value { unsafe {
    json!({
        "resourcesDirectory": c_str_to_string(gui.resources_directory),
        "iconTemplate": c_str_to_string(gui.icon_template),
        "settingsTemplate": c_str_to_string(gui.settings_template),
        "javascript": c_str_to_string(gui.javascript),
        "stylesheet": c_str_to_string(gui.stylesheet),
        "screenshot": c_str_to_string(gui.screenshot),
        "thumbnail": c_str_to_string(gui.thumbnail),
        "discussionURL": c_str_to_string(gui.discussion_url),
        "documentation": c_str_to_string(gui.documentation),
        "brand": c_str_to_string(gui.brand),
        "label": c_str_to_string(gui.label),
        "model": c_str_to_string(gui.model),
        "panel": c_str_to_string(gui.panel),
        "color": c_str_to_string(gui.color),
        "knob": c_str_to_string(gui.knob),
        "ports": gui_ports_to_json(gui.ports),
        "monitoredOutputs": c_str_array_to_vec(gui.monitored_outputs),
    })
}}

/// Convert PluginParameterRanges union to JSON
unsafe fn param_ranges_to_json(ranges: &PluginParameterRanges) -> Value { unsafe {
    match ranges.type_ as u8 as char {
        'f' => {
            let f = &*ranges.u.f;
            json!({
                "minimum": f.min,
                "maximum": f.max,
                "default": f.def,
            })
        }
        'l' => {
            let l = &*ranges.u.l;
            json!({
                "minimum": l.min,
                "maximum": l.max,
                "default": l.def,
            })
        }
        's' => {
            json!({
                "minimum": "",
                "maximum": "",
                "default": c_str_to_string(ranges.u.s),
            })
        }
        _ => json!({}),
    }
}}

/// Convert a valid-terminated array of PluginParameter to JSON
unsafe fn parameters_to_json(ptr: *const PluginParameter) -> Vec<Value> { unsafe {
    if ptr.is_null() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut i = 0;
    loop {
        let param = &*ptr.add(i);
        if !param.valid {
            break;
        }
        result.push(json!({
            "valid": param.valid,
            "readable": param.readable,
            "writable": param.writable,
            "uri": c_str_to_string(param.uri),
            "label": c_str_to_string(param.label),
            "type": c_str_to_string(param.type_),
            "ranges": param_ranges_to_json(&param.ranges),
            "units": units_to_json(&param.units),
            "comment": c_str_to_string(param.comment),
            "shortName": c_str_to_string(param.short_name),
            "fileTypes": c_str_array_to_vec(param.file_types),
            "supportedExtensions": c_str_array_to_vec(param.supported_extensions),
        }));
        i += 1;
    }
    result
}}

/// Convert a valid-terminated array of PluginPreset to JSON
unsafe fn presets_to_json(ptr: *const PluginPreset) -> Vec<Value> { unsafe {
    if ptr.is_null() {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut i = 0;
    loop {
        let preset = &*ptr.add(i);
        if !preset.valid {
            break;
        }
        result.push(json!({
            "valid": preset.valid,
            "uri": c_str_to_string(preset.uri),
            "label": c_str_to_string(preset.label),
            "path": c_str_to_string(preset.path),
        }));
        i += 1;
    }
    result
}}

// ---- Public API ----

/// Initialize the LV2 world
pub fn init() {
    if let Some(lib) = get_lib() {
        unsafe {
            if let Ok(func) = lib.get::<unsafe extern "C" fn()>(b"init") {
                func();
            }
        }
    }
}

/// Cleanup the LV2 world
pub fn cleanup() {
    if let Some(lib) = get_lib() {
        unsafe {
            if let Ok(func) = lib.get::<unsafe extern "C" fn()>(b"cleanup") {
                func();
            }
        }
    }
}

/// Check if a bundle is loaded
pub fn is_bundle_loaded(bundlepath: &str) -> bool {
    let Some(lib) = get_lib() else { return false };
    let c_path = CString::new(bundlepath).unwrap_or_default();
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(*const c_char) -> bool>(b"is_bundle_loaded") {
            func(c_path.as_ptr())
        } else {
            false
        }
    }
}

/// Add a bundle to the LV2 world, returns list of added plugin URIs
pub fn add_bundle_to_lilv_world(bundlepath: &str) -> Vec<String> {
    let Some(lib) = get_lib() else { return Vec::new() };
    let c_path = CString::new(bundlepath).unwrap_or_default();
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(*const c_char) -> *const *const c_char>(b"add_bundle_to_lilv_world") {
            c_str_array_to_vec(func(c_path.as_ptr()))
        } else {
            Vec::new()
        }
    }
}

/// Remove a bundle from the LV2 world, returns list of removed plugin URIs
pub fn remove_bundle_from_lilv_world(bundlepath: &str, resource: Option<&str>) -> Vec<String> {
    let Some(lib) = get_lib() else { return Vec::new() };
    let c_path = CString::new(bundlepath).unwrap_or_default();
    let c_res = resource.and_then(|r| CString::new(r).ok());
    let res_ptr = c_res.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(*const c_char, *const c_char) -> *const *const c_char>(b"remove_bundle_from_lilv_world") {
            c_str_array_to_vec(func(c_path.as_ptr(), res_ptr))
        } else {
            Vec::new()
        }
    }
}

/// Rescan presets for a given plugin URI (after saving/deleting a preset).
pub fn rescan_plugin_presets(uri: &str) {
    let Some(lib) = get_lib() else { return };
    let c_uri = CString::new(uri).unwrap_or_default();
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(*const c_char)>(b"rescan_plugin_presets") {
            func(c_uri.as_ptr());
        }
    }
}

/// C struct for StatePortValue
#[repr(C)]
struct StatePortValue {
    valid: bool,
    symbol: *const c_char,
    value: c_float,
}

/// Parse mod-host "preset_show" state string into a map of symbol -> value.
pub fn get_state_port_values(state: &str) -> std::collections::HashMap<String, f64> {
    let Some(lib) = get_lib() else { return std::collections::HashMap::new() };
    let c_state = CString::new(state).unwrap_or_default();
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(*const c_char) -> *const StatePortValue>(b"get_state_port_values") {
            let ptr = func(c_state.as_ptr());
            if ptr.is_null() {
                return std::collections::HashMap::new();
            }
            let mut result = std::collections::HashMap::new();
            let mut i = 0;
            loop {
                let spv = &*ptr.add(i);
                if !spv.valid {
                    break;
                }
                let symbol = c_str_to_string(spv.symbol);
                result.insert(symbol, spv.value as f64);
                i += 1;
            }
            result
        } else {
            std::collections::HashMap::new()
        }
    }
}

/// Get list of all available plugin URIs
pub fn get_plugin_list() -> Vec<String> {
    let Some(lib) = get_lib() else { return Vec::new() };
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn() -> *const *const c_char>(b"get_plugin_list") {
            c_str_array_to_vec(func())
        } else {
            Vec::new()
        }
    }
}

/// Get all available plugins as JSON-like mini info.
pub fn get_all_plugins() -> Vec<Value> {
    let Some(lib) = get_lib() else { return Vec::new() };
    unsafe {
        let func = match lib.get::<unsafe extern "C" fn() -> *const *const PluginInfoMini>(b"get_all_plugins") {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let ptr = func();
        if ptr.is_null() {
            return Vec::new();
        }
        let mut result = Vec::new();
        let mut i = 0;
        loop {
            let item = *ptr.add(i);
            if item.is_null() {
                break;
            }
            let info = &*item;
            let categories = c_str_array_to_vec(info.category);
            result.push(json!({
                "uri": c_str_to_string(info.uri),
                "name": c_str_to_string(info.name),
                "brand": c_str_to_string(info.brand),
                "label": c_str_to_string(info.label),
                "comment": c_str_to_string(info.comment),
                "buildEnvironment": c_str_to_string(info.build_environment),
                "category": categories,
                "microVersion": info.micro_version,
                "minorVersion": info.minor_version,
                "release": info.release,
                "builder": info.builder,
                "licensed": info.licensed,
                "iotype": info.iotype,
                "gui": {
                    "resourcesDirectory": c_str_to_string(info.gui.resources_directory),
                    "screenshot": c_str_to_string(info.gui.screenshot),
                    "thumbnail": c_str_to_string(info.gui.thumbnail),
                },
            }));
            i += 1;
        }
        result
    }
}

/// Get plugin GUI (full) info for a specific URI
pub fn get_plugin_gui(uri: &str) -> Option<Value> {
    let Some(lib) = get_lib() else { return None };
    let c_uri = CString::new(uri).ok()?;
    unsafe {
        let func = lib.get::<unsafe extern "C" fn(*const c_char) -> *const PluginGUI>(b"get_plugin_gui").ok()?;
        let ptr = func(c_uri.as_ptr());
        if ptr.is_null() {
            return None;
        }
        let gui = &*ptr;
        Some(gui_to_json(gui))
    }
}

/// Get plugin GUI mini info for a specific URI
pub fn get_plugin_gui_mini(uri: &str) -> Option<Value> {
    let Some(lib) = get_lib() else { return None };
    let c_uri = CString::new(uri).ok()?;
    unsafe {
        let func = lib.get::<unsafe extern "C" fn(*const c_char) -> *const PluginGUIMini>(b"get_plugin_gui_mini").ok()?;
        let ptr = func(c_uri.as_ptr());
        if ptr.is_null() {
            return None;
        }
        let gui = &*ptr;
        Some(json!({
            "resourcesDirectory": c_str_to_string(gui.resources_directory),
            "screenshot": c_str_to_string(gui.screenshot),
            "thumbnail": c_str_to_string(gui.thumbnail),
        }))
    }
}

/// Get plugin info for a specific URI (full info with ports, parameters, presets, etc.)
pub fn get_plugin_info(uri: &str) -> Option<Value> {
    let Some(lib) = get_lib() else { return None };
    let c_uri = CString::new(uri).ok()?;
    unsafe {
        let func = lib.get::<unsafe extern "C" fn(*const c_char) -> *const PluginInfo>(b"get_plugin_info").ok()?;
        let ptr = func(c_uri.as_ptr());
        if ptr.is_null() {
            return None;
        }
        let info = &*ptr;
        Some(json!({
            "uri": c_str_to_string(info.uri),
            "name": c_str_to_string(info.name),
            "binary": c_str_to_string(info.binary),
            "brand": c_str_to_string(info.brand),
            "label": c_str_to_string(info.label),
            "license": c_str_to_string(info.license),
            "comment": c_str_to_string(info.comment),
            "buildEnvironment": c_str_to_string(info.build_environment),
            "category": c_str_array_to_vec(info.category),
            "microVersion": info.micro_version,
            "minorVersion": info.minor_version,
            "release": info.release,
            "builder": info.builder,
            "licensed": info.licensed,
            "iotype": info.iotype,
            "hasExternalUI": info.has_external_ui,
            "version": c_str_to_string(info.version),
            "stability": c_str_to_string(info.stability),
            "author": {
                "name": c_str_to_string(info.author.name),
                "homepage": c_str_to_string(info.author.homepage),
                "email": c_str_to_string(info.author.email),
            },
            "bundles": c_str_array_to_vec(info.bundles),
            "gui": gui_to_json(&info.gui),
            "ports": {
                "audio": ports_i_to_json(&info.ports.audio),
                "control": ports_i_to_json(&info.ports.control),
                "cv": ports_i_to_json(&info.ports.cv),
                "midi": ports_i_to_json(&info.ports.midi),
            },
            "parameters": parameters_to_json(info.parameters),
            "presets": presets_to_json(info.presets),
        }))
    }
}

/// Get non-cached plugin info (licensed status and presets)
pub fn get_non_cached_plugin_info(uri: &str) -> Option<Value> {
    let Some(lib) = get_lib() else { return None };
    let c_uri = CString::new(uri).ok()?;
    unsafe {
        let func = lib.get::<unsafe extern "C" fn(*const c_char) -> *const NonCachedPluginInfo>(b"get_non_cached_plugin_info").ok()?;
        let ptr = func(c_uri.as_ptr());
        if ptr.is_null() {
            return None;
        }
        let info = &*ptr;
        Some(json!({
            "licensed": info.licensed,
            "presets": presets_to_json(info.presets),
        }))
    }
}

/// Get all pedalboards (mini info)
pub fn get_all_pedalboards(ptype: i32) -> Vec<Value> {
    let Some(lib) = get_lib() else { return Vec::new() };
    let c_ptype = ptype as c_int;
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(c_int) -> *const *const PedalboardInfoMini>(b"get_all_pedalboards") {
            let ptr = func(c_ptype);
            if ptr.is_null() {
                return Vec::new();
            }
            let mut result = Vec::new();
            let mut i = 0;
            loop {
                let item = *ptr.add(i);
                if item.is_null() {
                    break;
                }
                let info = &*item;
                result.push(json!({
                    "broken": info.broken,
                    "factory": info.factory,
                    "hasTrialPlugins": info.has_trial_plugins,
                    "uri": c_str_to_string(info.uri),
                    "bundle": c_str_to_string(info.bundle),
                    "title": c_str_to_string(info.title),
                    "version": info.version,
                }));
                i += 1;
            }
            result
        } else {
            Vec::new()
        }
    }
}

/// Get broken pedalboard bundle paths
pub fn get_broken_pedalboards() -> Vec<String> {
    let Some(lib) = get_lib() else { return Vec::new() };
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn() -> *const *const c_char>(b"get_broken_pedalboards") {
            c_str_array_to_vec(func())
        } else {
            Vec::new()
        }
    }
}

/// Reset cached pedalboard data
pub fn reset_get_all_pedalboards_cache(ptype: i32) {
    if let Some(lib) = get_lib() {
        unsafe {
            if let Ok(func) = lib.get::<unsafe extern "C" fn(c_int)>(b"reset_get_all_pedalboards_cache") {
                func(ptype as c_int);
            }
        }
    }
}

/// Get full pedalboard info (parsed from TTL)
pub fn get_pedalboard_info(bundle: &str) -> Option<Value> {
    let Some(lib) = get_lib() else { return None };
    let c_bundle = CString::new(bundle).ok()?;
    unsafe {
        let func = lib.get::<unsafe extern "C" fn(*const c_char) -> *const PedalboardInfoFull>(b"get_pedalboard_info").ok()?;
        let ptr = func(c_bundle.as_ptr());
        if ptr.is_null() {
            return None;
        }
        let info = &*ptr;

        // Parse plugins
        let mut plugins = Vec::new();
        if !info.plugins.is_null() {
            let mut i = 0;
            loop {
                let p = &*info.plugins.add(i);
                if !p.valid {
                    break;
                }
                let mut ports = Vec::new();
                if !p.ports.is_null() {
                    let mut j = 0;
                    loop {
                        let port = &*p.ports.add(j);
                        if !port.valid {
                            break;
                        }
                        ports.push(json!({
                            "symbol": c_str_to_string(port.symbol),
                            "value": port.value,
                            "midiCC": {
                                "channel": port.midi_cc.channel,
                                "control": port.midi_cc.control,
                                "hasRanges": port.midi_cc.has_ranges,
                                "minimum": port.midi_cc.minimum,
                                "maximum": port.midi_cc.maximum,
                            }
                        }));
                        j += 1;
                    }
                }
                plugins.push(json!({
                    "instance": c_str_to_string(p.instance),
                    "uri": c_str_to_string(p.uri),
                    "bypassed": p.bypassed,
                    "instanceNumber": p.instance_number,
                    "x": p.x,
                    "y": p.y,
                    "preset": c_str_to_string(p.preset),
                    "bypassCC": {
                        "channel": p.bypass_cc.channel,
                        "control": p.bypass_cc.control,
                    },
                    "ports": ports,
                }));
                i += 1;
            }
        }

        // Parse connections
        let mut connections = Vec::new();
        if !info.connections.is_null() {
            let mut i = 0;
            loop {
                let c = &*info.connections.add(i);
                if !c.valid {
                    break;
                }
                connections.push(json!({
                    "source": c_str_to_string(c.source),
                    "target": c_str_to_string(c.target),
                }));
                i += 1;
            }
        }

        // Parse hardware MIDI ports
        let mut midi_ins = Vec::new();
        if !info.hardware.midi_ins.is_null() {
            let mut i = 0;
            loop {
                let p = &*info.hardware.midi_ins.add(i);
                if !p.valid { break; }
                midi_ins.push(json!({
                    "symbol": c_str_to_string(p.symbol),
                    "name": c_str_to_string(p.name),
                }));
                i += 1;
            }
        }
        let mut midi_outs = Vec::new();
        if !info.hardware.midi_outs.is_null() {
            let mut i = 0;
            loop {
                let p = &*info.hardware.midi_outs.add(i);
                if !p.valid { break; }
                midi_outs.push(json!({
                    "symbol": c_str_to_string(p.symbol),
                    "name": c_str_to_string(p.name),
                }));
                i += 1;
            }
        }

        Some(json!({
            "title": c_str_to_string(info.title),
            "width": info.width,
            "height": info.height,
            "factory": info.factory,
            "midi_separated_mode": info.midi_separated_mode,
            "midi_loopback": info.midi_loopback,
            "version": info.version,
            "plugins": plugins,
            "connections": connections,
            "hardware": {
                "audio_ins": info.hardware.audio_ins,
                "audio_outs": info.hardware.audio_outs,
                "cv_ins": info.hardware.cv_ins,
                "cv_outs": info.hardware.cv_outs,
                "midi_ins": midi_ins,
                "midi_outs": midi_outs,
                "serial_midi_in": info.hardware.serial_midi_in,
                "serial_midi_out": info.hardware.serial_midi_out,
                "midi_merger_out": info.hardware.midi_merger_out,
                "midi_broadcaster_in": info.hardware.midi_broadcaster_in,
            },
            "timeInfo": {
                "available": info.time_info.available,
                "bpb": info.time_info.bpb,
                "bpbCC": {
                    "channel": info.time_info.bpb_cc.channel,
                    "control": info.time_info.bpb_cc.control,
                    "hasRanges": info.time_info.bpb_cc.has_ranges,
                    "minimum": info.time_info.bpb_cc.minimum,
                    "maximum": info.time_info.bpb_cc.maximum,
                },
                "bpm": info.time_info.bpm,
                "bpmCC": {
                    "channel": info.time_info.bpm_cc.channel,
                    "control": info.time_info.bpm_cc.control,
                    "hasRanges": info.time_info.bpm_cc.has_ranges,
                    "minimum": info.time_info.bpm_cc.minimum,
                    "maximum": info.time_info.bpm_cc.maximum,
                },
                "rolling": info.time_info.rolling,
                "rollingCC": {
                    "channel": info.time_info.rolling_cc.channel,
                    "control": info.time_info.rolling_cc.control,
                    "hasRanges": info.time_info.rolling_cc.has_ranges,
                    "minimum": info.time_info.rolling_cc.minimum,
                    "maximum": info.time_info.rolling_cc.maximum,
                },
            },
        }))
    }
}

/// List plugins in a bundle
pub fn list_plugins_in_bundle(bundle: &str) -> Vec<String> {
    let Some(lib) = get_lib() else { return Vec::new() };
    let c_bundle = CString::new(bundle).unwrap_or_default();
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(*const c_char) -> *const *const c_char>(b"list_plugins_in_bundle") {
            c_str_array_to_vec(func(c_bundle.as_ptr()))
        } else {
            Vec::new()
        }
    }
}

// ---- JACK functions ----

/// Initialize JACK client
pub fn init_jack() -> bool {
    let Some(lib) = get_lib() else { return false };
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn() -> bool>(b"init_jack") {
            func()
        } else {
            false
        }
    }
}

/// Close JACK client
pub fn close_jack() {
    if let Some(lib) = get_lib() {
        unsafe {
            if let Ok(func) = lib.get::<unsafe extern "C" fn()>(b"close_jack") {
                func();
            }
        }
    }
}

/// Get JACK data (CPU load, xruns, transport)
pub fn get_jack_data(with_transport: bool) -> Option<Value> {
    let Some(lib) = get_lib() else { return None };
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(bool) -> *const JackData>(b"get_jack_data") {
            let ptr = func(with_transport);
            if ptr.is_null() {
                return None;
            }
            let data = &*ptr;
            Some(json!({
                "cpuLoad": data.cpu_load,
                "xruns": data.xruns,
                "rolling": data.rolling,
                "bpb": data.bpb,
                "bpm": data.bpm,
            }))
        } else {
            None
        }
    }
}

/// Get JACK buffer size
pub fn get_jack_buffer_size() -> u32 {
    let Some(lib) = get_lib() else { return 0 };
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn() -> c_uint>(b"get_jack_buffer_size") {
            func()
        } else {
            0
        }
    }
}

/// Set JACK buffer size
pub fn set_jack_buffer_size(size: u32) -> u32 {
    let Some(lib) = get_lib() else { return 0 };
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(c_uint) -> c_uint>(b"set_jack_buffer_size") {
            func(size)
        } else {
            0
        }
    }
}

/// Get JACK sample rate
pub fn get_jack_sample_rate() -> f32 {
    let Some(lib) = get_lib() else { return 0.0 };
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn() -> c_float>(b"get_jack_sample_rate") {
            func()
        } else {
            0.0
        }
    }
}

/// Get JACK hardware ports
pub fn get_jack_hardware_ports(is_audio: bool, is_output: bool) -> Vec<String> {
    let Some(lib) = get_lib() else { return Vec::new() };
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(bool, bool) -> *const *const c_char>(b"get_jack_hardware_ports") {
            c_str_array_to_vec(func(is_audio, is_output))
        } else {
            Vec::new()
        }
    }
}

/// Get the alias for a JACK port (e.g. the ALSA device name)
pub fn get_jack_port_alias(port_name: &str) -> Option<String> {
    let lib = get_lib()?;
    let c_name = CString::new(port_name).ok()?;
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(*const c_char) -> *const c_char>(b"get_jack_port_alias") {
            let ptr = func(c_name.as_ptr());
            if ptr.is_null() {
                None
            } else {
                Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
            }
        } else {
            None
        }
    }
}

/// Connect two JACK ports
pub fn connect_jack_ports(port1: &str, port2: &str) -> bool {
    let Some(lib) = get_lib() else { return false };
    let c1 = CString::new(port1).unwrap_or_default();
    let c2 = CString::new(port2).unwrap_or_default();
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(*const c_char, *const c_char) -> bool>(b"connect_jack_ports") {
            func(c1.as_ptr(), c2.as_ptr())
        } else {
            false
        }
    }
}

/// Disconnect two JACK ports
pub fn disconnect_jack_ports(port1: &str, port2: &str) -> bool {
    let Some(lib) = get_lib() else { return false };
    let c1 = CString::new(port1).unwrap_or_default();
    let c2 = CString::new(port2).unwrap_or_default();
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(*const c_char, *const c_char) -> bool>(b"disconnect_jack_ports") {
            func(c1.as_ptr(), c2.as_ptr())
        } else {
            false
        }
    }
}

/// Reset JACK xruns counter
pub fn reset_xruns() {
    if let Some(lib) = get_lib() {
        unsafe {
            if let Ok(func) = lib.get::<unsafe extern "C" fn()>(b"reset_xruns") {
                func();
            }
        }
    }
}

/// Get true bypass value
pub fn get_truebypass_value(right: bool) -> bool {
    let Some(lib) = get_lib() else { return false };
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(bool) -> bool>(b"get_truebypass_value") {
            func(right)
        } else {
            false
        }
    }
}

/// Set true bypass value
pub fn set_truebypass_value(right: bool, bypassed: bool) -> bool {
    let Some(lib) = get_lib() else { return false };
    unsafe {
        if let Ok(func) = lib.get::<unsafe extern "C" fn(bool, bool) -> bool>(b"set_truebypass_value") {
            func(right, bypassed)
        } else {
            false
        }
    }
}

// Pedalboard info type constants
pub const PEDALBOARD_INFO_USER_ONLY: i32 = 0;
pub const PEDALBOARD_INFO_FACTORY_ONLY: i32 = 1;
pub const PEDALBOARD_INFO_BOTH: i32 = 2;

// Plugin license type constants
pub const PLUGIN_LICENSE_NON_COMMERCIAL: i32 = 0;
pub const PLUGIN_LICENSE_TRIAL: i32 = -1;
pub const PLUGIN_LICENSE_PAID: i32 = 1;

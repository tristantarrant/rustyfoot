// MOD protocol command definitions, ported from mod/mod_protocol.py

use std::collections::HashMap;

/// Argument types for protocol commands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArgType {
    Int,
    Float,
    Str,
}

/// Build the CMD_ARGS registry: maps model name → { command → [arg types] }.
pub fn cmd_args() -> HashMap<&'static str, HashMap<&'static str, Vec<ArgType>>> {
    use ArgType::*;

    let mut models: HashMap<&str, HashMap<&str, Vec<ArgType>>> = HashMap::new();

    let mut all = HashMap::new();
    all.insert("pi", vec![]);
    all.insert("say", vec![Str]);
    all.insert("l", vec![Int, Int, Int, Int]);
    all.insert("displ_bright", vec![Int]);
    all.insert("glcd_text", vec![Int, Int, Int, Str]);
    all.insert("glcd_dialog", vec![Str]);
    all.insert("glcd_draw", vec![Int, Int, Int, Str]);
    all.insert("uc", vec![]);
    all.insert("ud", vec![]);
    all.insert("a", vec![Int, Str, Int, Str, Float, Float, Float, Int, Int]);
    all.insert("d", vec![Int]);
    all.insert("g", vec![Int]);
    all.insert("s", vec![Int, Float]);
    all.insert("cps", vec![Str, Str, Float]);
    all.insert("fps", vec![Str, Str, Str]);
    all.insert("fpc", vec![Str, Str, Str]);
    all.insert("ncp", vec![Int, Int, Int]);
    all.insert("is", vec![Int, Int, Int, Int, Int, Int, Str, Str]);
    all.insert("bchng", vec![Int]);
    all.insert("b", vec![Int, Int]);
    all.insert("bn", vec![Str]);
    all.insert("bd", vec![Int]);
    all.insert("ba", vec![Int, Int, Str]);
    all.insert("br", vec![Int, Int, Int]);
    all.insert("p", vec![Int, Int, Int]);
    all.insert("pn", vec![Str]);
    all.insert("pchng", vec![Int]);
    all.insert("pb", vec![Int, Str]);
    all.insert("pr", vec![]);
    all.insert("ps", vec![]);
    all.insert("psa", vec![Str]);
    all.insert("pcl", vec![]);
    all.insert("pbd", vec![Int, Int]);
    all.insert("sr", vec![Int, Int]);
    all.insert("ssg", vec![Int, Int]);
    all.insert("sn", vec![Int, Str]);
    all.insert("ssl", vec![Int]);
    all.insert("sss", vec![]);
    all.insert("ssa", vec![Str]);
    all.insert("ssd", vec![Int]);
    all.insert("ts", vec![Float, Str, Int]);
    all.insert("tn", vec![]);
    all.insert("tf", vec![]);
    all.insert("ti", vec![Int]);
    all.insert("tr", vec![Int]);
    all.insert("restore", vec![]);
    all.insert("screenshot", vec![Int, Str]);
    all.insert("r", vec![Int]);
    all.insert("c", vec![Int, Int]);
    all.insert("upr", vec![Int]);
    all.insert("ups", vec![Int]);
    all.insert("lp", vec![Int]);
    all.insert("reset_eeprom", vec![]);
    all.insert("enc_clicked", vec![Int]);
    all.insert("enc_left", vec![Int]);
    all.insert("enc_right", vec![Int]);
    all.insert("button_clicked", vec![Int]);
    all.insert("pot_call_check", vec![Int]);
    all.insert("pot_call_ok", vec![Int]);
    all.insert("control_skip_enable", vec![]);
    all.insert("control_bad_skip", vec![]);
    all.insert("save_pot_cal", vec![Int, Int]);
    all.insert("sys_gio", vec![Int, Int, Float]);
    all.insert("sys_ghp", vec![Float]);
    all.insert("sys_cvi", vec![Int]);
    all.insert("sys_exp", vec![Int]);
    all.insert("sys_cvo", vec![Int]);
    all.insert("sys_ngc", vec![Int]);
    all.insert("sys_ngt", vec![Int]);
    all.insert("sys_ngd", vec![Int]);
    all.insert("sys_cmm", vec![Int]);
    all.insert("sys_cmr", vec![Int]);
    all.insert("sys_pbg", vec![Int]);
    all.insert("sys_ams", vec![]);
    all.insert("sys_bts", vec![]);
    all.insert("sys_btd", vec![]);
    all.insert("sys_ctl", vec![Str]);
    all.insert("sys_ver", vec![Str]);
    all.insert("sys_ser", vec![]);
    all.insert("sys_usb", vec![Int]);
    all.insert("sys_mnr", vec![Int]);
    all.insert("sys_rbt", vec![]);
    all.insert("sys_lbl", vec![Int, Int, Int, Int]);
    all.insert("sys_lbh", vec![Int, Int, Int]);
    all.insert("sys_nam", vec![Int, Str]);
    all.insert("sys_uni", vec![Int, Str]);
    all.insert("sys_val", vec![Int, Str]);
    all.insert("sys_ind", vec![Int, Float]);
    all.insert("sys_pop", vec![Int, Int, Str, Str]);
    all.insert("sys_pch", vec![Int]);
    all.insert("sys_spc", vec![Int]);
    models.insert("ALL", all);

    let mut duo = HashMap::new();
    duo.insert("boot", vec![Int, Int, Str]);
    duo.insert("fn", vec![Int]);
    duo.insert("bc", vec![Int, Int]);
    duo.insert("n", vec![Int]);
    duo.insert("si", vec![Int, Int, Int]);
    duo.insert("ncp", vec![Int, Int]); // Backwards compat for Duo and Duo X
    models.insert("DUO", duo);

    let mut duox = HashMap::new();
    duox.insert("boot", vec![Int, Int, Str]);
    duox.insert("ss", vec![Int]);
    duox.insert("sl", vec![Int]);
    duox.insert("sc", vec![]);
    duox.insert("pa", vec![Int, Int, Int, Int, Int, Int]);
    duox.insert("s_contrast", vec![Int, Int]);
    duox.insert("exp_overcurrent", vec![]);
    models.insert("DUOX", duox);

    let mut dwarf = HashMap::new();
    dwarf.insert("cs", vec![Int, Int]);
    dwarf.insert("pa", vec![Int, Int, Int, Int, Int, Int, Int, Int]);
    models.insert("DWARF", dwarf);

    models
}

// Command string constants
pub const CMD_PING: &str = "pi";
pub const CMD_SAY: &str = "say";
pub const CMD_LED: &str = "l";
pub const CMD_DISP_BRIGHTNESS: &str = "displ_bright";
pub const CMD_GLCD_TEXT: &str = "glcd_text";
pub const CMD_GLCD_DIALOG: &str = "glcd_dialog";
pub const CMD_GLCD_DRAW: &str = "glcd_draw";
pub const CMD_GUI_CONNECTED: &str = "uc";
pub const CMD_GUI_DISCONNECTED: &str = "ud";
pub const CMD_CONTROL_ADD: &str = "a";
pub const CMD_CONTROL_REMOVE: &str = "d";
pub const CMD_CONTROL_GET: &str = "g";
pub const CMD_CONTROL_SET: &str = "s";
pub const CMD_CONTROL_PARAM_SET: &str = "cps";
pub const CMD_FILE_PARAM_SET: &str = "fps";
pub const CMD_FILE_PARAM_CURRENT: &str = "fpc";
pub const CMD_CONTROL_PAGE: &str = "ncp";
pub const CMD_INITIAL_STATE: &str = "is";
pub const CMD_BANK_CHANGE: &str = "bchng";
pub const CMD_BANKS: &str = "b";
pub const CMD_BANK_NEW: &str = "bn";
pub const CMD_BANK_DELETE: &str = "bd";
pub const CMD_ADD_PBS_TO_BANK: &str = "ba";
pub const CMD_REORDER_PBS_IN_BANK: &str = "br";
pub const CMD_PEDALBOARDS: &str = "p";
pub const CMD_PEDALBOARD_NAME_SET: &str = "pn";
pub const CMD_PEDALBOARD_CHANGE: &str = "pchng";
pub const CMD_PEDALBOARD_LOAD: &str = "pb";
pub const CMD_PEDALBOARD_RESET: &str = "pr";
pub const CMD_PEDALBOARD_SAVE: &str = "ps";
pub const CMD_PEDALBOARD_SAVE_AS: &str = "psa";
pub const CMD_PEDALBOARD_CLEAR: &str = "pcl";
pub const CMD_PEDALBOARD_DELETE: &str = "pbd";
pub const CMD_PEDALBOARD_RELOAD_LIST: &str = "prl";
pub const CMD_REORDER_SSS_IN_PB: &str = "sr";
pub const CMD_SNAPSHOTS: &str = "ssg";
pub const CMD_SNAPSHOT_NAME_SET: &str = "sn";
pub const CMD_SNAPSHOTS_LOAD: &str = "ssl";
pub const CMD_SNAPSHOTS_SAVE: &str = "sss";
pub const CMD_SNAPSHOT_SAVE_AS: &str = "ssa";
pub const CMD_SNAPSHOT_DELETE: &str = "ssd";
pub const CMD_TUNER: &str = "ts";
pub const CMD_TUNER_ON: &str = "tn";
pub const CMD_TUNER_OFF: &str = "tf";
pub const CMD_TUNER_INPUT: &str = "ti";
pub const CMD_TUNER_REF_FREQ: &str = "tr";
pub const CMD_RESTORE: &str = "restore";
pub const CMD_SCREENSHOT: &str = "screenshot";
pub const CMD_RESPONSE: &str = "r";
pub const CMD_MENU_ITEM_CHANGE: &str = "c";
pub const CMD_PROFILE_LOAD: &str = "upr";
pub const CMD_PROFILE_STORE: &str = "ups";
pub const CMD_NEXT_PAGE: &str = "lp";
pub const CMD_RESET_EEPROM: &str = "reset_eeprom";
pub const CMD_SELFTEST_ENCODER_CLICKED: &str = "enc_clicked";
pub const CMD_SELFTEST_ENCODER_LEFT: &str = "enc_left";
pub const CMD_SELFTEST_ENCODER_RIGHT: &str = "enc_right";
pub const CMD_SELFTEST_BUTTON_CLICKED: &str = "button_clicked";
pub const CMD_SELFTEST_CHECK_CALIBRATION: &str = "pot_call_check";
pub const CMD_SELFTEST_CALLIBRATION_OK: &str = "pot_call_ok";
pub const CMD_SELFTEST_SKIP_CONTROL_ENABLE: &str = "control_skip_enable";
pub const CMD_SELFTEST_SKIP_CONTROL: &str = "control_bad_skip";
pub const CMD_SELFTEST_SAVE_POT_CALIBRATION: &str = "save_pot_cal";
pub const CMD_SYS_GAIN: &str = "sys_gio";
pub const CMD_SYS_HP_GAIN: &str = "sys_ghp";
pub const CMD_SYS_CVI_MODE: &str = "sys_cvi";
pub const CMD_SYS_EXP_MODE: &str = "sys_exp";
pub const CMD_SYS_CVO_MODE: &str = "sys_cvo";
pub const CMD_SYS_NG_CHANNEL: &str = "sys_ngc";
pub const CMD_SYS_NG_THRESHOLD: &str = "sys_ngt";
pub const CMD_SYS_NG_DECAY: &str = "sys_ngd";
pub const CMD_SYS_COMP_MODE: &str = "sys_cmm";
pub const CMD_SYS_COMP_RELEASE: &str = "sys_cmr";
pub const CMD_SYS_COMP_PEDALBOARD_GAIN: &str = "sys_pbg";
pub const CMD_SYS_AMIXER_SAVE: &str = "sys_ams";
pub const CMD_SYS_BT_STATUS: &str = "sys_bts";
pub const CMD_SYS_BT_DISCOVERY: &str = "sys_btd";
pub const CMD_SYS_SYSTEMCTL: &str = "sys_ctl";
pub const CMD_SYS_VERSION: &str = "sys_ver";
pub const CMD_SYS_SERIAL: &str = "sys_ser";
pub const CMD_SYS_USB_MODE: &str = "sys_usb";
pub const CMD_SYS_NOISE_REMOVAL: &str = "sys_mnr";
pub const CMD_SYS_REBOOT: &str = "sys_rbt";
pub const CMD_SYS_CHANGE_LED_BLINK: &str = "sys_lbl";
pub const CMD_SYS_CHANGE_LED_BRIGHTNESS: &str = "sys_lbh";
pub const CMD_SYS_CHANGE_NAME: &str = "sys_nam";
pub const CMD_SYS_CHANGE_UNIT: &str = "sys_uni";
pub const CMD_SYS_CHANGE_VALUE: &str = "sys_val";
pub const CMD_SYS_CHANGE_WIDGET_INDICATOR: &str = "sys_ind";
pub const CMD_SYS_LAUNCH_POPUP: &str = "sys_pop";
pub const CMD_SYS_PAGE_CHANGE: &str = "sys_pch";
pub const CMD_SYS_SUBPAGE_CHANGE: &str = "sys_spc";
pub const CMD_DUO_BOOT: &str = "boot";
pub const CMD_DUO_FOOT_NAVIG: &str = "fn";
pub const CMD_DUO_BANK_CONFIG: &str = "bc";
pub const CMD_DUO_CONTROL_NEXT: &str = "n";
pub const CMD_DUO_CONTROL_INDEX_SET: &str = "si";
pub const CMD_DUOX_BOOT: &str = "boot";
pub const CMD_DUOX_SNAPSHOT_SAVE: &str = "ss";
pub const CMD_DUOX_SNAPSHOT_LOAD: &str = "sl";
pub const CMD_DUOX_SNAPSHOT_CLEAR: &str = "sc";
pub const CMD_DUOX_PAGES_AVAILABLE: &str = "pa";
pub const CMD_DUOX_SET_CONTRAST: &str = "s_contrast";
pub const CMD_DUOX_EXP_OVERCURRENT: &str = "exp_overcurrent";
pub const CMD_DWARF_CONTROL_SUBPAGE: &str = "cs";
pub const CMD_DWARF_PAGES_AVAILABLE: &str = "pa";

// Bank function constants
pub const BANK_FUNC_NONE: i32 = 0;
pub const BANK_FUNC_TRUE_BYPASS: i32 = 1;
pub const BANK_FUNC_PEDALBOARD_NEXT: i32 = 2;
pub const BANK_FUNC_PEDALBOARD_PREV: i32 = 3;
pub const BANK_FUNC_COUNT: i32 = 4;

// Navigation flags
pub const FLAG_NAVIGATION_FACTORY: u32 = 0x1;
pub const FLAG_NAVIGATION_READ_ONLY: u32 = 0x2;
pub const FLAG_NAVIGATION_DIVIDER: u32 = 0x4;
pub const FLAG_NAVIGATION_TRIAL_PLUGINS: u32 = 0x8;

// Control flags
pub const FLAG_CONTROL_BYPASS: u32 = 0x001;
pub const FLAG_CONTROL_TAP_TEMPO: u32 = 0x002;
pub const FLAG_CONTROL_ENUMERATION: u32 = 0x004;
pub const FLAG_CONTROL_SCALE_POINTS: u32 = 0x008;
pub const FLAG_CONTROL_TRIGGER: u32 = 0x010;
pub const FLAG_CONTROL_TOGGLED: u32 = 0x020;
pub const FLAG_CONTROL_LOGARITHMIC: u32 = 0x040;
pub const FLAG_CONTROL_INTEGER: u32 = 0x080;
pub const FLAG_CONTROL_REVERSE: u32 = 0x100;
pub const FLAG_CONTROL_MOMENTARY: u32 = 0x200;

// Pagination flags
pub const FLAG_PAGINATION_PAGE_UP: u32 = 0x1;
pub const FLAG_PAGINATION_WRAP_AROUND: u32 = 0x2;
pub const FLAG_PAGINATION_INITIAL_REQ: u32 = 0x4;
pub const FLAG_PAGINATION_ALT_LED_COLOR: u32 = 0x8;

// Scale-point flags
pub const FLAG_SCALEPOINT_PAGINATED: u32 = 0x1;
pub const FLAG_SCALEPOINT_WRAP_AROUND: u32 = 0x2;
pub const FLAG_SCALEPOINT_END_PAGE: u32 = 0x4;
pub const FLAG_SCALEPOINT_ALT_LED_COLOR: u32 = 0x8;

// Menu IDs
pub const MENU_ID_SL_IN: i32 = 0;
pub const MENU_ID_SL_OUT: i32 = 1;
pub const MENU_ID_TUNER_MUTE: i32 = 2;
pub const MENU_ID_QUICK_BYPASS: i32 = 3;
pub const MENU_ID_PLAY_STATUS: i32 = 4;
pub const MENU_ID_MIDI_CLK_SOURCE: i32 = 5;
pub const MENU_ID_MIDI_CLK_SEND: i32 = 6;
pub const MENU_ID_SNAPSHOT_PRGCHGE: i32 = 7;
pub const MENU_ID_PB_PRGCHNGE: i32 = 8;
pub const MENU_ID_TEMPO: i32 = 9;
pub const MENU_ID_BEATS_PER_BAR: i32 = 10;
pub const MENU_ID_BYPASS1: i32 = 11;
pub const MENU_ID_BYPASS2: i32 = 12;
pub const MENU_ID_BRIGHTNESS: i32 = 13;
pub const MENU_ID_CURRENT_PROFILE: i32 = 14;
pub const MENU_ID_FOOTSWITCH_NAV: i32 = 30;
pub const MENU_ID_EXP_CV_INPUT: i32 = 40;
pub const MENU_ID_HP_CV_OUTPUT: i32 = 41;
pub const MENU_ID_MASTER_VOL_PORT: i32 = 42;
pub const MENU_ID_EXP_MODE: i32 = 43;
pub const MENU_ID_TOP: i32 = 44;

/// Convert a command wire string to its symbolic name.
pub fn cmd_to_str(cmd: &str) -> &'static str {
    match cmd {
        "pi" => "CMD_PING",
        "say" => "CMD_SAY",
        "l" => "CMD_LED",
        "displ_bright" => "CMD_DISP_BRIGHTNESS",
        "glcd_text" => "CMD_GLCD_TEXT",
        "glcd_dialog" => "CMD_GLCD_DIALOG",
        "glcd_draw" => "CMD_GLCD_DRAW",
        "uc" => "CMD_GUI_CONNECTED",
        "ud" => "CMD_GUI_DISCONNECTED",
        "a" => "CMD_CONTROL_ADD",
        "d" => "CMD_CONTROL_REMOVE",
        "g" => "CMD_CONTROL_GET",
        "s" => "CMD_CONTROL_SET",
        "cps" => "CMD_CONTROL_PARAM_SET",
        "fps" => "CMD_FILE_PARAM_SET",
        "fpc" => "CMD_FILE_PARAM_CURRENT",
        "ncp" => "CMD_CONTROL_PAGE",
        "is" => "CMD_INITIAL_STATE",
        "bchng" => "CMD_BANK_CHANGE",
        "b" => "CMD_BANKS",
        "bn" => "CMD_BANK_NEW",
        "bd" => "CMD_BANK_DELETE",
        "ba" => "CMD_ADD_PBS_TO_BANK",
        "br" => "CMD_REORDER_PBS_IN_BANK",
        "p" => "CMD_PEDALBOARDS",
        "pn" => "CMD_PEDALBOARD_NAME_SET",
        "pchng" => "CMD_PEDALBOARD_CHANGE",
        "pb" => "CMD_PEDALBOARD_LOAD",
        "pr" => "CMD_PEDALBOARD_RESET",
        "ps" => "CMD_PEDALBOARD_SAVE",
        "psa" => "CMD_PEDALBOARD_SAVE_AS",
        "pcl" => "CMD_PEDALBOARD_CLEAR",
        "pbd" => "CMD_PEDALBOARD_DELETE",
        "sr" => "CMD_REORDER_SSS_IN_PB",
        "ssg" => "CMD_SNAPSHOTS",
        "sn" => "CMD_SNAPSHOT_NAME_SET",
        "ssl" => "CMD_SNAPSHOTS_LOAD",
        "sss" => "CMD_SNAPSHOTS_SAVE",
        "ssa" => "CMD_SNAPSHOT_SAVE_AS",
        "ssd" => "CMD_SNAPSHOT_DELETE",
        "ts" => "CMD_TUNER",
        "tn" => "CMD_TUNER_ON",
        "tf" => "CMD_TUNER_OFF",
        "ti" => "CMD_TUNER_INPUT",
        "tr" => "CMD_TUNER_REF_FREQ",
        "restore" => "CMD_RESTORE",
        "screenshot" => "CMD_SCREENSHOT",
        "r" => "CMD_RESPONSE",
        "c" => "CMD_MENU_ITEM_CHANGE",
        "upr" => "CMD_PROFILE_LOAD",
        "ups" => "CMD_PROFILE_STORE",
        "lp" => "CMD_NEXT_PAGE",
        "reset_eeprom" => "CMD_RESET_EEPROM",
        "enc_clicked" => "CMD_SELFTEST_ENCODER_CLICKED",
        "enc_left" => "CMD_SELFTEST_ENCODER_LEFT",
        "enc_right" => "CMD_SELFTEST_ENCODER_RIGHT",
        "button_clicked" => "CMD_SELFTEST_BUTTON_CLICKED",
        "pot_call_check" => "CMD_SELFTEST_CHECK_CALIBRATION",
        "pot_call_ok" => "CMD_SELFTEST_CALLIBRATION_OK",
        "control_skip_enable" => "CMD_SELFTEST_SKIP_CONTROL_ENABLE",
        "control_bad_skip" => "CMD_SELFTEST_SKIP_CONTROL",
        "save_pot_cal" => "CMD_SELFTEST_SAVE_POT_CALIBRATION",
        "sys_gio" => "CMD_SYS_GAIN",
        "sys_ghp" => "CMD_SYS_HP_GAIN",
        "sys_cvi" => "CMD_SYS_CVI_MODE",
        "sys_exp" => "CMD_SYS_EXP_MODE",
        "sys_cvo" => "CMD_SYS_CVO_MODE",
        "sys_ngc" => "CMD_SYS_NG_CHANNEL",
        "sys_ngt" => "CMD_SYS_NG_THRESHOLD",
        "sys_ngd" => "CMD_SYS_NG_DECAY",
        "sys_cmm" => "CMD_SYS_COMP_MODE",
        "sys_cmr" => "CMD_SYS_COMP_RELEASE",
        "sys_pbg" => "CMD_SYS_COMP_PEDALBOARD_GAIN",
        "sys_ams" => "CMD_SYS_AMIXER_SAVE",
        "sys_bts" => "CMD_SYS_BT_STATUS",
        "sys_btd" => "CMD_SYS_BT_DISCOVERY",
        "sys_ctl" => "CMD_SYS_SYSTEMCTL",
        "sys_ver" => "CMD_SYS_VERSION",
        "sys_ser" => "CMD_SYS_SERIAL",
        "sys_usb" => "CMD_SYS_USB_MODE",
        "sys_mnr" => "CMD_SYS_NOISE_REMOVAL",
        "sys_rbt" => "CMD_SYS_REBOOT",
        "sys_lbl" => "CMD_SYS_CHANGE_LED_BLINK",
        "sys_lbh" => "CMD_SYS_CHANGE_LED_BRIGHTNESS",
        "sys_nam" => "CMD_SYS_CHANGE_NAME",
        "sys_uni" => "CMD_SYS_CHANGE_UNIT",
        "sys_val" => "CMD_SYS_CHANGE_VALUE",
        "sys_ind" => "CMD_SYS_CHANGE_WIDGET_INDICATOR",
        "sys_pop" => "CMD_SYS_LAUNCH_POPUP",
        "sys_pch" => "CMD_SYS_PAGE_CHANGE",
        "sys_spc" => "CMD_SYS_SUBPAGE_CHANGE",
        "boot" => "CMD_DUO_BOOT",
        "fn" => "CMD_DUO_FOOT_NAVIG",
        "bc" => "CMD_DUO_BANK_CONFIG",
        "n" => "CMD_DUO_CONTROL_NEXT",
        "si" => "CMD_DUO_CONTROL_INDEX_SET",
        "ss" => "CMD_DUOX_SNAPSHOT_SAVE",
        "sl" => "CMD_DUOX_SNAPSHOT_LOAD",
        "sc" => "CMD_DUOX_SNAPSHOT_CLEAR",
        "pa" => "CMD_DUOX_PAGES_AVAILABLE",
        "s_contrast" => "CMD_DUOX_SET_CONTRAST",
        "exp_overcurrent" => "CMD_DUOX_EXP_OVERCURRENT",
        "cs" => "CMD_DWARF_CONTROL_SUBPAGE",
        _ => "unknown",
    }
}

/// Convert a menu item ID to its symbolic name.
pub fn menu_item_id_to_str(idx: i32) -> &'static str {
    match idx {
        0 => "MENU_ID_SL_IN",
        1 => "MENU_ID_SL_OUT",
        2 => "MENU_ID_TUNER_MUTE",
        3 => "MENU_ID_QUICK_BYPASS",
        4 => "MENU_ID_PLAY_STATUS",
        5 => "MENU_ID_MIDI_CLK_SOURCE",
        6 => "MENU_ID_MIDI_CLK_SEND",
        7 => "MENU_ID_SNAPSHOT_PRGCHGE",
        8 => "MENU_ID_PB_PRGCHNGE",
        9 => "MENU_ID_TEMPO",
        10 => "MENU_ID_BEATS_PER_BAR",
        11 => "MENU_ID_BYPASS1",
        12 => "MENU_ID_BYPASS2",
        13 => "MENU_ID_BRIGHTNESS",
        14 => "MENU_ID_CURRENT_PROFILE",
        30 => "MENU_ID_FOOTSWITCH_NAV",
        40 => "MENU_ID_EXP_CV_INPUT",
        41 => "MENU_ID_HP_CV_OUTPUT",
        42 => "MENU_ID_MASTER_VOL_PORT",
        43 => "MENU_ID_EXP_MODE",
        44 => "MENU_ID_TOP",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_to_str() {
        assert_eq!(cmd_to_str("pi"), "CMD_PING");
        assert_eq!(cmd_to_str("a"), "CMD_CONTROL_ADD");
        assert_eq!(cmd_to_str("sys_rbt"), "CMD_SYS_REBOOT");
        assert_eq!(cmd_to_str("xyz"), "unknown");
    }

    #[test]
    fn test_menu_item_id_to_str() {
        assert_eq!(menu_item_id_to_str(9), "MENU_ID_TEMPO");
        assert_eq!(menu_item_id_to_str(44), "MENU_ID_TOP");
        assert_eq!(menu_item_id_to_str(99), "unknown");
    }

    #[test]
    fn test_cmd_args_has_all_models() {
        let args = cmd_args();
        assert!(args.contains_key("ALL"));
        assert!(args.contains_key("DUO"));
        assert!(args.contains_key("DUOX"));
        assert!(args.contains_key("DWARF"));
    }

    #[test]
    fn test_cmd_args_ping_no_args() {
        let args = cmd_args();
        assert_eq!(args["ALL"]["pi"], vec![]);
    }

    #[test]
    fn test_cmd_args_control_add() {
        use ArgType::*;
        let args = cmd_args();
        assert_eq!(
            args["ALL"]["a"],
            vec![Int, Str, Int, Str, Float, Float, Float, Int, Int]
        );
    }
}

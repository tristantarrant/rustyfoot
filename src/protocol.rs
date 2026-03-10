// Protocol message parser, ported from mod/protocol.py

use std::collections::HashMap;
use std::fmt;

use crate::mod_protocol::{self, ArgType};

// Plugin log levels
pub const PLUGIN_LOG_TRACE: i32 = 0;
pub const PLUGIN_LOG_NOTE: i32 = 1;
pub const PLUGIN_LOG_WARNING: i32 = 2;
pub const PLUGIN_LOG_ERROR: i32 = 3;

/// Protocol error with error code mapping.
#[derive(Debug, Clone)]
pub struct ProtocolError {
    pub err: String,
}

impl ProtocolError {
    pub fn new(err: &str) -> Self {
        Self {
            err: err.to_string(),
        }
    }

    /// Map raw error string to symbolic name.
    pub fn error_name(&self) -> &'static str {
        let cleaned = self.err.replace('\0', "");
        match cleaned.as_str() {
            "-1" => "ERR_INSTANCE_INVALID",
            "-2" => "ERR_INSTANCE_ALREADY_EXISTS",
            "-3" => "ERR_INSTANCE_NON_EXISTS",
            "-4" => "ERR_INSTANCE_UNLICENSED",
            "-101" => "ERR_LV2_INVALID_URI",
            "-102" => "ERR_LV2_INSTANTIATION",
            "-103" => "ERR_LV2_INVALID_PARAM_SYMBOL",
            "-104" => "ERR_LV2_INVALID_PRESET_URI",
            "-105" => "ERR_LV2_CANT_LOAD_STATE",
            "-201" => "ERR_JACK_CLIENT_CREATION",
            "-202" => "ERR_JACK_CLIENT_ACTIVATION",
            "-203" => "ERR_JACK_CLIENT_DEACTIVATION",
            "-204" => "ERR_JACK_PORT_REGISTER",
            "-205" => "ERR_JACK_PORT_CONNECTION",
            "-206" => "ERR_JACK_PORT_DISCONNECTION",
            "-207" => "ERR_JACK_VALUE_OUT_OF_RANGE",
            "-301" => "ERR_ASSIGNMENT_ALREADY_EXISTS",
            "-302" => "ERR_ASSIGNMENT_INVALID_OP",
            "-303" => "ERR_ASSIGNMENT_LIST_FULL",
            "-304" => "ERR_ASSIGNMENT_FAILED",
            "-401" => "ERR_CONTROL_CHAIN_UNAVAILABLE",
            "-402" => "ERR_LINK_UNAVAILABLE",
            "-901" => "ERR_MEMORY_ALLOCATION",
            "-902" => "ERR_INVALID_OPERATION",
            "not found" => "ERR_CMD_NOT_FOUND",
            "wrong arg type" => "ERR_INVALID_ARGUMENTS",
            "few arguments" => "ERR_FEW_ARGUMENTS",
            "many arguments" => "ERR_MANY_ARGUMENTS",
            "finish" => "ERR_FINISH",
            _ => "ERR_UNKNOWN",
        }
    }

    /// Get error code string for responses.
    pub fn error_code(&self) -> String {
        match self.err.parse::<i32>() {
            Ok(n) => format!("resp {}", n),
            Err(_) => self.err.clone(),
        }
    }
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error_name())
    }
}

impl std::error::Error for ProtocolError {}

/// Parsed argument value from protocol messages.
#[derive(Debug, Clone)]
pub enum ArgValue {
    Int(i64),
    Float(f64),
    Str(String),
}

/// Response value from process_resp.
#[derive(Debug, Clone)]
pub enum RespValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    FloatStructure { ok: bool, value: Option<f64> },
    None,
    Raw(String),
}

/// Process a response string according to expected datatype.
pub fn process_resp(resp: Option<&str>, datatype: &str) -> RespValue {
    match resp {
        None => match datatype {
            "boolean" => RespValue::Bool(false),
            "int" => RespValue::Int(0),
            "float_structure" => RespValue::FloatStructure {
                ok: false,
                value: None,
            },
            "string" => RespValue::Str(String::new()),
            _ => RespValue::None,
        },
        Some(resp) => match datatype {
            "float_structure" => {
                let parts: Vec<&str> = resp.split_whitespace().collect();
                let ok = parts
                    .first()
                    .and_then(|s| s.parse::<i64>().ok())
                    .map(|n| n >= 0)
                    .unwrap_or(false);
                let value = parts.get(1).and_then(|s| s.parse::<f64>().ok());
                RespValue::FloatStructure {
                    ok: ok && value.is_some(),
                    value,
                }
            }
            "string" => RespValue::Str(resp.to_string()),
            "boolean" => match resp.parse::<i64>() {
                Ok(n) => RespValue::Bool(n >= 0),
                Err(_) => RespValue::None,
            },
            "int" => match resp.parse::<i64>() {
                Ok(n) => RespValue::Int(n),
                Err(_) => RespValue::None,
            },
            "float" => match resp.parse::<f64>() {
                Ok(n) => RespValue::Float(n),
                Err(_) => RespValue::None,
            },
            _ => match resp.parse::<i64>() {
                Ok(n) => RespValue::Int(n),
                Err(_) => RespValue::None,
            },
        },
    }
}

/// The protocol command registry and message parser.
pub struct Protocol {
    /// Registered command argument types: cmd → [ArgType]
    commands_args: HashMap<String, Vec<ArgType>>,
    /// Registered command callbacks: cmd → callback function
    /// Using Box<dyn Fn> for flexibility; concrete type can be refined later.
    commands_func: HashMap<String, Box<dyn Fn(Vec<ArgValue>, Box<dyn FnOnce(&str)>) + Send + Sync>>,
    /// List of registered commands (for ordering/lookup)
    commands_used: Vec<String>,
}

impl Protocol {
    pub fn new() -> Self {
        Self {
            commands_args: HashMap::new(),
            commands_func: HashMap::new(),
            commands_used: Vec::new(),
        }
    }

    /// Register a command handler for a given model and command.
    pub fn register_cmd_callback<F>(
        &mut self,
        model: &str,
        cmd: &str,
        func: F,
    ) -> Result<(), ProtocolError>
    where
        F: Fn(Vec<ArgValue>, Box<dyn FnOnce(&str)>) + Send + Sync + 'static,
    {
        let all_args = mod_protocol::cmd_args();

        let model_args = all_args
            .get(model)
            .ok_or_else(|| ProtocolError::new(&format!("Model {} is not available", model)))?;

        let arg_types = model_args
            .get(cmd)
            .ok_or_else(|| ProtocolError::new(&format!("Command {} is not available", cmd)))?;

        if self.commands_used.contains(&cmd.to_string()) {
            return Err(ProtocolError::new(&format!(
                "Command {} is already registered",
                cmd
            )));
        }

        self.commands_args.insert(cmd.to_string(), arg_types.clone());
        self.commands_func.insert(cmd.to_string(), Box::new(func));
        self.commands_used.push(cmd.to_string());
        Ok(())
    }

    /// Check if a message is a response.
    pub fn is_resp(msg: &str) -> bool {
        const RESPONSES: &[&str] = &["r", "resp", "few arguments", "many arguments", "not found"];
        RESPONSES.iter().any(|r| msg.starts_with(r))
    }

    /// Parse a protocol message into command and arguments.
    pub fn parse(&self, raw: &str) -> Result<ParsedMessage, ProtocolError> {
        let msg = raw.replace('\0', "");
        let msg = msg.trim();

        if msg.is_empty() {
            return Err(ProtocolError::new("wrong arg type for: ''"));
        }

        if Self::is_resp(msg) {
            return Ok(ParsedMessage {
                msg: msg.to_string(),
                cmd: String::new(),
                args: Vec::new(),
                is_response: true,
            });
        }

        let (cmd, rest) = match msg.find(' ') {
            Some(pos) => (&msg[..pos], Some(&msg[pos + 1..])),
            None => (msg, None),
        };

        if !self.commands_used.contains(&cmd.to_string()) {
            return Err(ProtocolError::new("not found"));
        }

        let expected_args = &self.commands_args[cmd];
        let args = if let Some(rest) = rest {
            // Split into at most N+1 parts where N is the expected arg count
            let parts: Vec<&str> = if expected_args.is_empty() {
                vec![]
            } else {
                rest.splitn(expected_args.len(), ' ')
                    .collect()
            };
            self.parse_args(cmd, &parts, expected_args)?
        } else {
            Vec::new()
        };

        Ok(ParsedMessage {
            msg: msg.to_string(),
            cmd: cmd.to_string(),
            args,
            is_response: false,
        })
    }

    fn parse_args(
        &self,
        cmd: &str,
        parts: &[&str],
        expected: &[ArgType],
    ) -> Result<Vec<ArgValue>, ProtocolError> {
        let mut args = Vec::with_capacity(expected.len());

        for (typ, part) in expected.iter().zip(parts.iter()) {
            let val = match typ {
                ArgType::Int => part
                    .parse::<i64>()
                    .map(ArgValue::Int)
                    .map_err(|_| ProtocolError::new(&format!("wrong arg type for: {} {:?}", cmd, parts)))?,
                ArgType::Float => part
                    .parse::<f64>()
                    .map(ArgValue::Float)
                    .map_err(|_| ProtocolError::new(&format!("wrong arg type for: {} {:?}", cmd, parts)))?,
                ArgType::Str => ArgValue::Str(part.to_string()),
            };
            args.push(val);
        }

        Ok(args)
    }

    /// Run a parsed command's registered callback.
    pub fn run_cmd(&self, parsed: &ParsedMessage, callback: Box<dyn FnOnce(&str)>) {
        if parsed.cmd.is_empty() {
            callback("-1003");
            return;
        }

        let func = match self.commands_func.get(&parsed.cmd) {
            Some(f) => f,
            None => {
                callback("-1003");
                return;
            }
        };

        let expected_len = self.commands_args.get(&parsed.cmd).map(|a| a.len()).unwrap_or(0);
        if parsed.args.len() != expected_len {
            callback("-1003");
            return;
        }

        func(parsed.args.clone(), callback);
    }
}

/// A parsed protocol message.
#[derive(Debug)]
pub struct ParsedMessage {
    pub msg: String,
    pub cmd: String,
    pub args: Vec<ArgValue>,
    pub is_response: bool,
}

impl ParsedMessage {
    /// Process this message as a response of the given datatype.
    pub fn process_resp(&self, datatype: &str) -> RespValue {
        if self.msg.starts_with("r ") {
            let resp = &self.msg[2..];
            process_resp(Some(resp), datatype)
        } else if self.msg.starts_with("resp ") {
            let resp = &self.msg[5..];
            process_resp(Some(resp), datatype)
        } else {
            RespValue::Raw(self.msg.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_error_names() {
        let err = ProtocolError::new("-101");
        assert_eq!(err.error_name(), "ERR_LV2_INVALID_URI");
        assert_eq!(err.error_code(), "resp -101");

        let err = ProtocolError::new("not found");
        assert_eq!(err.error_name(), "ERR_CMD_NOT_FOUND");
        assert_eq!(err.error_code(), "not found");
    }

    #[test]
    fn test_process_resp_boolean() {
        match process_resp(Some("0"), "boolean") {
            RespValue::Bool(v) => assert!(v),
            _ => panic!("expected Bool"),
        }
        match process_resp(Some("-1"), "boolean") {
            RespValue::Bool(v) => assert!(!v),
            _ => panic!("expected Bool"),
        }
    }

    #[test]
    fn test_process_resp_float_structure() {
        match process_resp(Some("0 3.14"), "float_structure") {
            RespValue::FloatStructure { ok, value } => {
                assert!(ok);
                assert!((value.unwrap() - 3.14).abs() < 0.001);
            }
            _ => panic!("expected FloatStructure"),
        }
        match process_resp(Some("-1"), "float_structure") {
            RespValue::FloatStructure { ok, value } => {
                assert!(!ok);
                assert!(value.is_none());
            }
            _ => panic!("expected FloatStructure"),
        }
    }

    #[test]
    fn test_process_resp_none() {
        match process_resp(None, "boolean") {
            RespValue::Bool(false) => {}
            _ => panic!("expected Bool(false)"),
        }
        match process_resp(None, "int") {
            RespValue::Int(0) => {}
            _ => panic!("expected Int(0)"),
        }
    }

    #[test]
    fn test_is_resp() {
        assert!(Protocol::is_resp("r 0"));
        assert!(Protocol::is_resp("resp -1"));
        assert!(Protocol::is_resp("few arguments"));
        assert!(Protocol::is_resp("not found"));
        assert!(!Protocol::is_resp("pi"));
        assert!(!Protocol::is_resp("a 1 test"));
    }

    #[test]
    fn test_parse_and_run() {
        use std::sync::{Arc, Mutex};

        let mut proto = Protocol::new();
        let called = Arc::new(Mutex::new(false));
        let called_clone = called.clone();

        proto
            .register_cmd_callback("ALL", "pi", move |_args, callback| {
                *called_clone.lock().unwrap() = true;
                callback("r 0");
            })
            .unwrap();

        let parsed = proto.parse("pi").unwrap();
        assert_eq!(parsed.cmd, "pi");
        assert!(parsed.args.is_empty());
        assert!(!parsed.is_response);

        proto.run_cmd(&parsed, Box::new(|_| {}));
        assert!(*called.lock().unwrap());
    }

    #[test]
    fn test_parse_response() {
        let proto = Protocol::new();
        let parsed = proto.parse("r 0").unwrap();
        assert!(parsed.is_response);

        match parsed.process_resp("boolean") {
            RespValue::Bool(true) => {}
            other => panic!("expected Bool(true), got {:?}", other),
        }
    }
}
